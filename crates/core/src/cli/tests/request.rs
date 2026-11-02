//! The open request, encoded for the app's socket and read back.

use super::*;

#[test]
fn request_round_trip() {
    let request = OpenRequest {
        targets: vec![
            at("/a b/c.rs", Some(4), Some(2)),
            at("/d", None, None),
            at("/e:f", Some(1), None),
        ],
        new_window: true,
        reuse: false,
        wait: true,
    };
    assert_eq!(decode(&encode(&request)), Some(request));
    assert_eq!(
        decode(&encode(&OpenRequest::default())),
        Some(OpenRequest::default())
    );
    assert_eq!(decode(b"something else"), None);
    assert_eq!(decode(b"sawit-open/1\0\0/a\0x\0"), None);
}
