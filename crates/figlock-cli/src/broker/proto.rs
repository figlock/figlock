//! The broker wire protocol (`docs/spec/broker.md` 3): one JSON object per line, parsed strictly.

use figlock_core::ItemId;
use serde_json::{Map, Value, json};

/// One request line is at most this many bytes, newline excluded. Room for an HTTP body (`MAX_BODY`) with
/// JSON escaping; everything else in a request is tiny.
pub const MAX_LINE: usize = 512 * 1024;
/// The body of an `http.request`, in bytes.
pub const MAX_BODY: usize = 128 * 1024;
const MAX_URL: usize = 4096;
const MAX_HEADERS: usize = 32;
const MAX_ID: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Http,
    Run,
    Reveal,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Http => "http",
            Mode::Run => "run",
            Mode::Reveal => "reveal",
        }
    }

    fn parse(s: &str) -> Option<Mode> {
        match s {
            "http" => Some(Mode::Http),
            "run" => Some(Mode::Run),
            "reveal" => Some(Mode::Reveal),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ttl {
    Once,
    Min15,
    Session,
    Day,
    UntilLock,
}

impl Ttl {
    pub fn as_str(self) -> &'static str {
        match self {
            Ttl::Once => "once",
            Ttl::Min15 => "15m",
            Ttl::Session => "session",
            Ttl::Day => "1d",
            Ttl::UntilLock => "until-lock",
        }
    }

    fn parse(s: &str) -> Option<Ttl> {
        match s {
            "once" => Some(Ttl::Once),
            "15m" => Some(Ttl::Min15),
            "session" => Some(Ttl::Session),
            "1d" => Some(Ttl::Day),
            "until-lock" => Some(Ttl::UntilLock),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Item(ItemId),
    Tag(String),
    Vault,
    /// An item named by its title. Only in a request: the broker resolves it to `Item` before anything
    /// else, so a grant never holds one.
    Title(String),
}

impl Scope {
    /// The form used in the audit log and in grant lists.
    pub fn label(&self) -> String {
        match self {
            Scope::Item(id) => format!("item:{}", hex::encode(id)),
            Scope::Tag(t) => format!("tag:{t}"),
            Scope::Vault => "vault".to_owned(),
            Scope::Title(_) => "title".to_owned(),
        }
    }

    /// The form used in requests and responses.
    pub fn to_json(&self) -> Value {
        match self {
            Scope::Item(id) => json!({"item": hex::encode(id)}),
            Scope::Tag(t) => json!({"tag": t}),
            Scope::Vault => json!({"vault": true}),
            Scope::Title(t) => json!({"name": t}),
        }
    }
}

pub struct GrantRequest {
    pub scope: Scope,
    pub mode: Mode,
    pub ttl: Ttl,
    /// What the caller says it is. Never trusted; shown after the verified identity.
    pub label: Option<String>,
    pub reason: Option<String>,
}

pub enum Op {
    Hello,
    Status,
    GrantRequest(GrantRequest),
    GrantList,
    GrantRevoke {
        grant: String,
    },
    /// Environment variables for a program the caller starts (`run` mode). With no `vars` the item's own
    /// defaults apply: a token item gives its `env_var`, an env bundle gives every `env/NAME`.
    RunEnv {
        token: String,
        item: ItemId,
        vars: Vec<(String, String)>,
    },
    HttpRequest(HttpRequest),
    Lock,
}

/// What the caller wants sent. The broker adds the secret; nothing here is trusted beyond its shape.
#[derive(Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub token: String,
    pub item: ItemId,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// Header that carries the secret, and the word before it (`Bearer`); empty scheme sends the bare value.
    pub auth_header: String,
    pub auth_scheme: String,
    /// The item field that holds the secret.
    pub field: String,
}

/// At most this many variables in one `run.env` request.
pub const MAX_VARS: usize = 32;

pub struct Request {
    pub id: String,
    pub op: Op,
}

/// Codes of the `error` member. The text for each is fixed and says nothing about vault content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    BadRequest,
    Denied,
    NoGrant,
    Expired,
    NotAllowed,
    Locked,
    Busy,
    AuditFailed,
    /// The request was allowed but could not be completed (network, TLS, size). Not written to the audit log.
    Upstream,
    Internal,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::BadRequest => "bad_request",
            Code::Denied => "denied",
            Code::NoGrant => "no_grant",
            Code::Expired => "expired",
            Code::NotAllowed => "not_allowed",
            Code::Locked => "locked",
            Code::Busy => "busy",
            Code::AuditFailed => "audit_failed",
            Code::Upstream => "upstream",
            Code::Internal => "internal",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Code::BadRequest => "the request is not valid",
            Code::Denied => "the request was not approved",
            Code::NoGrant => "there is no such grant",
            Code::Expired => "the grant has ended",
            Code::NotAllowed => "the grant does not cover this",
            Code::Locked => "the broker is locked",
            Code::Busy => "try again later",
            Code::AuditFailed => "the audit log could not be written",
            Code::Upstream => "the request could not be completed",
            Code::Internal => "internal error",
        }
    }
}

/// A request that could not be parsed. `id` is echoed when it could be read.
#[derive(Debug)]
pub struct Malformed {
    pub id: String,
}

fn text<'a>(o: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ()> {
    match o.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(()),
    }
}

fn only(o: &Map<String, Value>, allowed: &[&str]) -> Result<(), ()> {
    if o.keys().all(|k| allowed.contains(&k.as_str())) {
        Ok(())
    } else {
        Err(())
    }
}

fn lower_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    hex::decode(s).ok()?.try_into().ok()
}

fn tag_ok(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 40
        && t.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// A variable name the program will see.
pub fn env_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .next()
            .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

const METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// RFC 9110 `token`: what a header name may contain.
fn header_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Headers the broker (or the HTTP library) owns; a caller cannot set them.
const OWN_HEADERS: [&str; 10] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "te",
    "upgrade",
    "expect",
    "proxy-authorization",
    "proxy-connection",
];

fn header_value(s: &str) -> bool {
    s.len() <= 8192 && !s.chars().any(|c| c.is_control() && c != '\t')
}

fn http_request(o: &Map<String, Value>) -> Result<HttpRequest, ()> {
    only(
        o,
        &[
            "v", "id", "op", "token", "item", "method", "url", "headers", "body", "auth", "field",
        ],
    )?;
    let token = text(o, "token")?.ok_or(())?;
    if token.len() > 64 {
        return Err(());
    }
    let item = lower_hex(text(o, "item")?.ok_or(())?).ok_or(())?;
    let method = text(o, "method")?.ok_or(())?;
    if !METHODS.contains(&method) {
        return Err(());
    }
    let url = text(o, "url")?.ok_or(())?;
    if url.is_empty() || url.len() > MAX_URL || url.chars().any(char::is_control) {
        return Err(());
    }
    let (mut auth_header, mut auth_scheme) = ("Authorization".to_owned(), "Bearer".to_owned());
    if let Some(a) = o.get("auth") {
        let a = a.as_object().ok_or(())?;
        only(a, &["header", "scheme"])?;
        if let Some(h) = text(a, "header")? {
            auth_header = h.to_owned();
        }
        if let Some(sc) = text(a, "scheme")? {
            auth_scheme = sc.to_owned();
        }
        if !header_token(&auth_header)
            || OWN_HEADERS.contains(&auth_header.to_ascii_lowercase().as_str())
            || auth_scheme.len() > 32
            || !auth_scheme.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(());
        }
    }
    let mut headers = Vec::new();
    if let Some(h) = o.get("headers") {
        let h = h.as_object().ok_or(())?;
        if h.len() > MAX_HEADERS {
            return Err(());
        }
        for (name, value) in h {
            let value = value.as_str().ok_or(())?;
            let lower = name.to_ascii_lowercase();
            if !header_token(name)
                || !header_value(value)
                || OWN_HEADERS.contains(&lower.as_str())
                || lower == auth_header.to_ascii_lowercase()
            {
                return Err(());
            }
            headers.push((name.clone(), value.to_owned()));
        }
    }
    let body = text(o, "body")?.map(str::to_owned);
    if body.as_ref().is_some_and(|b| b.len() > MAX_BODY) {
        return Err(());
    }
    let field = text(o, "field")?.unwrap_or("token");
    if !field_name(field) {
        return Err(());
    }
    Ok(HttpRequest {
        token: token.to_owned(),
        item,
        method: method.to_owned(),
        url: url.to_owned(),
        headers,
        body,
        auth_header,
        auth_scheme,
        field: field.to_owned(),
    })
}

/// An item field name (`items.rs` field ids: lower case words, `/`, upper case for `env/NAME`).
fn field_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'/' | b'.' | b'-'))
}

fn scope(v: &Value) -> Result<Scope, ()> {
    let o = v.as_object().ok_or(())?;
    if o.len() != 1 {
        return Err(());
    }
    match o.iter().next() {
        Some((k, Value::String(s))) if k == "item" => Ok(Scope::Item(lower_hex(s).ok_or(())?)),
        Some((k, Value::String(s))) if k == "tag" && tag_ok(s) => Ok(Scope::Tag(s.clone())),
        Some((k, Value::Bool(true))) if k == "vault" => Ok(Scope::Vault),
        Some((k, Value::String(s)))
            if k == "name"
                && !s.is_empty()
                && s.len() <= 80
                && !s.chars().any(char::is_control) =>
        {
            Ok(Scope::Title(s.clone()))
        }
        _ => Err(()),
    }
}

fn parse_op(op: &str, o: &Map<String, Value>) -> Result<Op, ()> {
    let base = ["v", "id", "op"];
    let with = |extra: &[&str]| {
        let all: Vec<&str> = base.iter().chain(extra).copied().collect();
        only(o, &all)
    };
    match op {
        "hello" | "status" | "grant.list" | "lock" => {
            with(&[])?;
            Ok(match op {
                "hello" => Op::Hello,
                "status" => Op::Status,
                "grant.list" => Op::GrantList,
                _ => Op::Lock,
            })
        }
        "grant.revoke" => {
            with(&["grant"])?;
            let g = text(o, "grant")?.ok_or(())?;
            lower_hex::<8>(g).ok_or(())?;
            Ok(Op::GrantRevoke {
                grant: g.to_owned(),
            })
        }
        "http.request" => Ok(Op::HttpRequest(http_request(o)?)),
        "run.env" => {
            with(&["token", "item", "vars"])?;
            let token = text(o, "token")?.ok_or(())?;
            if token.len() > 64 {
                return Err(());
            }
            let item = lower_hex(text(o, "item")?.ok_or(())?).ok_or(())?;
            let mut vars = Vec::new();
            if let Some(v) = o.get("vars") {
                let m = v.as_object().ok_or(())?;
                if m.len() > MAX_VARS {
                    return Err(());
                }
                for (name, field) in m {
                    let field = field.as_str().ok_or(())?;
                    if !env_name(name) || !field_name(field) {
                        return Err(());
                    }
                    vars.push((name.clone(), field.to_owned()));
                }
            }
            Ok(Op::RunEnv {
                token: token.to_owned(),
                item,
                vars,
            })
        }
        "grant.request" => {
            with(&["scope", "mode", "ttl", "label", "reason"])?;
            Ok(Op::GrantRequest(GrantRequest {
                scope: scope(o.get("scope").ok_or(())?)?,
                mode: Mode::parse(text(o, "mode")?.ok_or(())?).ok_or(())?,
                ttl: Ttl::parse(text(o, "ttl")?.ok_or(())?).ok_or(())?,
                label: text(o, "label")?.map(str::to_owned),
                reason: text(o, "reason")?.map(str::to_owned),
            }))
        }
        _ => Err(()),
    }
}

/// Parse one line (without its newline). Unknown members, wrong types, a second value on the line and a
/// wrong version are all `Malformed`.
pub fn parse_request(line: &str) -> Result<Request, Malformed> {
    let fail = |id: String| Malformed { id };
    if line.len() > MAX_LINE {
        return Err(fail(String::new()));
    }
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(line) else {
        return Err(fail(String::new()));
    };
    let id = match o.get("id") {
        Some(Value::String(s)) if s.len() <= MAX_ID && !s.chars().any(char::is_control) => {
            s.clone()
        }
        _ => return Err(fail(String::new())),
    };
    if o.get("v") != Some(&json!(1)) {
        return Err(fail(id));
    }
    let Some(Value::String(op)) = o.get("op") else {
        return Err(fail(id));
    };
    match parse_op(op, &o) {
        Ok(op) => Ok(Request { id, op }),
        Err(()) => Err(fail(id)),
    }
}

pub fn ok(id: &str, members: Value) -> String {
    let mut o = match members {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    o.insert("v".into(), json!(1));
    o.insert("id".into(), json!(id));
    o.insert("ok".into(), json!(true));
    Value::Object(o).to_string()
}

pub fn err(id: &str, code: Code) -> String {
    json!({
        "v": 1, "id": id, "ok": false,
        "error": {"code": code.as_str(), "message": code.message()}
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: &str) -> Result<Request, Malformed> {
        parse_request(line)
    }

    #[test]
    fn accepts_every_op() {
        assert!(matches!(
            p(r#"{"v":1,"id":"a","op":"hello"}"#).unwrap().op,
            Op::Hello
        ));
        assert!(matches!(
            p(r#"{"v":1,"id":"a","op":"status"}"#).unwrap().op,
            Op::Status
        ));
        assert!(matches!(
            p(r#"{"v":1,"id":"a","op":"grant.list"}"#).unwrap().op,
            Op::GrantList
        ));
        assert!(matches!(
            p(r#"{"v":1,"id":"a","op":"lock"}"#).unwrap().op,
            Op::Lock
        ));
        let r = p(r#"{"v":1,"id":"a","op":"grant.revoke","grant":"0123456789abcdef"}"#).unwrap();
        assert!(matches!(r.op, Op::GrantRevoke { .. }));
        let item = "00112233445566778899aabbccddeeff";
        let line = format!(
            r#"{{"v":1,"id":"r1","op":"grant.request","scope":{{"item":"{item}"}},"mode":"http","ttl":"15m","label":"x","reason":"y"}}"#
        );
        let Op::GrantRequest(g) = p(&line).unwrap().op else {
            panic!()
        };
        assert_eq!(g.mode, Mode::Http);
        assert_eq!(g.ttl, Ttl::Min15);
        assert_eq!(g.scope.label(), format!("item:{item}"));
        for (scope, label) in [
            (r#"{"tag":"work"}"#, "tag:work"),
            (r#"{"vault":true}"#, "vault"),
        ] {
            let line = format!(
                r#"{{"v":1,"id":"r","op":"grant.request","scope":{scope},"mode":"run","ttl":"once"}}"#
            );
            let Op::GrantRequest(g) = p(&line).unwrap().op else {
                panic!()
            };
            assert_eq!(g.scope.label(), label);
        }
    }

    #[test]
    fn refuses_everything_else() {
        let item = "00112233445566778899aabbccddeeff";
        let bad = [
            "",
            "not json",
            "[]",
            r#"{"v":1,"id":"a","op":"hello"} {"v":1,"id":"b","op":"hello"}"#,
            r#"{"v":1,"id":"a","op":"hello"}x"#,
            r#"{"v":2,"id":"a","op":"hello"}"#,
            r#"{"id":"a","op":"hello"}"#,
            r#"{"v":1,"op":"hello"}"#,
            r#"{"v":1,"id":5,"op":"hello"}"#,
            r#"{"v":1,"id":"a"}"#,
            r#"{"v":1,"id":"a","op":"nope"}"#,
            r#"{"v":1,"id":"a","op":"hello","extra":1}"#,
            r#"{"v":1,"id":"a","op":"lock","grant":"x"}"#,
            r#"{"v":1,"id":"a","op":"grant.revoke"}"#,
            r#"{"v":1,"id":"a","op":"grant.revoke","grant":"XYZ"}"#,
            r#"{"v":1,"id":"a","op":"grant.revoke","grant":"0123456789ABCDEF"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","mode":"http","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true},"mode":"root","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true},"mode":"http","ttl":"forever"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":false},"mode":"http","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"tag":"Work"},"mode":"http","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"item":"ZZ"},"mode":"http","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"item":"00112233445566778899AABBCCDDEEFF"},"mode":"http","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true,"tag":"x"},"mode":"http","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true},"mode":"http","ttl":"once","label":5}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true},"mode":"http","ttl":"once","token":"x"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"name":""},"mode":"run","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"grant.request","scope":{"name":"a\nb"},"mode":"run","ttl":"once"}"#,
            r#"{"v":1,"id":"a","op":"run.env","item":"00112233445566778899aabbccddeeff"}"#,
            r#"{"v":1,"id":"a","op":"run.env","token":"t","item":"ZZ"}"#,
            r#"{"v":1,"id":"a","op":"run.env","token":"t","item":"00112233445566778899aabbccddeeff","vars":[]}"#,
            r#"{"v":1,"id":"a","op":"run.env","token":"t","item":"00112233445566778899aabbccddeeff","vars":{"lower":"token"}}"#,
            r#"{"v":1,"id":"a","op":"run.env","token":"t","item":"00112233445566778899aabbccddeeff","vars":{"A":"to ken"}}"#,
            r#"{"v":1,"id":"a","op":"run.env","token":"t","item":"00112233445566778899aabbccddeeff","vars":{"A":5}}"#,
            r#"{"v":1,"id":"a","op":"run.env","token":"t","item":"00112233445566778899aabbccddeeff","extra":1}"#,
        ];
        for line in bad {
            assert!(p(line).is_err(), "{line}");
        }
        let long_id = format!(r#"{{"v":1,"id":"{}","op":"hello"}}"#, "i".repeat(65));
        assert!(p(&long_id).is_err());
        let huge = format!(
            r#"{{"v":1,"id":"a","op":"hello","pad":"{}"}}"#,
            "x".repeat(MAX_LINE)
        );
        assert!(p(&huge).is_err());
        let _ = item;
    }

    #[test]
    fn run_env_and_a_title_scope_parse() {
        let item = "00112233445566778899aabbccddeeff";
        let line = format!(
            r#"{{"v":1,"id":"a","op":"run.env","token":"t","item":"{item}","vars":{{"GH_TOKEN":"token","B":"env/B"}}}}"#
        );
        let Op::RunEnv { token, vars, .. } = p(&line).unwrap().op else {
            panic!()
        };
        assert_eq!(token, "t");
        assert_eq!(vars.len(), 2);
        let line = format!(r#"{{"v":1,"id":"a","op":"run.env","token":"t","item":"{item}"}}"#);
        let Op::RunEnv { vars, .. } = p(&line).unwrap().op else {
            panic!()
        };
        assert!(vars.is_empty());
        let line = r#"{"v":1,"id":"a","op":"grant.request","scope":{"name":"GitHub"},"mode":"run","ttl":"15m"}"#;
        let Op::GrantRequest(g) = p(line).unwrap().op else {
            panic!()
        };
        assert_eq!(g.scope, Scope::Title("GitHub".into()));
    }

    #[test]
    fn http_request_parses_strictly() {
        let item = "00112233445566778899aabbccddeeff";
        let line = |extra: &str| {
            format!(
                r#"{{"v":1,"id":"a","op":"http.request","token":"t","item":"{item}","method":"POST","url":"https://api.example.com/x"{extra}}}"#
            )
        };
        let Op::HttpRequest(r) = p(&line(
            r#","headers":{"X-Trace":"1"},"body":"{}","auth":{"header":"X-Api-Key","scheme":""},"field":"token""#,
        ))
        .unwrap()
        .op
        else {
            panic!()
        };
        assert_eq!(
            (
                r.method.as_str(),
                r.auth_header.as_str(),
                r.auth_scheme.as_str()
            ),
            ("POST", "X-Api-Key", "")
        );
        assert_eq!(r.headers, [("X-Trace".to_owned(), "1".to_owned())]);
        let Op::HttpRequest(r) = p(&line("")).unwrap().op else {
            panic!()
        };
        assert_eq!(
            (
                r.auth_header.as_str(),
                r.auth_scheme.as_str(),
                r.field.as_str()
            ),
            ("Authorization", "Bearer", "token")
        );
        for bad in [
            r#","headers":{"Authorization":"x"}"#,
            r#","headers":{"authorization":"x"}"#,
            r#","headers":{"Host":"evil"}"#,
            r#","headers":{"Content-Length":"1"}"#,
            r#","headers":{"Bad Name":"x"}"#,
            r#","headers":{"X":"a\nb"}"#,
            r#","headers":["X"]"#,
            r#","auth":{"header":"X Y"}"#,
            r#","auth":{"header":"Host"}"#,
            r#","auth":{"header":"Transfer-Encoding"}"#,
            r#","auth":{"header":"Content-Length"}"#,
            r#","auth":{"header":"connection"}"#,
            r#","auth":{"header":"Expect"}"#,
            r#","auth":{"header":"Proxy-Authorization"}"#,
            r#","auth":{"scheme":"Bear er"}"#,
            r#","auth":{"header":"X","extra":1}"#,
            r#","field":"to ken""#,
            r#","body":5"#,
            r#","extra":1"#,
        ] {
            assert!(p(&line(bad)).is_err(), "{bad}");
        }
        let custom = line(r#","auth":{"header":"X-Key"},"headers":{"Authorization":"other"}"#);
        assert!(
            p(&custom).is_ok(),
            "a different header may be set when the secret uses another"
        );
        let big = line(&format!(r#","body":"{}""#, "x".repeat(MAX_BODY + 1)));
        assert!(p(&big).is_err());
        for url in ["", "http://a/\\u0001"] {
            let l = format!(
                r#"{{"v":1,"id":"a","op":"http.request","token":"t","item":"{item}","method":"GET","url":"{url}"}}"#
            );
            assert!(p(&l).is_err(), "{url}");
        }
        let get = format!(
            r#"{{"v":1,"id":"a","op":"http.request","token":"t","item":"{item}","method":"TRACE","url":"https://a/"}}"#
        );
        assert!(p(&get).is_err());
    }

    #[test]
    fn the_id_is_echoed_on_failure_when_it_can_be_read() {
        let e = p(r#"{"v":1,"id":"keep-me","op":"nope"}"#).err().unwrap();
        assert_eq!(e.id, "keep-me");
        assert_eq!(p("garbage").err().unwrap().id, "");
    }

    #[test]
    fn responses_have_fixed_shape_and_text() {
        let e = err("x", Code::NotAllowed);
        let v: Value = serde_json::from_str(&e).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["code"], "not_allowed");
        assert_eq!(v["id"], "x");
        let o = ok("y", json!({"locked": false}));
        let v: Value = serde_json::from_str(&o).unwrap();
        assert_eq!(
            (v["ok"].as_bool(), v["id"].as_str()),
            (Some(true), Some("y"))
        );
    }
}
