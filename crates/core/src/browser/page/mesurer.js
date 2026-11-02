// Sawit's glue around mesurer (vendor/mesurer/), evaluated after mesurer's bundle in the
// `sawit-mesurer` content world, never the page's (crates/core/src/browser/mesurer/mod.rs). One
// block whose `const`s its sections (page/mesurer/) share: the bridge and the lanes' registries,
// the host, persistence, this world's overrides (clipboard, files, capture, color), comments,
// keys, and `SawitMesurer` (mount.js).
//
// A content world keeps page scripts away from mesurer's closures, the prototypes the overrides
// change and the bridge (its handler exists in this world alone; the app takes main-frame
// messages only), and keeps React out of the page's world. mesurer draws in a shadow root.
