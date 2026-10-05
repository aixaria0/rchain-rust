//! OCapN locators — the out-of-band (URI) addressing of peers and the objects on them.
//!
//! `draft-specifications/Locators.md` defines two, both with a Syrup form (`crate::peer`, the
//! *in-band* descriptor) and a URI form (this module, the *out-of-band* descriptor):
//!
//! * **Peer locator** — `ocapn://<designator>.<transport>[?hint=value&…]`.
//! * **Sturdyref locator** — a peer locator plus the object's swiss number,
//!   `ocapn://<designator>.<transport>/s/<swiss-num>[?…]`.
//!
//! A designator "may itself contain dots" — the *trailing* dot is the designator/transport
//! separator, so the split is at the last `.` ([`PeerLocator::parse_uri`]). The swiss number is
//! opaque and is never parsed as a number; on the wire it is a byte array, so in the URI it is
//! percent-encoded rather than assumed to be UTF-8 text.
//!
//! **Two peers are the same iff their designator and transport agree; hints are ignored**
//! (`Locators.md`: "Equality only requires designator + transport to match"), which is
//! [`PeerLocator::same_peer`] — the derived `PartialEq` compares hints too and is only for tests.
//!
//! **Escaping.** The spec asks for RFC 3986 escaping but gives no concrete vector, so this module
//! decodes `%XX` and encodes everything outside the unreserved + sub-delims set (and outside the
//! query delimiters in hints). The wildcard transport is the only one in use, and it needs none of
//! this; a peer that escapes differently would be a spec question, not a bug to guess at here.

use std::collections::BTreeMap;
use std::fmt;

/// Hints whose values may not be written literally (query delimiters, and the `+`/`;`/`,` that
/// RFC 3986 query parsing has historically read as separators).
const QUERY_DENY: &[u8] = b"&=+;,";

/// Where a peer is, and how to reach it — `Locators.md`'s three fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerLocator {
    /// "Typically the key, but per the netlayer can be any value determined by the netlayer."
    pub designator: String,
    /// "A unique identifier to specify a netlayer" (a symbol; carries no `.`).
    pub transport: String,
    /// Extra netlayer connection data; absent when the netlayer needs none.
    pub hints: BTreeMap<String, String>,
}

/// A peer locator plus the swiss number of one object at that peer — "the pair is treated as a
/// capability sufficient to obtain a CapTP reference".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sturdyref {
    pub peer: PeerLocator,
    /// Opaque: "String which identifies the object". Never parsed as an integer.
    /// Opaque bytes identifying the object at that peer. The reference sends these as a Syrup
    /// **byte array** (`b"VMDDd1voKWarCe2GvgLbxbVFysNzRPzx"`), where the Locators prose says
    /// "string"; the implementation wins (AUDIT C216).
    pub swiss_num: Vec<u8>,
}

impl PeerLocator {
    /// Parse a peer locator URI. A sturdyref URI (one carrying `/s/…`) is refused: it names an
    /// object, not just a peer.
    pub fn parse_uri(uri: &str) -> Result<PeerLocator, LocatorError> {
        let (authority, swiss, query) = split_uri(uri)?;
        if swiss.is_some() {
            return Err(LocatorError::UnexpectedSturdyref);
        }
        let (designator, transport) = parse_authority(authority)?;
        Ok(PeerLocator {
            designator,
            transport,
            hints: parse_hints(query.unwrap_or(""))?,
        })
    }

    /// The URI form. Hints are emitted in sorted key order, so the output is canonical.
    pub fn to_uri(&self) -> String {
        let mut s = String::from("ocapn://");
        s.push_str(&percent_encode(&self.designator, &[]));
        s.push('.');
        s.push_str(&percent_encode(&self.transport, &[]));
        write_hints(&mut s, &self.hints);
        s
    }

    /// The spec's peer equality: designator and transport, hints ignored.
    pub fn same_peer(&self, other: &PeerLocator) -> bool {
        self.designator == other.designator && self.transport == other.transport
    }
}

impl Sturdyref {
    pub fn parse_uri(uri: &str) -> Result<Sturdyref, LocatorError> {
        let (authority, swiss, query) = split_uri(uri)?;
        let swiss = swiss.ok_or(LocatorError::MissingSwissNum)?;
        let (designator, transport) = parse_authority(authority)?;
        Ok(Sturdyref {
            peer: PeerLocator {
                designator,
                transport,
                hints: parse_hints(query.unwrap_or(""))?,
            },
            swiss_num: percent_decode_bytes(swiss)?,
        })
    }

    pub fn to_uri(&self) -> String {
        let mut s = String::from("ocapn://");
        s.push_str(&percent_encode(&self.peer.designator, &[]));
        s.push('.');
        s.push_str(&percent_encode(&self.peer.transport, &[]));
        s.push_str("/s/");
        s.push_str(&percent_encode_bytes(&self.swiss_num, &[]));
        write_hints(&mut s, &self.peer.hints);
        s
    }
}

/// A locator that will not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocatorError {
    /// The URI did not begin with the `ocapn` scheme.
    BadScheme,
    /// The authority had no `.` separating a transport (or the transport was empty).
    MissingTransport,
    /// A path that was neither absent nor the `/s/<swiss-num>` shape.
    BadPath,
    /// A peer locator was given a sturdyref URI.
    UnexpectedSturdyref,
    /// A sturdyref URI carried no `/s/<swiss-num>`.
    MissingSwissNum,
    /// A `%XX` escape was truncated or not hexadecimal.
    BadEscape,
    /// A query member was not `key=value`.
    BadHint,
}

impl fmt::Display for LocatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocatorError::BadScheme => write!(f, "locator: not an `ocapn://` URI"),
            LocatorError::MissingTransport => {
                write!(f, "locator: no `.transport` after the designator")
            }
            LocatorError::BadPath => {
                write!(f, "locator: path is neither empty nor `/s/<swiss-num>`")
            }
            LocatorError::UnexpectedSturdyref => {
                write!(f, "locator: this URI names an object, not a peer")
            }
            LocatorError::MissingSwissNum => {
                write!(f, "locator: sturdyref URI has no `/s/<swiss-num>`")
            }
            LocatorError::BadEscape => write!(f, "locator: malformed percent-escape"),
            LocatorError::BadHint => write!(f, "locator: query member is not `key=value`"),
        }
    }
}

impl std::error::Error for LocatorError {}

/// Split `ocapn://authority[/s/swiss][?query]` into its three parts.
///
/// Returns `(authority, swiss, query)`. For a peer locator the query sits directly after the
/// authority; for a sturdyref it sits after the swiss number.
fn split_uri(uri: &str) -> Result<(&str, Option<&str>, Option<&str>), LocatorError> {
    let rest = uri
        .strip_prefix("ocapn://")
        .ok_or(LocatorError::BadScheme)?;
    let (head, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if path.is_empty() {
        let (authority, query) = split_once_opt(head, '?');
        return Ok((authority, None, query));
    }
    // A query before `/s/` would be malformed: the query belongs after the swiss number.
    if head.contains('?') {
        return Err(LocatorError::BadPath);
    }
    let tail = path.strip_prefix("/s/").ok_or(LocatorError::BadPath)?;
    let (swiss, query) = split_once_opt(tail, '?');
    Ok((head, Some(swiss), query))
}

fn split_once_opt(s: &str, delim: char) -> (&str, Option<&str>) {
    match s.split_once(delim) {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    }
}

/// `designator.transport`, split at the *last* dot (a designator may contain dots).
fn parse_authority(authority: &str) -> Result<(String, String), LocatorError> {
    let (designator, transport) = authority
        .rsplit_once('.')
        .ok_or(LocatorError::MissingTransport)?;
    if transport.is_empty() {
        return Err(LocatorError::MissingTransport);
    }
    // The designator is a string and may be escaped; the transport is a symbol (`Locators.md`:
    // "symbol (cannot contain `.`)") and is taken literally.
    Ok((percent_decode(designator)?, transport.to_string()))
}

fn write_hints(s: &mut String, hints: &BTreeMap<String, String>) {
    if hints.is_empty() {
        return;
    }
    s.push('?');
    for (i, (k, v)) in hints.iter().enumerate() {
        if i > 0 {
            s.push('&');
        }
        s.push_str(&percent_encode(k, QUERY_DENY));
        s.push('=');
        s.push_str(&percent_encode(v, QUERY_DENY));
    }
}

fn parse_hints(query: &str) -> Result<BTreeMap<String, String>, LocatorError> {
    let mut m = BTreeMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').ok_or(LocatorError::BadHint)?;
        m.insert(percent_decode(k)?, percent_decode(v)?);
    }
    Ok(m)
}

fn percent_decode(s: &str) -> Result<String, LocatorError> {
    let bytes = percent_decode_bytes(s)?;
    String::from_utf8(bytes).map_err(|_| LocatorError::BadEscape)
}

/// Percent-decode to raw bytes — for the swiss number, which is opaque and need not be UTF-8.
fn percent_decode_bytes(s: &str) -> Result<Vec<u8>, LocatorError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(LocatorError::BadEscape);
            }
            let hi = hex_val(bytes[i + 1]).ok_or(LocatorError::BadEscape)?;
            let lo = hex_val(bytes[i + 2]).ok_or(LocatorError::BadEscape)?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-encode every byte outside the unreserved + sub-delims set, plus `:` and `@`, minus any
/// byte in `deny`.
fn percent_encode(s: &str, deny: &[u8]) -> String {
    percent_encode_bytes(s.as_bytes(), deny)
}

/// As [`percent_encode`], over raw bytes.
fn percent_encode_bytes(bytes: &[u8], deny: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if is_uri_safe(b) && !deny.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_uri_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_peer_uri_round_trip() {
        let uri =
            "ocapn://a2ef69ddd5f84840970612ff660f5058.tcp-testing-only?host=127.0.0.1&port=22045";
        let l = PeerLocator::parse_uri(uri).unwrap();
        assert_eq!(l.designator, "a2ef69ddd5f84840970612ff660f5058");
        assert_eq!(l.transport, "tcp-testing-only");
        assert_eq!(l.hints.get("host").map(String::as_str), Some("127.0.0.1"));
        assert_eq!(l.hints.get("port").map(String::as_str), Some("22045"));
        assert_eq!(l.to_uri(), uri);
    }

    #[test]
    fn locator_peer_without_hints_round_trip() {
        let uri = "ocapn://abc.tcp-testing-only";
        let l = PeerLocator::parse_uri(uri).unwrap();
        assert!(l.hints.is_empty());
        assert_eq!(l.to_uri(), uri);
    }

    #[test]
    fn locator_sturdyref_uri_round_trip() {
        let uri = "ocapn://a2ef69ddd5f84840970612ff660f5058.tcp-testing-only/s/JadQ0++RzsD4M+40uLxTWVaVqM10DcBJ?host=127.0.0.1&port=22045";
        let s = Sturdyref::parse_uri(uri).unwrap();
        assert_eq!(s.peer.designator, "a2ef69ddd5f84840970612ff660f5058");
        assert_eq!(s.peer.transport, "tcp-testing-only");
        assert_eq!(s.swiss_num, b"JadQ0++RzsD4M+40uLxTWVaVqM10DcBJ".to_vec());
        assert_eq!(s.peer.hints.get("port").map(String::as_str), Some("22045"));
        assert_eq!(s.to_uri(), uri);
    }

    #[test]
    fn locator_designator_may_contain_dots() {
        let l = PeerLocator::parse_uri("ocapn://a.b.tcp-testing-only").unwrap();
        assert_eq!(l.designator, "a.b");
        assert_eq!(l.transport, "tcp-testing-only");
        assert_eq!(l.to_uri(), "ocapn://a.b.tcp-testing-only");
    }

    #[test]
    fn locator_same_peer_ignores_hints() {
        let a = PeerLocator::parse_uri("ocapn://x.tcp?host=a").unwrap();
        let b = PeerLocator::parse_uri("ocapn://x.tcp?host=b").unwrap();
        assert!(a.same_peer(&b));
        assert_ne!(a, b); // derived equality does not ignore hints
    }

    #[test]
    fn locator_rejects_bad_scheme() {
        assert_eq!(
            PeerLocator::parse_uri("https://x.y"),
            Err(LocatorError::BadScheme)
        );
    }

    #[test]
    fn locator_rejects_missing_transport() {
        assert_eq!(
            PeerLocator::parse_uri("ocapn://abc"),
            Err(LocatorError::MissingTransport)
        );
        assert_eq!(
            PeerLocator::parse_uri("ocapn://abc."),
            Err(LocatorError::MissingTransport)
        );
    }

    #[test]
    fn locator_peer_refuses_a_sturdyref_uri() {
        assert_eq!(
            PeerLocator::parse_uri("ocapn://abc.tcp/s/s1"),
            Err(LocatorError::UnexpectedSturdyref)
        );
    }

    #[test]
    fn locator_sturdyref_requires_a_swiss_num() {
        assert_eq!(
            Sturdyref::parse_uri("ocapn://abc.tcp"),
            Err(LocatorError::MissingSwissNum)
        );
    }

    #[test]
    fn locator_rejects_a_bad_escape() {
        assert_eq!(
            PeerLocator::parse_uri("ocapn://ab%2.tcp"),
            Err(LocatorError::BadEscape)
        );
    }

    #[test]
    fn locator_percent_escapes_round_trip() {
        // A space is not URI-safe, so it must be escaped on the way out and restored on the way in.
        let l = PeerLocator {
            designator: "a b".into(),
            transport: "tcp-testing-only".into(),
            hints: BTreeMap::new(),
        };
        let uri = l.to_uri();
        assert_eq!(uri, "ocapn://a%20b.tcp-testing-only");
        assert_eq!(PeerLocator::parse_uri(&uri).unwrap(), l);
    }
}
