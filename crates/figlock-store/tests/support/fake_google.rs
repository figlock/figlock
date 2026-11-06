//! A stand-in for Google's OAuth token and revoke endpoints, over loopback HTTP. It checks the PKCE proof
//! the way Google does and records what the client sent, so tests can assert on the actual requests.

use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;
use sha2::{Digest, Sha256};
use tiny_http::{Header, Response, Server};

pub struct GoogleState {
    /// Set by the test from the consent URL the client produced.
    pub challenge: Option<String>,
    pub code: String,
    pub refresh_token: Option<String>,
    pub scope: String,
    pub expires_in: u64,
    /// Answer the next token requests with this status and an `invalid_grant` body.
    pub fail_with: Option<u16>,
    pub refresh_calls: usize,
    /// Form bodies received, as (path, params).
    pub seen: Vec<(String, Vec<(String, String)>)>,
}

pub struct FakeGoogle {
    pub state: Arc<Mutex<GoogleState>>,
    pub base: String,
}

impl FakeGoogle {
    pub fn start() -> FakeGoogle {
        let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let state = Arc::new(Mutex::new(GoogleState {
            challenge: None,
            code: "good-code".into(),
            refresh_token: Some("refresh-1".into()),
            scope: "https://www.googleapis.com/auth/drive.appdata".into(),
            expires_in: 3600,
            fail_with: None,
            refresh_calls: 0,
            seen: Vec::new(),
        }));
        let st = state.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                let params: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes())
                    .into_owned()
                    .collect();
                let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                let path = req.url().to_owned();
                let mut s = st.lock().unwrap();
                s.seen.push((path.clone(), params.clone()));
                let (code, out) = if path == "/revoke" {
                    (200, json!({}))
                } else if let Some(c) = s.fail_with {
                    (c, json!({"error": "invalid_grant"}))
                } else {
                    match get("grant_type").as_deref() {
                        Some("authorization_code") => {
                            let proof = get("code_verifier")
                                .map(|v| URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes())));
                            if get("code").as_deref() != Some(&s.code) || proof != s.challenge {
                                (400, json!({"error": "invalid_grant"}))
                            } else {
                                let mut v = json!({"access_token": "good-token", "expires_in": s.expires_in,
                                                   "scope": s.scope, "token_type": "Bearer"});
                                if let Some(r) = &s.refresh_token {
                                    v["refresh_token"] = json!(r);
                                }
                                (200, v)
                            }
                        }
                        Some("refresh_token") => {
                            s.refresh_calls += 1;
                            if get("refresh_token").as_deref() != s.refresh_token.as_deref() {
                                (400, json!({"error": "invalid_grant"}))
                            } else {
                                (
                                    200,
                                    json!({"access_token": "good-token", "expires_in": s.expires_in,
                                             "scope": s.scope, "token_type": "Bearer"}),
                                )
                            }
                        }
                        _ => (400, json!({"error": "unsupported_grant_type"})),
                    }
                };
                drop(s);
                let _ = req.respond(
                    Response::from_data(out.to_string())
                        .with_status_code(code)
                        .with_header(
                            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                                .unwrap(),
                        ),
                );
            }
        });
        FakeGoogle { state, base }
    }

    pub fn oauth(&self) -> figlock_store::GoogleOAuth {
        figlock_store::GoogleOAuth::new("test-client", "test-secret")
            .with_endpoints(
                &format!("{}/auth", self.base),
                &format!("{}/token", self.base),
                &format!("{}/revoke", self.base),
            )
            .unwrap()
    }
}
