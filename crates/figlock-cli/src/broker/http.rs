//! The outbound half of `http.request` (`docs/spec/broker.md` 3.5): send one request with the secret added,
//! follow redirects by hand so each hop can be checked against the item's host allow-list, and read at most
//! [`MAX_RESPONSE`] bytes back. Policy (who may use which secret where) lives in the broker; this module only
//! asks it, through `allow_hop`, before the secret goes anywhere new.

use std::io::Read;
use std::time::Duration;

use url::Url;
use zeroize::Zeroizing;

/// Redirects followed before the last response is handed back as it is.
pub const MAX_REDIRECTS: usize = 5;
/// The response body is cut here.
pub const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Outgoing {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub auth_header: String,
}

pub struct Incoming {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub truncated: bool,
}

/// `https`, or plain `http` only to the machine itself: a secret must not cross a network in clear text.
pub fn scheme_ok(url: &Url) -> bool {
    url.scheme() == "https" || (url.scheme() == "http" && is_loopback(url))
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d == "localhost",
        Some(url::Host::Ipv4(a)) => a.is_loopback(),
        Some(url::Host::Ipv6(a)) => a.is_loopback(),
        None => false,
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // Redirects are followed here, one checked hop at a time.
        .max_redirects(0)
        .http_status_as_error(false)
        // A proxy in the environment would see the request; keep the secret's path short and known.
        .proxy(None)
        .timeout_global(Some(TIMEOUT))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

fn send(
    agent: &ureq::Agent,
    out: &Outgoing,
    method: &str,
    url: &Url,
    body: Option<&[u8]>,
    secret_header: &str,
) -> Result<Incoming, ()> {
    let mut b = ureq::http::Request::builder()
        .method(method)
        .uri(url.as_str());
    for (n, v) in &out.headers {
        b = b.header(n, v);
    }
    b = b.header(&out.auth_header, secret_header);
    let resp = match body {
        Some(data) => agent.run(b.body(data).map_err(|_| ())?),
        None => agent.run(b.body(()).map_err(|_| ())?),
    }
    .map_err(|_| ())?;
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
        .collect();
    let mut data = Vec::new();
    resp.into_body()
        .into_reader()
        .take(MAX_RESPONSE as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| ())?;
    let truncated = data.len() > MAX_RESPONSE;
    data.truncate(MAX_RESPONSE);
    Ok(Incoming {
        status,
        headers,
        body: data,
        truncated,
    })
}

fn location(r: &Incoming) -> Option<&str> {
    r.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("location"))
        .map(|(_, v)| v.as_str())
}

/// Send `out` with `secret_header` as the value of its auth header. A redirect is followed only when
/// `allow_hop` says its host may receive the secret (it also records the decision); otherwise the redirect
/// response itself is returned and the secret goes no further. The first URL is checked by the caller.
pub fn fetch(
    out: &Outgoing,
    secret_header: &Zeroizing<String>,
    allow_hop: &mut dyn FnMut(&str) -> bool,
) -> Result<Incoming, ()> {
    let agent = agent();
    let mut url = out.url.clone();
    let mut method = out.method.clone();
    let mut body = out.body.as_deref();
    for hop in 0..=MAX_REDIRECTS {
        let resp = send(&agent, out, &method, &url, body, secret_header)?;
        if hop == MAX_REDIRECTS {
            // Too many redirects: the last answer, not an error, and no further request.
            return Ok(resp);
        }
        let follow = matches!(resp.status, 301 | 302 | 303 | 307 | 308);
        let next = follow
            .then(|| location(&resp).and_then(|l| url.join(l).ok()))
            .flatten()
            .filter(|n| scheme_ok(n) && n.username().is_empty() && n.password().is_none())
            .filter(|n| n.host_str().is_some_and(&mut *allow_hop));
        let Some(next) = next else {
            return Ok(resp);
        };
        // What a browser does: a 303, and a 301 or 302 after POST, become a bodyless GET.
        if resp.status == 303 || (matches!(resp.status, 301 | 302) && method == "POST") {
            method = "GET".to_owned();
            body = None;
        }
        url = next;
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_for_this_machine_only() {
        for (url, ok) in [
            ("https://api.github.com/", true),
            ("http://127.0.0.1:8080/", true),
            ("http://localhost/", true),
            ("http://[::1]/", true),
            ("http://api.github.com/", false),
            ("http://127.0.0.1.evil.example/", false),
            ("http://10.0.0.5/", false),
            ("ftp://127.0.0.1/", false),
        ] {
            assert_eq!(scheme_ok(&Url::parse(url).unwrap()), ok, "{url}");
        }
    }

    /// Needs the internet, so it is not part of the normal run: `cargo test -- --ignored https_`.
    #[test]
    #[ignore = "needs the internet"]
    fn https_works_with_the_system_trust_store() {
        let out = Outgoing {
            method: "GET".into(),
            url: Url::parse("https://example.com/").unwrap(),
            headers: vec![],
            body: None,
            auth_header: "Authorization".into(),
        };
        let r = fetch(
            &out,
            &Zeroizing::new("Bearer not-a-real-secret".into()),
            &mut |_| false,
        )
        .expect("an https request");
        assert_eq!(r.status, 200);
        assert!(String::from_utf8_lossy(&r.body).contains("Example Domain"));
    }
}
