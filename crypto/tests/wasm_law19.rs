//! Law 19 on a target with no host: the same known-answer witnesses `crypto/src/` runs, run on
//! `wasm32-unknown-unknown` under node — plus the two host seams the wasm build added (issue #98),
//! the JS RNG backend and the `web-time` clock.
//!
//! Every `#[test]` in `crypto/src/` pins Law 19 on the **host only**; this file is the missing half,
//! and it is what #98's "closes when" asks for after the compile checks. It is a separate crate, so
//! it does **not** inherit the lib's `#![forbid(unsafe_code)]` — which matters, because the
//! `wasm_bindgen_test` macro expansion is not written to satisfy that lint.
//!
//! Run: `cargo test -p rchain-crypto --target wasm32-unknown-unknown`.

#![cfg(target_arch = "wasm32")]

use rchain_crypto::encryption::curve25519::decrypt;
use rchain_crypto::hash::{blake2b256, sha256};
use rchain_crypto::signatures::secp256k1::Secp256k1;
use rchain_crypto::signatures::signatures_alg::SignaturesAlg;
use rchain_shared::base16;
use wasm_bindgen_test::wasm_bindgen_test;

/// Law 19's Blake2b256 witnesses (`crypto/src/hash/blake2b256.rs`), on the real target.
#[wasm_bindgen_test]
fn blake2b256_known_answers_hold_on_wasm() {
    assert_eq!(
        base16::encode(&blake2b256::hash(b"")),
        "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"
    );
    assert_eq!(
        base16::encode(&blake2b256::hash(b"abc")),
        "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319"
    );
    assert_eq!(
        blake2b256::hash_many(&[b"ab", b"c"]),
        blake2b256::hash(b"abc")
    );
}

/// Law 19's secp256k1 ECDSA witnesses (`crypto/src/signatures/secp256k1.rs:191,203`) — two of the
/// seven Rust witnesses the conformance gate runs — on the real target.
#[wasm_bindgen_test]
fn secp256k1_known_answers_hold_on_wasm() {
    let alg = Secp256k1;
    let data = sha256::hash(b"testing");

    let sig = base16::unsafe_decode(
        "3044022079BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F817980220294F14E883B3F525B5367756C2A11EF6CF84B730B36C17CB0C56F0AAB2C98589",
    );
    let pub_key = base16::unsafe_decode(
        "040A629506E1B65CD9D2E0BA9C75DF9C4FED0DB16DC9625ED14397F0AFC836FAE595DC53F8B0EFE61E703075BD9B143BAC75EC0E19F82A2208CAEB32BE53414C40",
    );
    assert!(alg.verify(&data, &sig, &pub_key));

    let sec =
        base16::unsafe_decode("67E56582298859DDAE725F972992A07C6C4FB9F62A8FFF58CE3CA926A1063530");
    let created = alg.sign(&data, &sec).expect("sign with valid secret key");
    assert_eq!(
        base16::encode(&created).to_uppercase(),
        "30440220182A108E1448DC8F1FB467D06A0F3BB8EA0533584CB954EF8DA112F1D60E39A202201C66F36DA211C087F3AF88B50EDF4F9BDAA6CF5FD6817E74DCA34DB12390C6E9"
    );
}

/// Law 19's Curve25519 witness (`crypto/src/encryption/curve25519.rs:112`) on the real target.
#[wasm_bindgen_test]
fn curve25519_known_answer_holds_on_wasm() {
    let bob_pub =
        base16::unsafe_decode("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f");
    let alice_sec =
        base16::unsafe_decode("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
    let nonce = base16::unsafe_decode("69696ee955b62b73cd62bda875fc73d68219e0036b7a0b37");
    let message = base16::unsafe_decode(
        "be075fc53c81f2d5cf141316ebeb0c7b5228c52a4c62cbd44b66849b64244ffce5ecbaaf33bd751a1ac728d45e6c61296cdc3c01233561f41db66cce314adb310e3be8250c46f06dceea3a7fa1348057e2f6556ad6b1318a024a838f21af1fde048977eb48f59ffd4924ca1c60902e52f0a089bc76897040e082f937763848645e0705",
    );
    let cipher = base16::unsafe_decode(
        "f3ffc7703f9400e52a7dfb4b3d3305d98e993b9f48681273c29650ba32fc76ce48332ea7164d96a4476fb8c531a1186ac0dfc17c98dce87b4da7f011ec48c97271d2c20f9b928fe2270d6fb863d51738b48eeee314a7cc8ab932164548e526ae90224368517acfeabd6bb3732bc0e9da99832b61ca01b6de56244a9e88d5f9b37973f622a43d14a6599b1f654cb45a74e355a5",
    );
    assert_eq!(
        decrypt(&bob_pub, &alice_sec, &nonce, &cipher).expect("box decrypt"),
        message
    );
}

/// **The JS RNG backend** — the code the `getrandom js`/`wasm_js` features exist for. Generating a key
/// pair draws from `rand::rng()` → `getrandom`. Before #187 this path did not compile for wasm at all;
/// this is the test that proves it now *runs* there.
#[wasm_bindgen_test]
fn the_js_rng_backend_generates_a_usable_key_on_wasm() {
    let alg = Secp256k1;
    let (sec, pk) = alg.new_key_pair();
    let data = sha256::hash(b"the rng backend, exercised on wasm");
    let sig = alg
        .sign(&data, sec.bytes())
        .expect("sign with a generated key");
    assert!(
        alg.verify(&data, &sig, pk.bytes()),
        "a signature from a key the JS backend generated must verify"
    );

    // Two draws differ — the backend is random, not a constant.
    let (_, pk2) = alg.new_key_pair();
    assert_ne!(pk.bytes(), pk2.bytes());
}

/// **The clock seam** (`shared/src/time.rs`) — `std::time` compiles for wasm but panics at runtime, so
/// these three go through `web-time` on the target. Before this unit, calling them on wasm panicked.
#[wasm_bindgen_test]
fn the_host_clock_seam_answers_on_wasm() {
    let millis = rchain_shared::time::current_millis();
    assert!(
        millis > 1_500_000_000_000,
        "post-2017 epoch millis expected, got {millis}"
    );
    let a = rchain_shared::time::nano_time();
    let b = rchain_shared::time::nano_time();
    assert!(b >= a, "nano_time must be monotonic");
}

/// Law 19's X25519 witnesses (`crypto/src/encryption/x25519.rs`) — the RFC 7748 §6.1
/// Diffie-Hellman example, on the real target. The scalar-multiplication ladder is exactly the kind of
/// code a target change can break silently, and the OCapN Noise transport's static key is derived by
/// it, so it is pinned on wasm as the other primitives are.
#[wasm_bindgen_test]
fn x25519_known_answers_hold_on_wasm() {
    use rchain_crypto::encryption::x25519::{public_from_secret, x25519};

    let decode = |s: &str| -> [u8; 32] {
        base16::decode(s)
            .expect("a base16 vector")
            .try_into()
            .expect("32 bytes")
    };
    let alice_secret = decode("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
    let bob_secret = decode("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");

    assert_eq!(
        base16::encode(&public_from_secret(&alice_secret)),
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    );
    assert_eq!(
        base16::encode(&x25519(&alice_secret, &public_from_secret(&bob_secret))),
        "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742"
    );
}
