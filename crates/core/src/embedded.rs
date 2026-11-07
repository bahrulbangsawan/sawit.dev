//! The big JS bundles the binary carries deflated (`build.rs`): Pele (Mermaid diagrams) and
//! mesurer (the browser's Annotate). Each is inflated when it is run, into a string its user
//! drops afterwards, so it costs about a third of its size on disk and no memory while unused.

use std::io::Read as _;

/// Pele's bundle (`crates/core/assets/pele/pele.global.js`), deflated.
pub(crate) const PELE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pele.js.deflate"));
/// mesurer's bundle (`vendor/mesurer/bundle/mesurer.js`), deflated.
pub(crate) const MESURER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mesurer.js.deflate"));

/// `deflated` inflated into `into`, which it is appended to.
pub(crate) fn inflate_into(deflated: &[u8], into: &mut String) {
    flate2::read::DeflateDecoder::new(deflated)
        .read_to_string(into)
        .expect("a bundle the build deflated inflates to UTF-8");
}

/// `deflated` inflated.
pub(crate) fn inflate(deflated: &[u8]) -> String {
    let mut text = String::with_capacity(deflated.len() * 4);
    inflate_into(deflated, &mut text);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each bundle inflates to the file it was made from, byte for byte.
    #[test]
    fn the_bundles_inflate_to_their_files() {
        assert_eq!(inflate(PELE), include_str!("../assets/pele/pele.global.js"));
        assert_eq!(
            inflate(MESURER),
            include_str!("../../../vendor/mesurer/bundle/mesurer.js")
        );
        // On disk they take well under half their size.
        assert!(PELE.len() * 2 < include_str!("../assets/pele/pele.global.js").len());
        assert!(
            MESURER.len() * 2 < include_str!("../../../vendor/mesurer/bundle/mesurer.js").len()
        );
    }
}
