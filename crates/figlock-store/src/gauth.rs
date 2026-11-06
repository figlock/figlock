//! Google sign-in for the Drive adapter: OAuth 2.0 for installed apps with a loopback redirect and PKCE.
//!
//! `sign_in` opens nothing itself; it hands the consent URL to the caller, waits on a one-shot listener bound
//! to `127.0.0.1`, and trades the returned code for a refresh token. The listener only accepts a request that
//! carries our random `state`, so a stray page or another local program cannot complete or poison the flow.
//! PKCE (S256) means a stolen code is useless without the verifier, which never leaves this process.
//!
//! Only the `drive.appdata` scope is requested (ADR-0010), and a token response that grants anything else is
//! refused. The client ID and secret identify the app, not the user; Google documents the secret of an
//! installed-app client as non-confidential, so the defaults are compiled in and can be overridden with
//! `FIGLOCK_GOOGLE_CLIENT_ID` / `FIGLOCK_GOOGLE_CLIENT_SECRET` (for forks, or to spread API quota).
//!
//! The refresh token is the long-lived credential. It is returned to the caller to store; this module never
//! writes it anywhere and never puts it in an error or a `Debug` string.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::gdrive::TokenSource;
use crate::{Result, StoreError};

/// The only scope FigLock ever asks for.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata";

/// The FigLock Desktop OAuth client (Google Cloud project `figlock-prod`, FIG-40).
const DEFAULT_CLIENT_ID: &str =
    "994923112244-m67ar9qhp123m2meq1l2g39d0m7h3icm.apps.googleusercontent.com";
/// Non-confidential for installed apps (see the module docs). Empty until the real value is filled in.
const DEFAULT_CLIENT_SECRET: &str = "";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";

/// Refresh this long before the access token actually expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);
/// Largest request the loopback listener will read.
const MAX_CALLBACK_BYTES: usize = 8 * 1024;

/// What a successful sign-in yields. The caller stores the refresh token (owner-only, never in logs).
pub struct Credentials {
    pub refresh_token: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials(..)")
    }
}

/// An OAuth client for Google's endpoints.
pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
    auth_url: Url,
    token_url: Url,
    revoke_url: Url,
    agent: ureq::Agent,
}

impl GoogleOAuth {
    pub fn new(client_id: &str, client_secret: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        GoogleOAuth {
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            auth_url: Url::parse(AUTH_URL).expect("constant"),
            token_url: Url::parse(TOKEN_URL).expect("constant"),
            revoke_url: Url::parse(REVOKE_URL).expect("constant"),
            agent,
        }
    }

    /// The built-in client, or the one named by `FIGLOCK_GOOGLE_CLIENT_ID` and `FIGLOCK_GOOGLE_CLIENT_SECRET`.
    /// `None` when no secret is configured at all.
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let id = var("FIGLOCK_GOOGLE_CLIENT_ID").unwrap_or_else(|| DEFAULT_CLIENT_ID.to_owned());
        let secret =
            var("FIGLOCK_GOOGLE_CLIENT_SECRET").unwrap_or_else(|| DEFAULT_CLIENT_SECRET.to_owned());
        (!secret.is_empty()).then(|| Self::new(&id, &secret))
    }

    /// Point at other endpoints (a test double). Plain `http` is accepted for loopback tests only.
    pub fn with_endpoints(mut self, auth: &str, token: &str, revoke: &str) -> Result<Self> {
        let parse = |s: &str| Url::parse(s).map_err(|_| StoreError::InvalidPath);
        self.auth_url = parse(auth)?;
        self.token_url = parse(token)?;
        self.revoke_url = parse(revoke)?;
        Ok(self)
    }

    /// Run the browser sign-in. `show_url` receives the consent URL (print it, open it, or both); this call
    /// then blocks until the browser comes back, the user declines, or `timeout` passes.
    pub fn sign_in(&self, show_url: impl FnOnce(&str), timeout: Duration) -> Result<Credentials> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let redirect = format!("http://127.0.0.1:{port}/");

        let verifier = random_token()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_token()?;

        let mut consent = self.auth_url.clone();
        consent
            .query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPE)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            // Offline access plus a forced consent screen is what makes Google return a refresh token.
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
        show_url(consent.as_str());

        let code = wait_for_code(&listener, &state, Instant::now() + timeout)?;
        let tokens = self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
        ])?;
        check_scope(&tokens)?;
        let refresh_token = tokens["refresh_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or(StoreError::Unauthorized)?
            .to_owned();
        Ok(Credentials { refresh_token })
    }

    /// A [`TokenSource`] that mints access tokens from a stored refresh token.
    pub fn token_source(self, creds: Credentials) -> RefreshingToken {
        RefreshingToken {
            oauth: self,
            refresh_token: creds.refresh_token,
            cached: Mutex::new(None),
        }
    }

    /// Tell Google to invalidate a refresh token (sign-out). Best effort: an already-dead token is fine.
    pub fn revoke(&self, refresh_token: &str) -> Result<()> {
        let body = form(&[("token", refresh_token)]);
        let resp = self
            .agent
            .post(self.revoke_url.as_str())
            .content_type("application/x-www-form-urlencoded")
            .send(body.as_bytes())
            .map_err(|_| StoreError::Unavailable)?;
        match resp.status().as_u16() {
            200..=299 | 400 => Ok(()),
            401 | 403 => Err(StoreError::Unauthorized),
            _ => Err(StoreError::Unavailable),
        }
    }

    /// POST to the token endpoint with our client credentials and parse the JSON answer.
    fn token_request(&self, params: &[(&str, &str)]) -> Result<Value> {
        let mut all = vec![
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ];
        all.extend_from_slice(params);
        let body = form(&all);
        let mut resp = self
            .agent
            .post(self.token_url.as_str())
            .content_type("application/x-www-form-urlencoded")
            .send(body.as_bytes())
            .map_err(|_| StoreError::Unavailable)?;
        let status = resp.status().as_u16();
        let bytes = resp
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_vec()
            .map_err(|_| StoreError::Unavailable)?;
        match status {
            200..=299 => serde_json::from_slice(&bytes).map_err(|_| StoreError::Unavailable),
            // `invalid_grant` is a revoked or expired refresh token, `invalid_client` a bad app registration:
            // either way signing in again (or fixing the client) is the remedy, not retrying.
            400 | 401 | 403 => Err(StoreError::Unauthorized),
            _ => Err(StoreError::Unavailable),
        }
    }
}

/// Mints and caches access tokens from a refresh token.
pub struct RefreshingToken {
    oauth: GoogleOAuth,
    refresh_token: String,
    cached: Mutex<Option<(String, Instant)>>,
}

impl TokenSource for RefreshingToken {
    fn access_token(&self) -> Result<String> {
        // Held across the refresh so concurrent callers share one network round trip.
        let mut cached = self.cached.lock().map_err(|_| StoreError::Unavailable)?;
        if let Some((token, expires)) = cached.as_ref()
            && Instant::now() + EXPIRY_MARGIN < *expires
        {
            return Ok(token.clone());
        }
        let v = self.oauth.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &self.refresh_token),
        ])?;
        check_scope(&v)?;
        let token = v["access_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or(StoreError::Unavailable)?;
        let lifetime = v["expires_in"].as_u64().unwrap_or(0).min(24 * 3600);
        *cached = Some((
            token.to_owned(),
            Instant::now() + Duration::from_secs(lifetime),
        ));
        Ok(token.to_owned())
    }

    fn invalidate(&self) {
        if let Ok(mut c) = self.cached.lock() {
            *c = None;
        }
    }
}

impl fmt::Debug for RefreshingToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshingToken(..)")
    }
}

/// The grant must be exactly `drive.appdata`. Anything broader means something other than what we asked for.
fn check_scope(tokens: &Value) -> Result<()> {
    match tokens["scope"].as_str() {
        Some(s) if s.split_whitespace().eq([SCOPE]) => Ok(()),
        _ => Err(StoreError::Unauthorized),
    }
}

fn form(params: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish()
}

/// 32 random bytes as URL-safe text: a valid PKCE verifier (43 characters) and a good `state`.
fn random_token() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|_| StoreError::Io(std::io::ErrorKind::Other))?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}

/// Accept connections until one carries our `state` with a code (or an error), or the deadline passes.
fn wait_for_code(listener: &TcpListener, state: &str, deadline: Instant) -> Result<String> {
    listener.set_nonblocking(true)?;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(outcome) = handle_callback(stream, state) {
                    return outcome;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(StoreError::Io(std::io::ErrorKind::TimedOut));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// `Some` when this request was the genuine callback; `None` for anything else, which is answered and ignored.
fn handle_callback(mut stream: TcpStream, state: &str) -> Option<Result<String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_CALLBACK_BYTES {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (method, target) = (parts.next(), parts.next());
    let params = (method == Some("GET"))
        .then_some(target)
        .flatten()
        .and_then(|t| Url::parse(&format!("http://127.0.0.1{t}")).ok())
        .filter(|u| u.path() == "/")
        .map(|u| u.query_pairs().into_owned().collect::<Vec<_>>());
    let get = |k: &str| {
        params
            .as_ref()
            .and_then(|p| p.iter().find(|(n, _)| n == k))
            .map(|(_, v)| v.as_str())
    };

    // Constant-time compare is unnecessary here: the state is a one-shot value on a loopback socket that is
    // closed as soon as the flow ends.
    if get("state") != Some(state) {
        respond(
            &mut stream,
            400,
            "This is not the FigLock sign-in response.",
        );
        return None;
    }
    if get("error").is_some() {
        respond(
            &mut stream,
            200,
            "Sign-in was cancelled. You can close this tab and return to FigLock.",
        );
        return Some(Err(StoreError::Unauthorized));
    }
    match get("code").filter(|c| !c.is_empty()) {
        Some(code) => {
            respond(
                &mut stream,
                200,
                "FigLock is signed in to Google Drive. You can close this tab.",
            );
            Some(Ok(code.to_owned()))
        }
        None => {
            respond(&mut stream, 400, "The sign-in response had no code.");
            None
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>FigLock</title><body style=\"font:16px system-ui;margin:3em\"><p>{message}</p>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}
