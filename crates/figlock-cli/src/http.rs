//! `figlock http`: make one HTTP request through the broker, which adds the item's secret as an auth header
//! for the item's allowed hosts only. The program that runs this never receives the secret
//! (`docs/spec/broker.md` 3.5, ADR-0003: the default way for an agent to use a token).

use std::io::{Read, Write};
use std::path::Path;

use serde_json::json;

use crate::b64;
use crate::broker::client::{call, expect_ok};
use crate::broker::server;
use crate::error::{Fail, Res};

pub struct HttpSpec {
    /// The item's title.
    pub item: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub ttl: String,
    pub reason: Option<String>,
    pub auth_header: Option<String>,
    pub auth_scheme: Option<String>,
    pub field: Option<String>,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub truncated: bool,
}

/// Ask for approval, then send the request through the broker.
pub fn execute(socket: &Path, spec: &HttpSpec) -> Res<Reply> {
    let mut request = json!({
        "scope": {"name": spec.item}, "mode": "http", "ttl": spec.ttl,
        "label": "figlock http",
    });
    if let (Some(o), Some(r)) = (request.as_object_mut(), &spec.reason) {
        o.insert("reason".into(), json!(r));
    }
    let granted = expect_ok(call(socket, "grant.request", request)?)?;
    let bad = || Fail::new("the broker's answer was not valid");
    let token = granted["grant"]["token"].as_str().ok_or_else(bad)?;
    let item = granted["grant"]["scope"]["item"].as_str().ok_or_else(bad)?;

    let mut members = json!({
        "token": token, "item": item, "method": spec.method, "url": spec.url,
    });
    if !spec.headers.is_empty() {
        let h: serde_json::Map<String, serde_json::Value> = spec
            .headers
            .iter()
            .map(|(n, v)| (n.clone(), json!(v)))
            .collect();
        members["headers"] = h.into();
    }
    if let Some(b) = &spec.body {
        members["body"] = json!(b);
    }
    let mut auth = serde_json::Map::new();
    if let Some(h) = &spec.auth_header {
        auth.insert("header".into(), json!(h));
    }
    if let Some(s) = &spec.auth_scheme {
        auth.insert("scheme".into(), json!(s));
    }
    if !auth.is_empty() {
        members["auth"] = auth.into();
    }
    if let Some(f) = &spec.field {
        members["field"] = json!(f);
    }
    let got = expect_ok(call(socket, "http.request", members)?)?;
    let headers = got["headers"]
        .as_object()
        .ok_or_else(bad)?
        .iter()
        .filter_map(|(n, v)| Some((n.clone(), v.as_str()?.to_owned())))
        .collect();
    Ok(Reply {
        status: u16::try_from(got["status"].as_u64().ok_or_else(bad)?).map_err(|_| bad())?,
        headers,
        body: b64::decode(got["body_b64"].as_str().ok_or_else(bad)?).ok_or_else(bad)?,
        truncated: got["truncated"] == true,
    })
}

/// Print the answer like `curl` would and return the exit code.
pub fn show(reply: &Reply, include: bool, fail: bool, out: &mut impl Write) -> Res<i32> {
    if include {
        writeln!(out, "HTTP {}", reply.status)?;
        for (n, v) in &reply.headers {
            writeln!(out, "{n}: {v}")?;
        }
        writeln!(out)?;
    }
    out.write_all(&reply.body)?;
    out.flush()?;
    if reply.truncated {
        eprintln!("note: the response was longer than the broker returns and was cut");
    }
    // curl's code for "HTTP error with --fail".
    Ok(if fail && reply.status >= 400 { 22 } else { 0 })
}

/// A request body given as `@-` is read from standard input.
pub fn read_body(data: &str) -> Res<String> {
    if data != "@-" {
        return Ok(data.to_owned());
    }
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|_| Fail::new("could not read the request body from standard input"))?;
    Ok(s)
}

pub fn run(spec: &HttpSpec, include: bool, fail: bool) -> Res<i32> {
    let reply = execute(&server::socket_path()?, spec)?;
    show(&reply, include, fail, &mut std::io::stdout())
}
