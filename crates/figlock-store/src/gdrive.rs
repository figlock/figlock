//! Google Drive appDataFolder adapter (`sync.md` 2, ADR-0010).
//!
//! The vault lives in the hidden `appDataFolder`, reached with the `drive.appdata` scope only. The
//! namespace is flat: the full FigLock path (`log/ab12/00000001.op`) is the Drive file name, so there are no
//! folders to create or race on. Drive does not enforce unique names, so create-only is built on top:
//!
//! 1. look the name up; if it exists, the answer is `Exists`;
//! 2. create the file with a random nonce in its `appProperties`;
//! 3. list every file with that name. The oldest wins (by `createdTime`, then id), which every client
//!    computes the same way. If a file with our nonce won, we own the path; otherwise we delete what we
//!    created and report `Exists`.
//!
//! The nonce makes the check safe across retries: a create that failed in an ambiguous way (timeout, 5xx)
//! may or may not have landed, and we find out by looking for our nonce rather than creating again blindly.
//! Duplicates created by someone else are never trusted over the oldest copy, and every read goes through
//! the oldest copy only.
//!
//! Everything Drive returns is treated as untrusted: names must pass [`validate_path`], ids must be plain
//! tokens, sizes are capped, and an upload-session `Location` is only followed if it stays on the API host.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use serde_json::{Value, json};
use url::Url;

use crate::path::{validate_path, validate_prefix};
use crate::{BlobStore, Entry, MAX_BLOB_BYTES, Result, StoreError};

const DEFAULT_BASE: &str = "https://www.googleapis.com";
/// Above this a single multipart request is not allowed by Drive; larger blobs use a resumable session.
const MULTIPART_MAX: usize = 5 * 1024 * 1024;
const RETRY_DELAYS_MS: [u64; 3] = [250, 1000, 3000];
const CREATE_ATTEMPTS: usize = 3;
const MAX_LIST_PAGES: usize = 1000;

/// Supplies OAuth access tokens. The sign-in flow and token storage live elsewhere (FIG-42 follow-up); this
/// adapter only asks for a token and says when one was refused.
pub trait TokenSource: Send + Sync {
    /// A currently valid access token.
    fn access_token(&self) -> Result<String>;
    /// The last token was refused with 401; drop any cached copy so the next call refreshes.
    fn invalidate(&self) {}
}

/// A fixed token, for tests and one-off tooling.
pub struct StaticToken(pub String);

impl TokenSource for StaticToken {
    fn access_token(&self) -> Result<String> {
        Ok(self.0.clone())
    }
}

/// A store in the signed-in user's Google Drive appDataFolder.
pub struct GoogleDrive {
    agent: ureq::Agent,
    base: Url,
    tokens: Box<dyn TokenSource>,
    retry_delays: Vec<Duration>,
    /// Oldest copy of each path as of the last `list`: (id, size). Only a shortcut for `get` so reading a
    /// listed blob costs one request instead of two. A miss or a 404 falls back to a lookup by name, and
    /// every write or delete of a path drops its entry, so a stale id can cost a request but never wrong data.
    listed: Mutex<HashMap<String, (String, u64)>>,
}

/// One Drive file, after validation.
#[derive(Clone, Debug)]
struct Remote {
    id: String,
    name: String,
    size: u64,
    created: String,
    nonce: Option<String>,
}

impl Remote {
    /// Oldest first; ties broken by id so every client orders duplicates identically.
    fn age_key(&self) -> (&str, &str) {
        (&self.created, &self.id)
    }
}

struct Response {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Retry {
    Yes,
    /// Used for creates: the caller reconciles ambiguous failures itself instead of repeating the request.
    No,
}

impl GoogleDrive {
    pub fn new(tokens: impl TokenSource + 'static) -> Self {
        Self::with_endpoint(DEFAULT_BASE, tokens).expect("the default endpoint is valid")
    }

    /// Use another API host (a test double, or a corporate proxy). Plain `http` is accepted so tests can run
    /// against a loopback server; production callers use [`GoogleDrive::new`].
    pub fn with_endpoint(base: &str, tokens: impl TokenSource + 'static) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| StoreError::InvalidPath)?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err(StoreError::InvalidPath);
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // Never follow a redirect: it would carry the bearer token to wherever it points.
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        Ok(GoogleDrive {
            agent,
            base,
            tokens: Box::new(tokens),
            retry_delays: RETRY_DELAYS_MS.map(Duration::from_millis).to_vec(),
            listed: Mutex::new(HashMap::new()),
        })
    }

    /// Shorten the retry backoff (tests).
    #[doc(hidden)]
    pub fn with_retry_delays(mut self, delays: Vec<Duration>) -> Self {
        self.retry_delays = delays;
        self
    }

    // ---- HTTP plumbing -------------------------------------------------------------------------------

    fn url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        let mut u = self.base.clone();
        u.set_path(path);
        u.query_pairs_mut().extend_pairs(query);
        u
    }

    /// One attempt with the current token, refreshing once on 401.
    fn attempt(
        &self,
        method: &str,
        url: &Url,
        headers: &[(&str, &str)],
        body: Option<(&str, &[u8])>,
    ) -> Result<Response> {
        for fresh in [false, true] {
            let token = self.tokens.access_token()?;
            let resp = self.raw(method, url, &token, headers, body)?;
            if resp.status == 401 && !fresh {
                self.tokens.invalidate();
                continue;
            }
            return Ok(resp);
        }
        unreachable!("the loop returns on its second pass")
    }

    fn raw(
        &self,
        method: &str,
        url: &Url,
        token: &str,
        headers: &[(&str, &str)],
        body: Option<(&str, &[u8])>,
    ) -> Result<Response> {
        let auth = format!("Bearer {token}");
        let sent = match (method, body) {
            ("GET", _) => with_headers(self.agent.get(url.as_str()), &auth, headers).call(),
            ("DELETE", _) => with_headers(self.agent.delete(url.as_str()), &auth, headers).call(),
            ("POST", Some((ct, b))) => with_headers(self.agent.post(url.as_str()), &auth, headers)
                .content_type(ct)
                .send(b),
            ("PATCH", Some((ct, b))) => {
                with_headers(self.agent.patch(url.as_str()), &auth, headers)
                    .content_type(ct)
                    .send(b)
            }
            ("PUT", Some((ct, b))) => with_headers(self.agent.put(url.as_str()), &auth, headers)
                .content_type(ct)
                .send(b),
            _ => return Err(StoreError::InvalidPath),
        };
        let mut resp = sent.map_err(|_| StoreError::Unavailable)?;
        let status = resp.status().as_u16();
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_BLOB_BYTES + 1)
            .read_to_vec()
            .map_err(|_| StoreError::Unavailable)?;
        Ok(Response {
            status,
            location,
            body,
        })
    }

    /// Send with retry on throttling, server errors and transport failures. Returns the first response that
    /// is not retryable; the caller maps 2xx and the status codes it expects.
    fn send(
        &self,
        method: &str,
        url: &Url,
        headers: &[(&str, &str)],
        body: Option<(&str, &[u8])>,
        retry: Retry,
    ) -> Result<Response> {
        let mut delays = self.retry_delays.iter();
        loop {
            let outcome = self.attempt(method, url, headers, body);
            let again = match &outcome {
                Ok(r) => is_retryable(r),
                Err(StoreError::Unavailable) => true,
                Err(_) => false,
            };
            if !again || retry == Retry::No {
                return outcome.and_then(check_status);
            }
            match delays.next() {
                Some(d) => sleep(*d),
                None => return outcome.and_then(check_status),
            }
        }
    }

    // ---- Drive operations ----------------------------------------------------------------------------

    /// Every file in the appDataFolder, optionally only those with exactly `name`. Foreign or malformed
    /// entries are dropped; the rest are validated.
    fn list_remote(&self, name: Option<&str>) -> Result<Vec<Remote>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let mut q = vec![
                ("spaces", "appDataFolder"),
                ("pageSize", "1000"),
                (
                    "fields",
                    "nextPageToken,files(id,name,size,createdTime,appProperties)",
                ),
            ];
            let filter;
            if let Some(n) = name {
                // `n` already passed validate_path: lower case letters, digits and `./_-` only.
                filter = format!("name = '{n}' and trashed = false");
            } else {
                filter = "trashed = false".to_owned();
            }
            q.push(("q", &filter));
            if let Some(t) = page.as_deref() {
                q.push(("pageToken", t));
            }
            let resp = self.send(
                "GET",
                &self.url("/drive/v3/files", &q),
                &[],
                None,
                Retry::Yes,
            )?;
            let v: Value =
                serde_json::from_slice(&resp.body).map_err(|_| StoreError::Unavailable)?;
            for f in v["files"].as_array().into_iter().flatten() {
                if let Some(r) = parse_remote(f) {
                    out.push(r);
                }
            }
            match v["nextPageToken"].as_str() {
                Some(t) if !t.is_empty() => page = Some(t.to_owned()),
                _ => return Ok(out),
            }
        }
        // A listing that never ends is a hostile or broken provider, not a big vault.
        Err(StoreError::Unavailable)
    }

    fn forget(&self, path: &str) {
        if let Ok(mut m) = self.listed.lock() {
            m.remove(path);
        }
    }

    fn download(&self, id: &str) -> Result<Vec<u8>> {
        let url = self.url(&format!("/drive/v3/files/{id}"), &[("alt", "media")]);
        let resp = self.send("GET", &url, &[], None, Retry::Yes)?;
        if resp.body.len() as u64 > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        Ok(resp.body)
    }

    fn oldest(&self, name: &str) -> Result<Option<Remote>> {
        let mut all = self.list_remote(Some(name))?;
        all.retain(|r| r.name == name);
        all.sort_by(|a, b| a.age_key().cmp(&b.age_key()));
        Ok(all.into_iter().next())
    }

    fn delete_id(&self, id: &str) -> Result<()> {
        let url = self.url(&format!("/drive/v3/files/{id}"), &[]);
        match self.send("DELETE", &url, &[], None, Retry::Yes) {
            Ok(_) | Err(StoreError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Create a file holding `bytes` under `name`, tagged with `nonce`. Returns nothing: callers re-list,
    /// because the response of an ambiguous failure is not available and the listing is the truth.
    fn create(&self, name: &str, nonce: &str, bytes: &[u8]) -> Result<()> {
        let meta = json!({
            "name": name,
            "parents": ["appDataFolder"],
            "appProperties": { "n": nonce },
        })
        .to_string();
        if bytes.len() <= MULTIPART_MAX {
            let (ct, body) = multipart(&meta, bytes);
            let url = self.url(
                "/upload/drive/v3/files",
                &[("uploadType", "multipart"), ("fields", "id")],
            );
            self.send("POST", &url, &[], Some((&ct, &body)), Retry::No)?;
        } else {
            let url = self.url(
                "/upload/drive/v3/files",
                &[("uploadType", "resumable"), ("fields", "id")],
            );
            self.resumable("POST", &url, &meta, bytes, Retry::No)?;
        }
        Ok(())
    }

    /// Replace the content of an existing file.
    fn update(&self, id: &str, bytes: &[u8]) -> Result<()> {
        let path = format!("/upload/drive/v3/files/{id}");
        if bytes.len() <= MULTIPART_MAX {
            let url = self.url(&path, &[("uploadType", "media"), ("fields", "id")]);
            self.send(
                "PATCH",
                &url,
                &[],
                Some(("application/octet-stream", bytes)),
                Retry::Yes,
            )?;
        } else {
            let url = self.url(&path, &[("uploadType", "resumable"), ("fields", "id")]);
            self.resumable("PATCH", &url, "{}", bytes, Retry::Yes)?;
        }
        Ok(())
    }

    /// Start a resumable session and upload the whole blob in one request. The file only appears in Drive
    /// when the upload completes, so readers still see nothing or the whole blob.
    fn resumable(
        &self,
        method: &str,
        start: &Url,
        meta: &str,
        bytes: &[u8],
        retry: Retry,
    ) -> Result<()> {
        let resp = self.send(
            method,
            start,
            &[("X-Upload-Content-Type", "application/octet-stream")],
            Some(("application/json; charset=UTF-8", meta.as_bytes())),
            retry,
        )?;
        let loc = resp.location.ok_or(StoreError::Unavailable)?;
        let session = Url::parse(&loc).map_err(|_| StoreError::Unavailable)?;
        // The session URL is handed to us by the server and we attach the bearer token to it, so it must
        // stay on the API host.
        if session.scheme() != self.base.scheme()
            || session.host_str() != self.base.host_str()
            || session.port_or_known_default() != self.base.port_or_known_default()
        {
            return Err(StoreError::Unavailable);
        }
        self.send(
            "PUT",
            &session,
            &[],
            Some(("application/octet-stream", bytes)),
            retry,
        )?;
        Ok(())
    }
}

fn with_headers<B>(
    mut req: ureq::RequestBuilder<B>,
    auth: &str,
    headers: &[(&str, &str)],
) -> ureq::RequestBuilder<B> {
    req = req.header("Authorization", auth);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req
}

fn is_retryable(r: &Response) -> bool {
    match r.status {
        408 | 429 | 500..=599 => true,
        // Drive reports throttling as 403 with a rate-limit reason.
        403 => rate_limited(&r.body),
        _ => false,
    }
}

fn rate_limited(body: &[u8]) -> bool {
    reason(body)
        .is_some_and(|r| matches!(r.as_str(), "rateLimitExceeded" | "userRateLimitExceeded"))
}

fn reason(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    v["error"]["errors"][0]["reason"]
        .as_str()
        .map(str::to_owned)
}

/// Map a final HTTP status to the store's error vocabulary. 2xx passes through.
fn check_status(r: Response) -> Result<Response> {
    match r.status {
        200..=299 => Ok(r),
        401 => Err(StoreError::Unauthorized),
        403 if reason(&r.body).as_deref() == Some("storageQuotaExceeded") => {
            Err(StoreError::Io(std::io::ErrorKind::StorageFull))
        }
        403 if rate_limited(&r.body) => Err(StoreError::Unavailable),
        403 => Err(StoreError::Unauthorized),
        404 => Err(StoreError::NotFound),
        408 | 429 | 500..=599 => Err(StoreError::Unavailable),
        _ => Err(StoreError::Unavailable),
    }
}

fn parse_remote(f: &Value) -> Option<Remote> {
    let id = f["id"].as_str()?;
    // Ids go into URL paths: refuse anything that is not a plain token.
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return None;
    }
    let name = f["name"].as_str()?;
    validate_path(name).ok()?;
    let size = f["size"].as_str()?.parse::<u64>().ok()?;
    Some(Remote {
        id: id.to_owned(),
        name: name.to_owned(),
        size,
        created: f["createdTime"].as_str()?.to_owned(),
        nonce: f["appProperties"]["n"].as_str().map(str::to_owned),
    })
}

/// A `multipart/related` body: JSON metadata, then the bytes. The boundary is extended until it cannot
/// appear in the payload.
fn multipart(meta: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let mut n = 0u32;
    let boundary = loop {
        let b = format!("figlock-boundary-{n}");
        if !contains(bytes, b.as_bytes()) && !meta.contains(&b) {
            break b;
        }
        n += 1;
    };
    let mut body = Vec::with_capacity(bytes.len() + meta.len() + 256);
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
    (format!("multipart/related; boundary={boundary}"), body)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn fresh_nonce() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|_| StoreError::Io(std::io::ErrorKind::Other))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

impl BlobStore for GoogleDrive {
    fn get(&self, path: &str) -> Result<Vec<u8>> {
        validate_path(path)?;
        let hit = self.listed.lock().ok().and_then(|m| m.get(path).cloned());
        if let Some((id, size)) = hit {
            if size > MAX_BLOB_BYTES {
                return Err(StoreError::TooLarge);
            }
            match self.download(&id) {
                Err(StoreError::NotFound) => self.forget(path),
                other => return other,
            }
        }
        let file = self.oldest(path)?.ok_or(StoreError::NotFound)?;
        if file.size > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        self.download(&file.id)
    }

    fn put_new(&self, path: &str, bytes: &[u8]) -> Result<()> {
        validate_path(path)?;
        self.forget(path);
        if bytes.len() as u64 > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        // The listing after the create is what decides, so a small blob does not need a lookup first: if the
        // path is taken we lose the race below, delete our copy and report `Exists`. A big one would waste
        // a long upload, so it is looked up first.
        if bytes.len() > MULTIPART_MAX && self.oldest(path)?.is_some() {
            return Err(StoreError::Exists);
        }
        let nonce = fresh_nonce()?;
        for _ in 0..CREATE_ATTEMPTS {
            let created = self.create(path, &nonce, bytes);
            // Only an ambiguous failure can have landed. A refusal (signed out, too large) is final and
            // needs no listing.
            if let Err(e) = created
                && e != StoreError::Unavailable
            {
                return Err(e);
            }
            // Otherwise, whether or not the create reported success, the listing says what landed.
            let mut files = self.list_remote(Some(path))?;
            files.retain(|r| r.name == path);
            files.sort_by(|a, b| a.age_key().cmp(&b.age_key()));
            let mine: Vec<&Remote> = files
                .iter()
                .filter(|r| r.nonce.as_deref() == Some(&nonce))
                .collect();
            match (files.first(), mine.first()) {
                (Some(winner), Some(first_mine)) if winner.id == first_mine.id => {
                    // We own the path. Drop any extra copies our own retries created.
                    for dup in mine.iter().skip(1) {
                        self.delete_id(&dup.id)?;
                    }
                    return Ok(());
                }
                (Some(_), Some(_)) => {
                    for m in &mine {
                        self.delete_id(&m.id)?;
                    }
                    return Err(StoreError::Exists);
                }
                (Some(_), None) => return Err(StoreError::Exists),
                // Nothing landed: the ambiguous failure (or a create that has not shown up yet) is retried.
                (None, _) => {}
            }
        }
        Err(StoreError::Unavailable)
    }

    fn put_replace(&self, path: &str, bytes: &[u8]) -> Result<()> {
        validate_path(path)?;
        self.forget(path);
        if bytes.len() as u64 > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        match self.oldest(path)? {
            Some(f) => self.update(&f.id, bytes),
            None => match self.put_new(path, bytes) {
                Ok(()) => Ok(()),
                // Someone created it between our look and our create: last writer wins, so replace theirs.
                Err(StoreError::Exists) => {
                    let f = self.oldest(path)?.ok_or(StoreError::Unavailable)?;
                    self.update(&f.id, bytes)
                }
                Err(e) => Err(e),
            },
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<Entry>> {
        validate_prefix(prefix)?;
        let mut oldest: BTreeMap<String, Remote> = BTreeMap::new();
        for r in self.list_remote(None)? {
            match oldest.get(&r.name) {
                Some(cur) if cur.age_key() <= r.age_key() => {}
                _ => {
                    oldest.insert(r.name.clone(), r);
                }
            }
        }
        if let Ok(mut m) = self.listed.lock() {
            *m = oldest
                .iter()
                .map(|(n, r)| (n.clone(), (r.id.clone(), r.size)))
                .collect();
        }
        Ok(oldest
            .into_values()
            .filter(|r| r.name.starts_with(prefix))
            .map(|r| Entry {
                path: r.name,
                size: r.size,
                mtime: None,
                available: true,
            })
            .collect())
    }

    fn delete(&self, path: &str) -> Result<()> {
        validate_path(path)?;
        self.forget(path);
        // Every copy goes, not just the oldest, so a duplicate cannot resurface the deleted blob.
        for f in self.list_remote(Some(path))? {
            if f.name == path {
                self.delete_id(&f.id)?;
            }
        }
        Ok(())
    }
}
