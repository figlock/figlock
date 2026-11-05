//! The broker wire protocol (`docs/spec/broker.md` 3): one JSON object per line, parsed strictly.

use figlock_core::ItemId;
use serde_json::{Map, Value, json};

/// One request line is at most this many bytes, newline excluded.
pub const MAX_LINE: usize = 16 * 1024;
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
}

impl Scope {
    /// The form used in the audit log and in grant lists.
    pub fn label(&self) -> String {
        match self {
            Scope::Item(id) => format!("item:{}", hex::encode(id)),
            Scope::Tag(t) => format!("tag:{t}"),
            Scope::Vault => "vault".to_owned(),
        }
    }

    /// The form used in requests and responses.
    pub fn to_json(&self) -> Value {
        match self {
            Scope::Item(id) => json!({"item": hex::encode(id)}),
            Scope::Tag(t) => json!({"tag": t}),
            Scope::Vault => json!({"vault": true}),
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
    GrantRevoke { grant: String },
    Lock,
}

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

fn scope(v: &Value) -> Result<Scope, ()> {
    let o = v.as_object().ok_or(())?;
    if o.len() != 1 {
        return Err(());
    }
    match o.iter().next() {
        Some((k, Value::String(s))) if k == "item" => Ok(Scope::Item(lower_hex(s).ok_or(())?)),
        Some((k, Value::String(s))) if k == "tag" && tag_ok(s) => Ok(Scope::Tag(s.clone())),
        Some((k, Value::Bool(true))) if k == "vault" => Ok(Scope::Vault),
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
