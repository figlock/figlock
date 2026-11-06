//! A small in-process stand-in for the Google Drive v3 API: just the appDataFolder calls the adapter
//! makes, over real HTTP on loopback, so the adapter's actual requests are exercised. It also carries the
//! fault knobs the negative tests need (revoked token, throttling, hostile responses).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

#[derive(Clone)]
pub struct StoredFile {
    pub id: String,
    pub name: String,
    pub created: String,
    pub bytes: Vec<u8>,
    pub nonce: Option<String>,
}

#[derive(Default)]
pub struct State {
    pub files: Vec<StoredFile>,
    pub counter: usize,
    /// The only bearer token the server accepts.
    pub token: String,
    /// Answer this many upcoming requests with this status before behaving.
    pub fail_next: Vec<u16>,
    /// Page size used for listings, whatever the client asked for.
    pub page_size: usize,
    /// Pending resumable sessions: session id -> (file id to update, or new file name + nonce).
    sessions: HashMap<String, Session>,
    /// Where the resumable session URL points; tests set a foreign host to check it is refused.
    pub session_host: Option<String>,
    /// After the next create, answer 503 even though the file was stored (an ambiguous failure).
    pub lose_next_create_response: bool,
    pub requests: Vec<(String, String)>,
    pub max_multipart_seen: usize,
}

enum Session {
    Create { name: String, nonce: Option<String> },
    Update { id: String },
}

pub struct FakeDrive {
    pub state: Arc<Mutex<State>>,
    pub base: String,
    hits: Arc<AtomicUsize>,
}

impl FakeDrive {
    pub fn start() -> FakeDrive {
        let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let state = Arc::new(Mutex::new(State {
            token: "good-token".into(),
            page_size: 1000,
            ..State::default()
        }));
        let hits = Arc::new(AtomicUsize::new(0));
        let (s, st, b, h) = (server.clone(), state.clone(), base.clone(), hits.clone());
        std::thread::spawn(move || {
            for req in s.incoming_requests() {
                let (st, b) = (st.clone(), b.clone());
                h.fetch_add(1, Ordering::Relaxed);
                std::thread::spawn(move || handle(req, &st, &b));
            }
        });
        FakeDrive { state, base, hits }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<_> = self
            .state
            .lock()
            .unwrap()
            .files
            .iter()
            .map(|f| f.name.clone())
            .collect();
        v.sort();
        v
    }

    /// Plant a file as another client (or a hostile account owner) would, with no nonce.
    pub fn plant(&self, name: &str, bytes: &[u8]) {
        let mut s = self.state.lock().unwrap();
        let f = new_file(&mut s, name, None, bytes.to_vec());
        s.files.push(f);
    }
}

fn new_file(s: &mut State, name: &str, nonce: Option<String>, bytes: Vec<u8>) -> StoredFile {
    s.counter += 1;
    let c = s.counter;
    StoredFile {
        id: format!("id{c}"),
        name: name.to_owned(),
        created: format!(
            "2026-01-01T{:02}:{:02}:{:02}.000Z",
            c / 3600,
            (c / 60) % 60,
            c % 60
        ),
        bytes,
        nonce,
    }
}

fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(v.to_string())
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
}

fn err(code: u16, reason: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_resp(
        code,
        json!({"error": {"code": code, "errors": [{"reason": reason}]}}),
    )
}

fn handle(mut req: Request, state: &Arc<Mutex<State>>, base: &str) {
    let method = req.method().clone();
    let url = req.url().to_owned();
    let auth = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_owned())
        .unwrap_or_default();
    let ctype = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Content-Type"))
        .map(|h| h.value.as_str().to_owned())
        .unwrap_or_default();
    let mut body = Vec::new();
    req.as_reader().read_to_end(&mut body).unwrap();

    let parsed = url::Url::parse(&format!("http://x{url}")).unwrap();
    let path = parsed.path().to_owned();
    let q: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    let resp = {
        let mut s = state.lock().unwrap();
        s.requests.push((method.to_string(), path.clone()));
        if let Some(code) = (!s.fail_next.is_empty()).then(|| s.fail_next.remove(0)) {
            let r = if code == 403 {
                err(403, "rateLimitExceeded")
            } else {
                err(code, "backendError")
            };
            drop(s);
            let _ = req.respond(r);
            return;
        }
        if auth != format!("Bearer {}", s.token) {
            drop(s);
            let _ = req.respond(err(401, "authError"));
            return;
        }
        route(&mut s, &method, &path, &q, &ctype, &body, base)
    };
    let _ = req.respond(resp);
}

fn route(
    s: &mut State,
    method: &Method,
    path: &str,
    q: &HashMap<String, String>,
    ctype: &str,
    body: &[u8],
    base: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    match (method, path) {
        (Method::Get, "/drive/v3/files") => {
            assert_eq!(q.get("spaces").map(String::as_str), Some("appDataFolder"));
            let name = q.get("q").and_then(|q| {
                let rest = q.strip_prefix("name = '")?;
                Some(rest[..rest.find('\'')?].to_owned())
            });
            let mut matches: Vec<&StoredFile> = s
                .files
                .iter()
                .filter(|f| name.as_ref().is_none_or(|n| &f.name == n))
                .collect();
            matches.sort_by(|a, b| a.id.cmp(&b.id));
            let start: usize = q.get("pageToken").and_then(|t| t.parse().ok()).unwrap_or(0);
            let end = (start + s.page_size).min(matches.len());
            let page: Vec<Value> = matches[start..end]
                .iter()
                .map(|f| {
                    json!({"id": f.id, "name": f.name, "size": f.bytes.len().to_string(),
                           "createdTime": f.created, "appProperties": f.nonce.as_ref().map(|n| json!({"n": n})).unwrap_or(json!({}))})
                })
                .collect();
            let mut out = json!({ "files": page });
            if end < matches.len() {
                out["nextPageToken"] = json!(end.to_string());
            }
            json_resp(200, out)
        }
        (Method::Get, p) if p.starts_with("/drive/v3/files/") => {
            let id = &p["/drive/v3/files/".len()..];
            match s.files.iter().find(|f| f.id == id) {
                Some(f) => Response::from_data(f.bytes.clone()).with_status_code(200),
                None => err(404, "notFound"),
            }
        }
        (Method::Delete, p) if p.starts_with("/drive/v3/files/") => {
            let id = &p["/drive/v3/files/".len()..];
            let before = s.files.len();
            s.files.retain(|f| f.id != id);
            if s.files.len() == before {
                err(404, "notFound")
            } else {
                Response::from_data(vec![]).with_status_code(204)
            }
        }
        (Method::Post, "/upload/drive/v3/files") => match q.get("uploadType").map(String::as_str) {
            Some("multipart") => {
                let (meta, media) = parse_multipart(ctype, body);
                s.max_multipart_seen = s.max_multipart_seen.max(media.len());
                assert_eq!(
                    meta["parents"],
                    json!(["appDataFolder"]),
                    "only the appDataFolder is ever used"
                );
                let name = meta["name"].as_str().unwrap().to_owned();
                let nonce = meta["appProperties"]["n"].as_str().map(str::to_owned);
                let f = new_file(s, &name, nonce, media);
                let id = f.id.clone();
                s.files.push(f);
                if std::mem::take(&mut s.lose_next_create_response) {
                    return err(503, "backendError");
                }
                json_resp(200, json!({"id": id}))
            }
            Some("resumable") => {
                let meta: Value = serde_json::from_slice(body).unwrap();
                let sid = format!("sess{}", s.sessions.len() + 1);
                s.sessions.insert(
                    sid.clone(),
                    Session::Create {
                        name: meta["name"].as_str().unwrap().to_owned(),
                        nonce: meta["appProperties"]["n"].as_str().map(str::to_owned),
                    },
                );
                session_response(s, base, &sid)
            }
            other => panic!("unexpected uploadType {other:?}"),
        },
        (Method::Patch, p) if p.starts_with("/upload/drive/v3/files/") => {
            let id = p["/upload/drive/v3/files/".len()..].to_owned();
            match q.get("uploadType").map(String::as_str) {
                Some("media") => match s.files.iter_mut().find(|f| f.id == id) {
                    Some(f) => {
                        f.bytes = body.to_vec();
                        json_resp(200, json!({"id": id}))
                    }
                    None => err(404, "notFound"),
                },
                Some("resumable") => {
                    let sid = format!("sess{}", s.sessions.len() + 1);
                    s.sessions.insert(sid.clone(), Session::Update { id });
                    session_response(s, base, &sid)
                }
                other => panic!("unexpected uploadType {other:?}"),
            }
        }
        (Method::Put, p) if p.starts_with("/session/") => {
            let sid = &p["/session/".len()..];
            match s.sessions.remove(sid) {
                Some(Session::Create { name, nonce }) => {
                    let f = new_file(s, &name, nonce, body.to_vec());
                    let id = f.id.clone();
                    s.files.push(f);
                    json_resp(200, json!({"id": id}))
                }
                Some(Session::Update { id }) => match s.files.iter_mut().find(|f| f.id == id) {
                    Some(f) => {
                        f.bytes = body.to_vec();
                        json_resp(200, json!({"id": id}))
                    }
                    None => err(404, "notFound"),
                },
                None => err(404, "notFound"),
            }
        }
        _ => err(404, "notFound"),
    }
}

fn session_response(s: &State, base: &str, sid: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let host = s.session_host.clone().unwrap_or_else(|| base.to_owned());
    Response::from_data(vec![])
        .with_status_code(200)
        .with_header(
            Header::from_bytes(&b"Location"[..], format!("{host}/session/{sid}").as_bytes())
                .unwrap(),
        )
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn parse_multipart(ctype: &str, body: &[u8]) -> (Value, Vec<u8>) {
    let boundary = ctype.split("boundary=").nth(1).expect("multipart boundary");
    let delim = format!("--{boundary}");
    let first = find(body, delim.as_bytes(), 0).unwrap();
    let h1 = find(body, b"\r\n\r\n", first).unwrap() + 4;
    let e1 = find(body, format!("\r\n{delim}").as_bytes(), h1).unwrap();
    let meta: Value = serde_json::from_slice(&body[h1..e1]).unwrap();
    let h2 = find(body, b"\r\n\r\n", e1).unwrap() + 4;
    let e2 = body.len() - format!("\r\n{delim}--").len();
    assert_eq!(
        &body[e2..],
        format!("\r\n{delim}--").as_bytes(),
        "closing boundary"
    );
    (meta, body[h2..e2].to_vec())
}
