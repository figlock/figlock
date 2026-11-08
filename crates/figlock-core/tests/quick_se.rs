//! The software half of Secure Enclave quick unlock (ADR-0025) through the public API, with a software P-256 key
//! standing in for `QK`. Keys are generated per run; nothing here is a real secret.

use figlock_core::{
    Key, QUICK_SE_POINT_LEN, open_quick_unlock_se, quick_unlock_se_ephemeral_public,
    seal_quick_unlock_se,
};
use p256::{PublicKey, SecretKey, ecdh::diffie_hellman, elliptic_curve::sec1::ToSec1Point};

const VAULT: [u8; 16] = [7; 16];
const DEVICE: [u8; 16] = [9; 16];

fn qk() -> (SecretKey, Vec<u8>) {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).unwrap();
    let k = SecretKey::from_slice(&b).unwrap();
    let public = k.public_key().to_sec1_point(false).as_bytes().to_vec();
    (k, public)
}

/// What the Secure Enclave does after the user is present.
fn agree(qk: &SecretKey, blob: &[u8]) -> Vec<u8> {
    let eph = quick_unlock_se_ephemeral_public(blob).unwrap();
    let peer = PublicKey::from_sec1_bytes(&eph).unwrap();
    diffie_hellman(qk.to_nonzero_scalar(), peer.as_affine())
        .raw_secret_bytes()
        .to_vec()
}

#[test]
fn a_wrapped_unlock_key_comes_back_only_with_the_right_key_and_ids() {
    let (secret, public) = qk();
    let auk = Key::random().unwrap();
    let blob = seal_quick_unlock_se(&public, &auk, &VAULT, &DEVICE).unwrap();
    assert_eq!(blob[0], 1);
    let shared = agree(&secret, &blob);
    let back = open_quick_unlock_se(&blob, &public, &shared, &VAULT, &DEVICE).unwrap();
    assert_eq!(back.expose(), auk.expose());

    // Another device, another vault, another Secure Enclave key, a wrong shared secret: all refused.
    assert!(open_quick_unlock_se(&blob, &public, &shared, &VAULT, &[1; 16]).is_err());
    assert!(open_quick_unlock_se(&blob, &public, &shared, &[1; 16], &DEVICE).is_err());
    let (other, other_public) = qk();
    let wrong = agree(&other, &blob);
    assert!(open_quick_unlock_se(&blob, &public, &wrong, &VAULT, &DEVICE).is_err());
    assert!(open_quick_unlock_se(&blob, &other_public, &shared, &VAULT, &DEVICE).is_err());
    assert!(open_quick_unlock_se(&blob, &public, &shared[..31], &VAULT, &DEVICE).is_err());
}

#[test]
fn every_wrap_uses_a_fresh_ephemeral_key_and_nonce() {
    let (_, public) = qk();
    let auk = Key::random().unwrap();
    let a = seal_quick_unlock_se(&public, &auk, &VAULT, &DEVICE).unwrap();
    let b = seal_quick_unlock_se(&public, &auk, &VAULT, &DEVICE).unwrap();
    assert_ne!(a, b);
    assert_ne!(
        quick_unlock_se_ephemeral_public(&a).unwrap(),
        quick_unlock_se_ephemeral_public(&b).unwrap()
    );
}

#[test]
fn damaged_blobs_never_reach_the_platform_or_open() {
    let (secret, public) = qk();
    let auk = Key::random().unwrap();
    let blob = seal_quick_unlock_se(&public, &auk, &VAULT, &DEVICE).unwrap();
    let shared = agree(&secret, &blob);
    // Every single-byte change after the version byte is caught: the point check, or the tag.
    for at in 0..blob.len() {
        let mut bad = blob.clone();
        bad[at] ^= 0x80;
        let outcome = quick_unlock_se_ephemeral_public(&bad)
            .and_then(|_| open_quick_unlock_se(&bad, &public, &shared, &VAULT, &DEVICE));
        assert!(outcome.is_err(), "byte {at} was accepted");
    }
    for len in [0, 1, QUICK_SE_POINT_LEN, 100] {
        assert!(
            quick_unlock_se_ephemeral_public(&blob[..len]).is_err(),
            "len {len}"
        );
    }
    // A blob cut short by one byte still has a valid ephemeral key, so only opening catches it.
    for len in [0, 1, QUICK_SE_POINT_LEN, 100, blob.len() - 1] {
        assert!(open_quick_unlock_se(&blob[..len], &public, &shared, &VAULT, &DEVICE).is_err());
    }
    // A public key that is not a point on the curve cannot be wrapped to.
    let mut off_curve = public.clone();
    off_curve[64] ^= 1;
    assert!(seal_quick_unlock_se(&off_curve, &auk, &VAULT, &DEVICE).is_err());
    assert!(seal_quick_unlock_se(&public[..64], &auk, &VAULT, &DEVICE).is_err());
}
