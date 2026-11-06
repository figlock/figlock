//! Google sign-in: loopback + PKCE against a fake Google, with a scripted "browser".
#![cfg(feature = "gdrive")]

mod support;

use std::time::Duration;

use figlock_store::{BlobStore, GoogleDrive, StoreError, TokenSource};
use support::fake_drive::FakeDrive;
use support::fake_google::FakeGoogle;
use url::Url;

const WAIT: Duration = Duration::from_secs(10);

/// What the consent URL told the "browser" to do.
struct Consent {
    redirect: String,
    state: String,
    challenge: String,
    query: Vec<(String, String)>,
}

fn parse(consent: &str) -> Consent {
    let u = Url::parse(consent).unwrap();
    let q: Vec<(String, String)> = u.query_pairs().into_owned().collect();
    let get = |k: &str| q.iter().find(|(n, _)| n == k).unwrap().1.clone();
    Consent {
        redirect: get("redirect_uri"),
        state: get("state"),
        challenge: get("code_challenge"),
        query: q,
    }
}

fn visit(url: &str) -> u16 {
    ureq::get(url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .unwrap()
        .status()
        .as_u16()
}

/// Sign in, with `browser` playing the user. It receives the parsed consent URL.
fn sign_in(
    google: &FakeGoogle,
    timeout: Duration,
    browser: impl FnOnce(Consent) + Send + 'static,
) -> figlock_store::Result<figlock_store::Credentials> {
    let g = google.state.clone();
    google.oauth().sign_in(
        move |url| {
            let c = parse(url);
            g.lock().unwrap().challenge = Some(c.challenge.clone());
            std::thread::spawn(move || browser(c));
        },
        timeout,
    )
}

fn approve(c: Consent) {
    visit(&format!("{}?code=good-code&state={}", c.redirect, c.state));
}

#[test]
fn sign_in_returns_a_refresh_token_and_proves_pkce() {
    let google = FakeGoogle::start();
    let seen_consent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sc = seen_consent.clone();
    let creds = sign_in(&google, WAIT, move |c| {
        *sc.lock().unwrap() = c.query.clone();
        approve(c);
    })
    .unwrap();
    assert_eq!(creds.refresh_token, "refresh-1");

    let q = seen_consent.lock().unwrap().clone();
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(
        get("scope"),
        Some("https://www.googleapis.com/auth/drive.appdata"),
        "narrowest scope only"
    );
    assert_eq!(get("code_challenge_method"), Some("S256"));
    assert_eq!(get("response_type"), Some("code"));
    assert_eq!(get("access_type"), Some("offline"));
    assert!(
        get("redirect_uri")
            .unwrap()
            .starts_with("http://127.0.0.1:"),
        "loopback only"
    );
    assert!(get("state").unwrap().len() >= 43, "state is 256 bits");

    // The fake only issues tokens when SHA256(verifier) matches the challenge from the consent URL, so a
    // success already proves the verifier was sent. Check the client credentials went along too.
    let s = google.state.lock().unwrap();
    let (_, params) = &s.seen[0];
    assert!(params.contains(&("client_id".into(), "test-client".into())));
    assert!(params.contains(&("client_secret".into(), "test-secret".into())));
}

#[test]
fn a_callback_without_our_state_is_ignored_and_does_not_end_the_flow() {
    let google = FakeGoogle::start();
    let creds = sign_in(&google, WAIT, |c| {
        assert_eq!(visit(&format!("{}?code=evil&state=wrong", c.redirect)), 400);
        assert_eq!(visit(&format!("{}?code=evil", c.redirect)), 400);
        assert_eq!(
            visit(&format!("{}other?code=evil&state={}", c.redirect, c.state)),
            400,
            "wrong path"
        );
        approve(c);
    })
    .unwrap();
    assert_eq!(creds.refresh_token, "refresh-1");
    let s = google.state.lock().unwrap();
    assert_eq!(s.seen.len(), 1, "the forged codes never reached Google");
}

#[test]
fn declining_consent_is_unauthorized() {
    let google = FakeGoogle::start();
    let r = sign_in(&google, WAIT, |c| {
        visit(&format!(
            "{}?error=access_denied&state={}",
            c.redirect, c.state
        ));
    });
    assert_eq!(r.unwrap_err(), StoreError::Unauthorized);
    assert!(google.state.lock().unwrap().seen.is_empty());
}

#[test]
fn no_browser_means_a_timeout() {
    let google = FakeGoogle::start();
    let r = sign_in(&google, Duration::from_millis(300), |_| {});
    assert_eq!(r.unwrap_err(), StoreError::Io(std::io::ErrorKind::TimedOut));
}

#[test]
fn a_grant_broader_than_drive_appdata_is_refused() {
    let google = FakeGoogle::start();
    google.state.lock().unwrap().scope =
        "https://www.googleapis.com/auth/drive.appdata https://www.googleapis.com/auth/drive"
            .into();
    assert_eq!(
        sign_in(&google, WAIT, approve).unwrap_err(),
        StoreError::Unauthorized
    );
    google.state.lock().unwrap().scope = "https://www.googleapis.com/auth/drive".into();
    assert_eq!(
        sign_in(&google, WAIT, approve).unwrap_err(),
        StoreError::Unauthorized
    );
}

#[test]
fn a_response_without_a_refresh_token_is_refused() {
    let google = FakeGoogle::start();
    google.state.lock().unwrap().refresh_token = None;
    assert_eq!(
        sign_in(&google, WAIT, approve).unwrap_err(),
        StoreError::Unauthorized
    );
}

#[test]
fn credentials_do_not_print_the_token() {
    let google = FakeGoogle::start();
    let creds = sign_in(&google, WAIT, approve).unwrap();
    assert!(!format!("{creds:?}").contains("refresh-1"));
    let src = google.oauth().token_source(creds);
    assert!(!format!("{src:?}").contains("refresh-1"));
}

fn token_source(google: &FakeGoogle) -> figlock_store::RefreshingToken {
    let creds = sign_in(google, WAIT, approve).unwrap();
    google.oauth().token_source(creds)
}

#[test]
fn access_tokens_are_cached_until_near_expiry_and_refreshed_on_invalidate() {
    let google = FakeGoogle::start();
    let src = token_source(&google);
    assert_eq!(src.access_token().unwrap(), "good-token");
    assert_eq!(src.access_token().unwrap(), "good-token");
    assert_eq!(
        google.state.lock().unwrap().refresh_calls,
        1,
        "second call came from the cache"
    );
    src.invalidate();
    src.access_token().unwrap();
    assert_eq!(google.state.lock().unwrap().refresh_calls, 2);
}

#[test]
fn a_token_about_to_expire_is_not_reused() {
    let google = FakeGoogle::start();
    google.state.lock().unwrap().expires_in = 30; // inside the safety margin
    let src = token_source(&google);
    src.access_token().unwrap();
    src.access_token().unwrap();
    assert_eq!(google.state.lock().unwrap().refresh_calls, 2);
}

#[test]
fn concurrent_callers_share_one_refresh() {
    let google = FakeGoogle::start();
    let src = std::sync::Arc::new(token_source(&google));
    let hs: Vec<_> = (0..8)
        .map(|_| {
            let s = src.clone();
            std::thread::spawn(move || s.access_token().unwrap())
        })
        .collect();
    for h in hs {
        assert_eq!(h.join().unwrap(), "good-token");
    }
    assert_eq!(google.state.lock().unwrap().refresh_calls, 1);
}

#[test]
fn a_revoked_refresh_token_is_unauthorized_and_an_outage_is_unavailable() {
    let google = FakeGoogle::start();
    let src = token_source(&google);
    google.state.lock().unwrap().fail_with = Some(400);
    assert_eq!(src.access_token().unwrap_err(), StoreError::Unauthorized);
    google.state.lock().unwrap().fail_with = Some(503);
    assert_eq!(src.access_token().unwrap_err(), StoreError::Unavailable);
    google.state.lock().unwrap().fail_with = None;
    assert_eq!(
        src.access_token().unwrap(),
        "good-token",
        "recovers once Google does"
    );
}

#[test]
fn revoke_posts_the_token_and_tolerates_an_already_dead_one() {
    let google = FakeGoogle::start();
    google.oauth().revoke("refresh-1").unwrap();
    let s = google.state.lock().unwrap();
    let (path, params) = s.seen.last().unwrap();
    assert_eq!(path, "/revoke");
    assert!(params.contains(&("token".into(), "refresh-1".into())));
}

#[test]
fn sign_in_then_use_the_vault_store_end_to_end() {
    let google = FakeGoogle::start();
    let drive_fake = FakeDrive::start();
    let src = token_source(&google);
    let drive = GoogleDrive::with_endpoint(&drive_fake.base, src).unwrap();
    drive.put_new("vault.json", b"{}").unwrap();
    assert_eq!(drive.get("vault.json").unwrap(), b"{}");
}
