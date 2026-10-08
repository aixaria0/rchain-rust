//! Netstring framing — the `tcp-testing-only` netlayer's message boundary.
//!
//! The OCapN prose says this netlayer "streams pure Syrup-encoded data directly, without
//! encryption or metadata beyond regular CapTP messages", but its reference implementation
//! (`ocapn-test-suite/utils/netstrings.py` and `netlayers/base.py`) wraps every message:
//! `CapTPSocket.send_message` does `Netstring(message.to_syrup())`, and the receiver reads one
//! netstring. So the boundary is a netstring, and a peer that streams bare Syrup would not be
//! understood. (The same prose-versus-implementation gap as AUDIT C216.)
//!
//! The format is `<ascii-decimal length>:<payload>` — **no trailing comma**, unlike the classic
//! netstring, because the reference's `to_netstring` is `str(len(self)).encode() + b":" + self` and
//! its reader stops after `length` bytes. The length is read up to the `:` and must be all digits.

use std::fmt;

/// Frame a message: `<len>:<payload>`.
pub fn encode(payload: &[u8]) -> Vec<u8> {
    let mut out = payload.len().to_string().into_bytes();
    out.push(b':');
    out.extend_from_slice(payload);
    out
}

/// Read one netstring from the front of `buf`.
///
/// `Ok(Some((payload, consumed)))` when a whole message is present; `Ok(None)` when more bytes are
/// needed (a prefix still being read, or a payload not yet complete) — which is exactly the signal
/// a stream reader needs. A malformed length is an error, never a silent skip.
pub fn decode_prefix(buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>, NetstringError> {
    let Some(colon) = buf.iter().position(|&b| b == b':') else {
        // No separator yet: fine only while every byte so far is a digit.
        return if buf.iter().all(u8::is_ascii_digit) {
            Ok(None)
        } else {
            Err(NetstringError::NotADigit)
        };
    };
    if colon == 0 || !buf[..colon].iter().all(u8::is_ascii_digit) {
        return Err(NetstringError::NotADigit);
    }
    let len: usize = std::str::from_utf8(&buf[..colon])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(NetstringError::LengthOverflow)?;
    let start = colon + 1;
    let end = start
        .checked_add(len)
        .ok_or(NetstringError::LengthOverflow)?;
    if end > buf.len() {
        return Ok(None);
    }
    Ok(Some((buf[start..end].to_vec(), end)))
}

/// A length prefix that is not a netstring length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetstringError {
    /// A byte before the `:` was not an ASCII digit.
    NotADigit,
    /// The length did not fit or would overrun the address space.
    LengthOverflow,
}

impl fmt::Display for NetstringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetstringError::NotADigit => write!(f, "netstring: length prefix is not all digits"),
            NetstringError::LengthOverflow => write!(f, "netstring: length prefix overflows"),
        }
    }
}

impl std::error::Error for NetstringError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netstring_kat() {
        assert_eq!(encode(b"hello"), b"5:hello".to_vec());
        assert_eq!(encode(b""), b"0:".to_vec());
        // The reference has no trailing comma; a peer's reader stops after `length` bytes.
        assert!(!encode(b"hello").contains(&b','));
    }

    #[test]
    fn netstring_round_trips_and_splits_a_stream() {
        let mut stream = encode(b"one");
        stream.extend_from_slice(&encode(b"two"));
        let (first, n) = decode_prefix(&stream).unwrap().unwrap();
        assert_eq!(first, b"one".to_vec());
        let (second, m) = decode_prefix(&stream[n..]).unwrap().unwrap();
        assert_eq!(second, b"two".to_vec());
        assert_eq!(n + m, stream.len());
    }

    #[test]
    fn netstring_asks_for_more_until_the_payload_arrives() {
        assert_eq!(decode_prefix(b"").unwrap(), None);
        assert_eq!(decode_prefix(b"1").unwrap(), None);
        assert_eq!(decode_prefix(b"12").unwrap(), None); // no colon yet
        assert_eq!(decode_prefix(b"5:hel").unwrap(), None); // short payload
        assert_eq!(
            decode_prefix(b"5:hello").unwrap(),
            Some((b"hello".to_vec(), 7))
        );
    }

    #[test]
    fn netstring_refuses_a_non_digit_prefix() {
        assert_eq!(decode_prefix(b"x:"), Err(NetstringError::NotADigit));
        assert_eq!(decode_prefix(b"1x"), Err(NetstringError::NotADigit));
        assert_eq!(decode_prefix(b":abc"), Err(NetstringError::NotADigit));
    }
}
