//! Base64, both ways.

use crate::browser::{base64_decode, base64_encode};

#[test]
fn base64_round_trips() {
    for bytes in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\xfe\x10"] {
        assert_eq!(base64_decode(&base64_encode(bytes)).unwrap(), bytes);
    }
    assert!(base64_decode("a$b").is_none());
    assert_eq!(
        base64_decode("iVBO\nRw==\n").unwrap(),
        [0x89, b'P', b'N', b'G']
    );
}

#[test]
fn base64() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"h"), "aA==");
    assert_eq!(base64_encode(b"hi"), "aGk=");
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(base64_encode(&[0x89, b'P', b'N', b'G']), "iVBORw==");
}
