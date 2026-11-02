//! mesurer (`vendor/mesurer/`, MIT, Copyright (c) 2026 Julien Thibeaut) in the browser tab's
//! Annotate: its bundle and keyboard gate, Sawit's glue around it, the bridge between them and the
//! app ([`protocol`]), and where its settings and workspaces are kept ([`store`]).
//!
//! While Annotate is open, the app evaluates, once per page load: [`MESURER_JS`] and [`GLUE_JS`]
//! in the [`WORLD`] content world, then `SawitMesurer.mount(…)` with [`mount_args`], then
//! [`MESURER_GATE_JS`] in the page's own world (as mesurer's extension runs its gate in the page's
//! MAIN world; after the mount, so mesurer's listeners come first on the window). The glue posts to the [`HANDLER`] message handler,
//! which exists in that world alone; see `page/mesurer/bridge.js` for every message kind.

pub mod files;
mod markdown;
mod protocol;
mod store;

#[cfg(test)]
mod tests;

use serde_json::{Value, json};

pub use protocol::{ClearWhat, Envelope, FromMesurer, Lane, parse, parse_envelope};
pub use store::{
    Summary, chrome_of, no_links, origin_key, page_artifacts, page_key, read_json, remove_file,
    settings_path, workspace_path, write_json, write_json_within,
};

/// The mesurer release the bundle is built from.
pub const MESURER_VERSION: &str = "0.2.7";
/// Its commit in <https://github.com/ibelick/mesurer>.
pub const MESURER_COMMIT: &str = "5c2fdaefde54591329d94b179af6ef5f32e15676";

/// React, React DOM and mesurer with its stylesheet, one IIFE (`vendor/mesurer/build/entry.tsx`):
/// it defines `__sawitMesurerBundle` in the world it runs in.
pub const MESURER_JS: &str = include_str!("../../../../../vendor/mesurer/bundle/mesurer.js");
/// mesurer's keyboard gate and history patch, for the page's own world
/// (`vendor/mesurer/build/gate.ts`).
pub const MESURER_GATE_JS: &str = include_str!("../../../../../vendor/mesurer/bundle/gate.js");

/// Sawit's glue, evaluated after [`MESURER_JS`] in [`WORLD`]: one block whose `const`s its
/// sections share (each a file in `page/mesurer/`, its opening comment in `page/mesurer.js`).
pub const GLUE_JS: &str = concat!(
    include_str!("../page/mesurer.js"),
    "{\n",
    include_str!("../page/mesurer/bridge.js"),
    include_str!("../page/mesurer/host.js"),
    include_str!("../page/mesurer/store.js"),
    include_str!("../page/mesurer/clipboard.js"),
    include_str!("../page/mesurer/files.js"),
    include_str!("../page/mesurer/capture.js"),
    include_str!("../page/mesurer/color.js"),
    include_str!("../page/mesurer/comments.js"),
    include_str!("../page/mesurer/keys.js"),
    include_str!("../page/mesurer/mount.js"),
    "}\n",
);

/// The content world mesurer and the glue run in (`WKContentWorld worldWithName:`).
pub const WORLD: &str = "sawit-mesurer";
/// The script message handler the glue posts to, registered in [`WORLD`] only.
pub const HANDLER: &str = "sawitMesurer";
/// mesurer's host element; its core looks it up by this id.
pub const HOST_ID: &str = "mesurer-extension-host";

/// What the world evaluates once per page: the bundle, then the glue. It ends in `undefined`, a
/// value WebKit can hand back.
pub fn world_script() -> String {
    format!("{MESURER_JS}\n;{GLUE_JS}\n;void 0;")
}

/// The argument of `SawitMesurer.mount`: mesurer's props from the app (its theme), and the
/// snapshot read from Sawit's files (`{settings, workspace}`).
pub fn mount_args(props: &Value, snapshot: &Value) -> String {
    json!({ "props": props, "snapshot": snapshot }).to_string()
}

/// `callAsyncJavaScript` bodies the app runs in [`WORLD`]; `json` is the argument, a JSON
/// string, and each answers a JSON string.
pub mod calls {
    /// Whether the world has the glue (the page's world is a new one after each load).
    pub const LOADED: &str = "return JSON.stringify(typeof SawitMesurer === \"object\");";
    pub const MOUNT: &str =
        "return JSON.stringify(await SawitMesurer.mount(JSON.parse(json)) ?? null);";
    pub const UNMOUNT: &str = "return JSON.stringify(typeof SawitMesurer === \"object\" ? SawitMesurer.unmount() : { mounted: false });";
    /// `json`: `"all"`, `"chrome"` or `"none"`.
    pub const HIDE: &str = "return JSON.stringify(SawitMesurer.hide(JSON.parse(json)));";
    /// `json`: `{snapshot, source}`.
    pub const APPLY: &str = "const a = JSON.parse(json); return JSON.stringify(SawitMesurer.apply(a.snapshot, a.source));";
    /// `json`: the props.
    pub const UPDATE: &str = "return JSON.stringify(SawitMesurer.update(JSON.parse(json)));";
    /// `json`: `{name, args}`.
    pub const CALL: &str = "const a = JSON.parse(json); return JSON.stringify(await SawitMesurer.call(a.name, a.args) ?? null);";
}
