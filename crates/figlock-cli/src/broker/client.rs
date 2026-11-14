//! Talking to a running broker from the `figlock broker ...` commands.

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::server::{Line, read_line};
use figlock_session::error::{Fail, Res};

/// Most responses are small; the largest is an `http.request` answer (a 4 MiB body in base64, plus headers).
/// This only bounds a broken or hostile peer.
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
/// A request can wait for the user's answer (60 s) plus slack.
const WAIT: Duration = Duration::from_secs(120);

/// Send one request and return the response object. `members` are the op's own members.
pub fn call(socket: &Path, op: &str, members: Value) -> Res<Value> {
    let mut stream = UnixStream::connect(socket).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            Fail::new("no broker is running; start one with `figlock broker start`")
        }
        _ => Fail::from(e),
    })?;
    stream.set_read_timeout(Some(WAIT))?;
    stream.set_write_timeout(Some(WAIT))?;
    let mut req = match members {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    req.insert("v".into(), json!(1));
    req.insert("id".into(), json!("cli"));
    req.insert("op".into(), json!(op));
    use std::io::Write;
    stream.write_all(Value::Object(req).to_string().as_bytes())?;
    stream.write_all(b"\n")?;
    let mut reader = BufReader::new(stream);
    match read_line(&mut reader, MAX_RESPONSE)? {
        Line::Text(t) => {
            serde_json::from_str(&t).map_err(|_| Fail::new("the broker's answer was not valid"))
        }
        _ => Err(Fail::new("the broker closed the connection")),
    }
}

/// The `error` message of a failed response, or the response itself when it succeeded.
pub fn expect_ok(resp: Value) -> Res<Value> {
    if resp["ok"] == true {
        return Ok(resp);
    }
    let code = resp["error"]["code"].as_str().unwrap_or("error");
    let msg = resp["error"]["message"]
        .as_str()
        .unwrap_or("request failed");
    Err(Fail::new(format!("{msg} ({code})")))
}
