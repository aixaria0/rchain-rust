//! **The Noise interop run**: Agoric's implementation of the handshake, against ours.
//!
//! `ocapn/src/noise.rs` is verified against itself by its own tests — two of our endpoints
//! handshaking with each other, which proves the state machine and the framing agree with each other
//! and says nothing about whether they agree with anyone else. This drives the *other* side of the
//! wire: the reference's own Rust core, fetched by `run.sh` at a pinned commit and inlined below,
//! handshaking with our `NoiseNetlayer`.
//!
//! **The reference is the RESPONDER here**, and our node is the initiator. **The reverse direction is
//! NOT run** — `run.sh` invokes this harness once — so `respond()`'s half, including the
//! intended-responder prefix check, is unverified against the reference's initiator: the two paths are
//! different code, and a run of one says nothing about the other.
//!
//! What this proves: the handshake completes across implementations, and transport messages decrypt
//! in both directions. What it **cannot** prove, and the transcript says so: the *record framing*.
//! The reference binds `encrypt`/`decrypt` over one record of at most 65535 bytes and leaves record
//! boundaries to a netlayer — and `@endo/ocapn-noise` ships no netlayer, so our length prefix and
//! chunking have no counterpart to be tested against. The framing is ours.

// **One line of the reference is removed by `run.sh` before this is inlined**: its `#![no_std]` crate
// attribute, which is meaningful only when that file *is* a crate root, and this harness needs `std`
// for its sockets. Nothing else in the reference is touched, and `run.sh` re-checks that the file it
// fetched is the one this harness expects.
include!("../ref/lib.rs");

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};

use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::noise::{NoiseIdentity, NoiseNetlayer};

/// The address of the reference's static buffer, which it hands us through its own callback.
static BUFFER_ADDR: AtomicUsize = AtomicUsize::new(0);

/// The one host symbol the reference needs. Its `buffer()` and every key-generation call invokes this
/// with the address of its static buffer, which is how a host is meant to find it.
#[unsafe(no_mangle)]
pub extern "C" fn buffer_callback(buffer: *const u8) {
    BUFFER_ADDR.store(buffer as usize, Ordering::SeqCst);
}

fn buffer_addr() -> usize {
    let addr = BUFFER_ADDR.load(Ordering::SeqCst);
    assert!(addr != 0, "the reference has not published its buffer yet");
    addr
}

/// Write into the reference's buffer at one of its own offsets.
fn put(offset: usize, bytes: &[u8]) {
    // SAFETY: the address is the reference's own static buffer, whose size (65535) and layout are
    // constants in the source above; every call site writes inside it. This is the harness the
    // reference is designed to be driven by (see `bindings.js`), so a raw pointer is the interface.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), (buffer_addr() + offset) as *mut u8, bytes.len())
    };
}

/// Read out of the reference's buffer.
fn get(offset: usize, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    // SAFETY: as `put` — a read inside the reference's own static buffer.
    unsafe {
        std::ptr::copy_nonoverlapping((buffer_addr() + offset) as *const u8, out.as_mut_ptr(), len)
    };
    out
}

/// One transport record, as this side frames it: a `u32` big-endian ciphertext length, then the
/// ciphertext.
fn read_frame(sock: &mut std::net::TcpStream) -> Vec<u8> {
    let mut header = [0u8; 4];
    sock.read_exact(&mut header).expect("read the frame length");
    let len = u32::from_be_bytes(header) as usize;
    let mut body = vec![0u8; len];
    sock.read_exact(&mut body).expect("read the frame body");
    body
}

fn write_frame(sock: &mut std::net::TcpStream, body: &[u8]) {
    sock.write_all(&(body.len() as u32).to_be_bytes())
        .expect("write the frame length");
    sock.write_all(body).expect("write the frame body");
    sock.flush().expect("flush");
}

fn main() {
    println!("=== OCapN noise interop: Agoric's core as responder, rchain as initiator ===");
    println!("reference: endojs/endo rust/ocapn_noise at the commit pinned by run.sh");

    // The reference publishes its buffer lazily; every entry point calls back with the address.
    generate_responder_keys();
    let responder_verifying = get(RESPONDER_VERIFYING_KEY_OFFSET, VERIFYING_KEY_LENGTH);
    println!(
        "reference responder key: {}",
        rchain_shared_base16(&responder_verifying)
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the harness listener");
    let addr = listener.local_addr().expect("listener address");
    println!("reference listening on {addr}");

    let client = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("a tokio runtime");
        runtime.block_on(async move {
            let identity = NoiseIdentity::generate().expect("a fresh identity");
            let netlayer = NoiseNetlayer::bind("127.0.0.1:0", identity)
                .await
                .expect("bind the dialing side");
            let locator = PeerLocator {
                designator: "agoric-noise".to_string(),
                transport: "noise".to_string(),
                hints: BTreeMap::from([
                    ("host".to_string(), "127.0.0.1".to_string()),
                    ("port".to_string(), addr.port().to_string()),
                    (
                        "verify".to_string(),
                        rchain_shared_base16(&responder_verifying),
                    ),
                ]),
            };
            let mut conn = netlayer
                .new_outgoing_connection(&locator)
                .await
                .expect("the reference should complete the handshake");
            conn.send(b"hello from rchain")
                .await
                .expect("send a transport message");
            let reply = conn
                .recv()
                .await
                .expect("read the reply")
                .expect("a reply, not a closed connection");
            reply
        })
    });

    let (mut sock, _) = listener.accept().expect("accept the dial");

    // --- the handshake, in the reference's own steps --------------------------------------------
    let mut prefixed = [0u8; 164];
    sock.read_exact(&mut prefixed).expect("read the prefixed SYN");
    put(INTENDED_RESPONDER_KEY_OFFSET, &prefixed[..32]);
    put(SYN_OFFSET, &prefixed[32..]);
    let rc = responder_read_syn();
    assert_eq!(rc, 0, "responder_read_syn refused the SYN with code {rc}");
    let rc = responder_write_synack();
    assert_eq!(rc, 0, "responder_write_synack failed with code {rc}");
    sock.write_all(&get(SYNACK_OFFSET, SYNACK_LENGTH))
        .expect("write the SYNACK");

    let mut ack = [0u8; 64];
    sock.read_exact(&mut ack).expect("read the ACK");
    put(ACK_OFFSET, &ack);
    let rc = responder_read_ack();
    assert_eq!(rc, 0, "responder_read_ack refused the ACK with code {rc}");
    println!("handshake: completed — the reference accepted our SYN and our ACK");

    // --- the transport ---------------------------------------------------------------------------
    let frame = read_frame(&mut sock);
    put(0, &frame);
    let rc = decrypt(frame.len());
    assert_eq!(rc, 0, "the reference could not decrypt our message");
    let plain = get(0, frame.len() - 16);
    println!(
        "reference decrypted: {:?}",
        String::from_utf8_lossy(&plain)
    );

    let reply = b"hello from agoric's noise core";
    put(0, reply);
    let _ = encrypt(reply.len());
    write_frame(&mut sock, &get(0, reply.len() + 16));

    let received = client.join().expect("the dialing thread");
    println!(
        "rchain decrypted:   {:?}",
        String::from_utf8_lossy(&received)
    );
    println!("=== interop: OK ===");
    println!(
        "NOT PROVEN by this run: the record framing. The reference frames one record of at most \
         65535 bytes and leaves boundaries to a netlayer; `@endo/ocapn-noise` ships none, so the \
         length prefix and chunking in `ocapn/src/noise.rs` have no counterpart here."
    );
}

/// The reference's own `base16`-free world: the harness prints keys the way the rest of this
/// repository does.
fn rchain_shared_base16(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
