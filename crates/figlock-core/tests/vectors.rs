//! Runs every vector in `docs/spec/vectors/` against figlock-core (FIG-24).
//!
//! Positive cases must reproduce the expected bytes exactly; `"expected": "reject"` cases must fail with the
//! single generic error. Files for later tasks (HLC, merge, reader state machine, SLIP-0039) are only checked
//! for presence here.

use figlock_core as fc;
use figlock_core::vector_hooks as vh;
use serde_json::Value;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/vectors")
}

fn load(name: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["format"], "figlock-vectors-1", "{name}");
    doc["cases"].as_array().unwrap().clone()
}

fn bytes(v: &Value) -> Vec<u8> {
    hex::decode(
        v.as_str()
            .unwrap_or_else(|| panic!("not a hex string: {v}")),
    )
    .unwrap()
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    bytes(v)
        .try_into()
        .unwrap_or_else(|_| panic!("expected {N} bytes"))
}

fn key(v: &Value) -> fc::Key {
    fc::Key::from_bytes(arr::<32>(v))
}

fn is_reject(c: &Value) -> bool {
    c["expected"] == "reject"
}

fn name(c: &Value) -> &str {
    c["name"].as_str().unwrap()
}

fn params(i: &Value) -> fc::KdfParams {
    fc::KdfParams {
        m_kib: i["m_kib"].as_u64().unwrap() as u32,
        t: i["t"].as_u64().unwrap() as u32,
        p: i["p"].as_u64().unwrap() as u32,
    }
}

fn u32f(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}

/// Assert `result` is the generic error (negative case) without needing `Debug` on secret types.
fn assert_rejected<T>(result: fc::Result<T>, what: &str) {
    assert!(result.is_err(), "{what}: expected reject");
}

#[test]
fn secret_key() {
    for c in load("secret-key.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        if is_reject(&c) {
            assert_rejected(fc::SecretKey::parse(i["text"].as_str().unwrap()), name(&c));
        } else if let Some(text) = e.get("text") {
            let sk = fc::SecretKey::from_bytes(arr(&i["sk"]));
            assert_eq!(sk.format().as_str(), text.as_str().unwrap(), "{}", name(&c));
        } else {
            let sk = fc::SecretKey::parse(i["text"].as_str().unwrap()).unwrap();
            assert_eq!(sk.expose().to_vec(), bytes(&e["sk"]), "{}", name(&c));
        }
    }
}

#[test]
fn kdf() {
    for c in load("kdf.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        if is_reject(&c) {
            assert_rejected(params(i).check(), name(&c));
        } else if let Some(pws) = i.get("passwords_equal_after_nfkd") {
            for pw in pws.as_array().unwrap() {
                let k = fc::derive_pw_key(pw.as_str().unwrap(), &bytes(&i["salt"]), &params(i))
                    .unwrap();
                assert_eq!(k.expose().to_vec(), bytes(&e["pw_key"]), "{}", name(&c));
            }
        } else {
            let k = fc::derive_pw_key(
                i["password"].as_str().unwrap(),
                &bytes(&i["salt"]),
                &params(i),
            )
            .unwrap();
            assert_eq!(k.expose().to_vec(), bytes(&e["pw_key"]), "{}", name(&c));
        }
    }
}

#[test]
fn key_hierarchy() {
    for c in load("key-hierarchy.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        if i.get("wrapped_vk").is_some() {
            let r = fc::unwrap_vk(
                &key(&i["auk"]),
                &bytes(&i["wrapped_vk"]),
                &arr(&i["vault_id"]),
                u32f(&i["key_epoch"]),
                &bytes(&i["kdf_params_bytes"]),
            );
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                assert_eq!(r.unwrap().expose().to_vec(), bytes(&e["vk"]), "{n}");
            }
        } else if i.get("nonce").is_some() {
            let w = vh::wrap_vk_with_nonce(
                &key(&i["auk"]),
                &key(&i["vk"]),
                &arr(&i["vault_id"]),
                u32f(&i["key_epoch"]),
                &bytes(&i["kdf_params_bytes"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(w, bytes(&e["wrapped_vk"]), "{n}");
        } else if i.get("vk").is_some() {
            let d = fc::derive_vault_keys(&key(&i["vk"])).unwrap();
            assert_eq!(d.lk.expose().to_vec(), bytes(&e["lk"]), "{n}");
            assert_eq!(d.wk.expose().to_vec(), bytes(&e["wk"]), "{n}");
            assert_eq!(
                d.authority_seed.expose().to_vec(),
                bytes(&e["authority_seed"]),
                "{n}"
            );
            assert_eq!(
                d.authority_public().to_vec(),
                bytes(&e["authority_pub"]),
                "{n}"
            );
        } else {
            let sk = fc::SecretKey::from_bytes(arr(&i["sk"]));
            let auk = fc::derive_auk(
                i["password"].as_str().unwrap(),
                &sk,
                &arr(&i["vault_id"]),
                &arr(&i["salt"]),
                &params(i),
            )
            .unwrap();
            assert_eq!(auk.expose().to_vec(), bytes(&e["auk"]), "{n}");
        }
    }
}

#[test]
fn items() {
    for c in load("items.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        if i.get("wrapped_ik").is_some() {
            assert_rejected(
                fc::unwrap_item_key(
                    &key(&i["wk"]),
                    &bytes(&i["wrapped_ik"]),
                    &arr(&i["vault_id"]),
                    &arr(&i["item_id"]),
                    u32f(&i["key_epoch"]),
                ),
                n,
            );
        } else if i.get("wk").is_some() {
            let w = vh::wrap_item_key_with_nonce(
                &key(&i["wk"]),
                &key(&i["ik"]),
                &arr(&i["vault_id"]),
                &arr(&i["item_id"]),
                u32f(&i["key_epoch"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(w, bytes(&e["wrapped_ik"]), "{n}");
        } else if i.get("nonce").is_some() {
            let b = vh::seal_field_with_nonce(
                &key(&i["ik"]),
                &bytes(&i["value_cbor"]),
                &arr(&i["vault_id"]),
                &arr(&i["item_id"]),
                i["field_id"].as_str().unwrap(),
                &arr(&i["hlc"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(b, bytes(&e["field_blob"]), "{n}");
        } else {
            let r = fc::open_field(
                &key(&i["ik"]),
                &bytes(&i["field_blob"]),
                &arr(&i["vault_id"]),
                &arr(&i["item_id"]),
                i["field_id"].as_str().unwrap(),
                &arr(&i["hlc"]),
            );
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                assert_eq!(r.unwrap().to_vec(), bytes(&e["value_cbor"]), "{n}");
            }
        }
    }
}

fn kind(i: &Value) -> fc::Kind {
    if i["kind"] == "op" {
        fc::Kind::Op
    } else {
        fc::Kind::Snapshot
    }
}

#[test]
fn files() {
    for c in load("files.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        if i.get("nonce").is_some() {
            let (vault, device) = (arr::<16>(&i["vault_id"]), arr::<16>(&i["device_id"]));
            let id = fc::FileId {
                vault_id: &vault,
                device_id: &device,
                seq: i["seq"].as_u64().unwrap(),
            };
            let f = vh::build_file_with_nonce(
                kind(i),
                &key(&i["lk"]),
                &key(&i["device_sign_seed"]),
                &id,
                u32f(&i["key_epoch"]),
                &bytes(&i["payload_cbor"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(f, bytes(&e["file"]), "{n}");
        } else {
            let pubkey: Option<[u8; 32]> = if i["device_sign_pub"].is_null() {
                None
            } else {
                Some(arr(&i["device_sign_pub"]))
            };
            let (vault, device) = (arr::<16>(&i["vault_id"]), arr::<16>(&i["device_id"]));
            let id = fc::FileId {
                vault_id: &vault,
                device_id: &device,
                seq: i["seq"].as_u64().unwrap(),
            };
            let r = fc::read_file(
                &bytes(&i["file"]),
                &key(&i["lk"]),
                pubkey.as_ref(),
                &id,
                kind(i),
            );
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                assert_eq!(r.unwrap().to_vec(), bytes(&e["payload_cbor"]), "{n}");
            }
        }
    }
}

#[test]
fn signatures() {
    for c in load("signatures.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        let (obj, default_ctx) = match i.get("record") {
            Some(r) => (r, "device-record"),
            None => (&i["vault_json"], "vault-json"),
        };
        if i.get("authority_seed").is_some() {
            assert_eq!(
                String::from_utf8(vh::canonical_json(obj).unwrap()).unwrap(),
                i["jcs"].as_str().unwrap(),
                "{n}: jcs"
            );
            let sig = fc::sign_json(&key(&i["authority_seed"]), default_ctx, obj).unwrap();
            assert_eq!(sig.to_vec(), bytes(&e["signature"]), "{n}");
        } else {
            let ctx = i
                .get("context")
                .and_then(Value::as_str)
                .unwrap_or(default_ctx);
            let r = fc::verify_json(&arr(&i["authority_pub"]), ctx, obj, &arr(&i["signature"]));
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                r.unwrap();
            }
        }
    }
}

#[test]
fn rotation() {
    for c in load("rotation.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        if i.get("nonce").is_some() {
            let s = vh::epoch_key_with_nonce(
                &key(&i["new_vk"]),
                &key(&i["old_vk"]),
                &arr(&i["vault_id"]),
                u32f(&i["old_epoch"]),
                u32f(&i["new_epoch"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(s, bytes(&e["epoch_key"]), "{n}");
        } else if i.get("epoch_key").is_some() {
            let r = fc::open_epoch_key(
                &key(&i["new_vk"]),
                &bytes(&i["epoch_key"]),
                &arr(&i["vault_id"]),
                u32f(&i["old_epoch"]),
                u32f(&i["new_epoch"]),
            );
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                assert_eq!(r.unwrap().expose().to_vec(), bytes(&e["old_vk"]), "{n}");
            }
        } else {
            let (o, nw) = (
                fc::derive_vault_keys(&key(&i["old_vk"])).unwrap(),
                fc::derive_vault_keys(&key(&i["new_vk"])).unwrap(),
            );
            assert_eq!(o.lk.expose().to_vec(), bytes(&e["old_lk"]), "{n}");
            assert_eq!(nw.lk.expose().to_vec(), bytes(&e["new_lk"]), "{n}");
            assert_eq!(
                o.authority_public().to_vec(),
                bytes(&e["old_authority_pub"]),
                "{n}"
            );
            assert_eq!(
                nw.authority_public().to_vec(),
                bytes(&e["new_authority_pub"]),
                "{n}"
            );
        }
    }
}

#[test]
fn quick_unlock() {
    for c in load("quick-unlock.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        if i.get("nonce").is_some() {
            let b = vh::seal_quick_unlock_with_nonce(
                &key(&i["qk"]),
                &key(&i["auk"]),
                &arr(&i["vault_id"]),
                &arr(&i["device_id"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(b, bytes(&e["blob"]), "{n}");
        } else {
            let r = fc::open_quick_unlock(
                &key(&i["qk"]),
                &bytes(&i["blob"]),
                &arr(&i["vault_id"]),
                &arr(&i["device_id"]),
            );
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                assert_eq!(r.unwrap().expose().to_vec(), bytes(&e["auk"]), "{n}");
            }
        }
    }
}

#[test]
fn pairing() {
    for c in load("pairing.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        match n {
            "invite-code-format" => assert_eq!(
                fc::invite_code_format(&arr(&i["s"])).unwrap(),
                e["code"].as_str().unwrap(),
                "{n}"
            ),
            "invite-code-parse" => assert_eq!(
                fc::invite_code_parse(i["code"].as_str().unwrap())
                    .unwrap()
                    .to_vec(),
                bytes(&e["s"]),
                "{n}"
            ),
            _ if n.starts_with("invite-code-") => {
                assert_rejected(fc::invite_code_parse(i["code"].as_str().unwrap()), n)
            }
            "invite-keys" => {
                let s = arr::<13>(&i["s"]);
                let vault = arr::<16>(&i["vault_id"]);
                let from_n =
                    fc::invite_keys(&s, &vault, &key(&i["en_priv"]), &arr(&i["et_pub"])).unwrap();
                let from_t =
                    fc::invite_keys(&s, &vault, &key(&i["et_priv"]), &arr(&i["en_pub"])).unwrap();
                assert_eq!(from_n.invite_id.to_vec(), bytes(&e["invite_id"]), "{n}");
                assert_eq!(from_n.k_req.expose().to_vec(), bytes(&e["k_req"]), "{n}");
                assert_eq!(from_n.k_resp.expose().to_vec(), bytes(&e["k_resp"]), "{n}");
                assert_eq!(from_t.invite_id, from_n.invite_id, "{n}: both sides agree");
                assert_eq!(
                    from_t.k_req.expose(),
                    from_n.k_req.expose(),
                    "{n}: both sides agree"
                );
            }
            "invite-req" => {
                let a = fc::aad(
                    "invite-req",
                    &[&bytes(&i["vault_id"]), &bytes(&i["invite_id"])],
                );
                let env = vh::seal_with_nonce(
                    &key(&i["k_req"]),
                    &bytes(&i["plaintext_cbor"]),
                    &a,
                    &arr(&i["nonce"]),
                )
                .unwrap();
                let mut file = bytes(&i["en_pub"]);
                file.extend_from_slice(&env);
                assert_eq!(file, bytes(&e["req_file"]), "{n}");
            }
            "invite-resp" => {
                let a = fc::aad(
                    "invite-resp",
                    &[&bytes(&i["vault_id"]), &bytes(&i["invite_id"])],
                );
                let env = vh::seal_with_nonce(
                    &key(&i["k_resp"]),
                    &bytes(&i["plaintext_cbor"]),
                    &a,
                    &arr(&i["nonce"]),
                )
                .unwrap();
                assert_eq!(env, bytes(&e["resp_file"]), "{n}");
            }
            _ => {
                let a = fc::aad(
                    "invite-resp",
                    &[&bytes(&i["vault_id"]), &bytes(&i["invite_id"])],
                );
                assert_rejected(fc::open(&key(&i["k_resp"]), &bytes(&i["resp_file"]), &a), n);
            }
        }
    }
}

#[test]
fn seal_to() {
    for c in load("seal-to.json") {
        let (i, e) = (&c["inputs"], &c["expected"]);
        let n = name(&c);
        if i.get("eph_priv").is_some() {
            let pt = i.get("plaintext").or_else(|| i.get("new_vk")).unwrap();
            let a = fc::aad(
                "vk-wrap-rk",
                &[&bytes(&i["vault_id"]), &fc::u32be(u32f(&i["key_epoch"]))],
            );
            let blob = vh::seal_to_with(
                &arr(&i["rk_pub"]),
                &bytes(pt),
                &a,
                &key(&i["eph_priv"]),
                &arr(&i["nonce"]),
            )
            .unwrap();
            assert_eq!(blob, bytes(&e["wrapped_vk_rk"]), "{n}");
        } else {
            let a = fc::aad(
                "vk-wrap-rk",
                &[&bytes(&i["vault_id"]), &fc::u32be(u32f(&i["key_epoch"]))],
            );
            let r = fc::open_seal_to(&key(&i["rk_priv"]), &bytes(&i["blob"]), &a);
            if is_reject(&c) {
                assert_rejected(r, n);
            } else {
                assert_eq!(r.unwrap().to_vec(), bytes(&e["vk"]), "{n}");
            }
        }
    }
}

/// Published vectors for the primitives, so a wiring mistake cannot hide behind our own vectors.
#[test]
fn primitives_match_published_vectors() {
    // RFC 5869 A.3: zero-length salt and info.
    let okm = vh::hkdf_raw(&[0x0b; 22], None, b"", 42).unwrap();
    assert_eq!(
        hex::encode(&*okm),
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
    );
    // RFC 7748 6.1.
    let a = key(&serde_json::json!(
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"
    ));
    assert_eq!(
        hex::encode(fc::x25519_public(&a)),
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    );
    // draft-irtf-cfrg-xchacha-03 A.3.1.
    let k: Vec<u8> = (0x80u8..0xa0).collect();
    let nonce: [u8; 24] = core::array::from_fn(|i| 0x40 + i as u8);
    let ad = hex::decode("50515253c0c1c2c3c4c5c6c7").unwrap();
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let kk = fc::Key::from_slice(&k).unwrap();
    let env = vh::seal_with_nonce(&kk, pt, &ad, &nonce).unwrap();
    assert_eq!(hex::encode(&env[25..33]), "bd6d179d3e83d43b");
    assert_eq!(
        hex::encode(&env[env.len() - 16..]),
        "c0875924c1c7987947deafd8780acf49"
    );
    assert_eq!(fc::open(&kk, &env, &ad).unwrap().to_vec(), pt.to_vec());
}

#[test]
fn files_for_later_tasks_are_present() {
    for f in ["hlc.json", "merge.json", "sync-reader.json", "slip39.json"] {
        assert!(!load(f).is_empty(), "{f}");
    }
}

/// Random seal and open round trip with the public API (no fixed nonce), plus a few negative checks that
/// are not in the JSON because they depend on fresh randomness.
#[test]
fn random_roundtrip_and_nonce_freshness() {
    let k = fc::Key::random().unwrap();
    let a = fc::aad("test", &[b"x"]);
    let e1 = fc::seal(&k, b"hello", &a).unwrap();
    let e2 = fc::seal(&k, b"hello", &a).unwrap();
    assert_ne!(e1, e2, "fresh nonce every time");
    assert_eq!(fc::open(&k, &e1, &a).unwrap().to_vec(), b"hello");
    assert_rejected(fc::open(&k, &e1, &fc::aad("test", &[b"y"])), "wrong aad");
    assert_rejected(fc::open(&fc::Key::random().unwrap(), &e1, &a), "wrong key");
    assert_rejected(fc::open(&k, &e1[..10], &a), "short envelope");
}
