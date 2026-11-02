//! The page script the browser tab injects into every page: the page agent and the page modules
//! it takes. Each is a JS file per section under `page/` (its opening comment in
//! `page/<name>.js`, its sections in `page/<name>/`), put back together here, in order.

use std::sync::LazyLock;

/// The page agent; the app replaces [`TOKEN_PLACEHOLDER`] before injecting it. It is one
/// strict-mode function, which the modules are spliced into.
pub const AGENT_JS: &str = concat!(
    include_str!("page/agent.js"),
    "(() => {\n  \"use strict\";\n",
    include_str!("page/agent/ipc.js"),
    include_str!("page/agent/console.js"),
    include_str!("page/agent/network.js"),
    include_str!("page/agent/url.js"),
    include_str!("page/agent/elements.js"),
    include_str!("page/agent/pointer.js"),
    include_str!("page/agent/keyboard.js"),
    include_str!("page/agent/scripts.js"),
    include_str!("page/agent/ops.js"),
    "})();\n",
);
pub const TOKEN_PLACEHOLDER: &str = "__SAWIT_TOKEN__";
/// The picker, highlights and markers (a page module). It is one block, whose `const`s its
/// sections share.
pub const OVERLAY_JS: &str = concat!(
    include_str!("page/overlay.js"),
    "{\n",
    include_str!("page/overlay/natives.js"),
    include_str!("page/overlay/theme.js"),
    include_str!("page/overlay/host.js"),
    include_str!("page/overlay/selectors.js"),
    include_str!("page/overlay/details.js"),
    include_str!("page/overlay/placement.js"),
    include_str!("page/overlay/drawing.js"),
    include_str!("page/overlay/markers.js"),
    include_str!("page/overlay/popover.js"),
    include_str!("page/overlay/events.js"),
    include_str!("page/overlay/picking.js"),
    include_str!("page/overlay/agents.js"),
    include_str!("page/overlay/ops.js"),
    "}\n",
);
/// The design scan, stylesheet rules and element styles (a page module; uses the overlay's helpers).
/// It is one block, as the overlay is.
pub const DESIGN_JS: &str = concat!(
    include_str!("page/design.js"),
    "{\n",
    include_str!("page/design/limits.js"),
    include_str!("page/design/helpers.js"),
    include_str!("page/design/color_math.js"),
    include_str!("page/design/colors.js"),
    include_str!("page/design/boxes.js"),
    include_str!("page/design/fonts.js"),
    include_str!("page/design/stylesheets.js"),
    include_str!("page/design/html.js"),
    include_str!("page/design/scan.js"),
    include_str!("page/design/rules.js"),
    include_str!("page/design/style.js"),
    "}\n",
);
/// Where the agent takes its page modules.
pub const MODULES_PLACEHOLDER: &str = "/*__SAWIT_MODULES__*/";

/// The page script around its token, made the first time: the agent with its page modules
/// spliced in is the same for every page, and a page's script is these pieces joined by its
/// token, as `replace` would set it.
static AROUND_TOKEN: LazyLock<Vec<String>> = LazyLock::new(|| {
    let modules = [OVERLAY_JS, DESIGN_JS].join("\n");
    AGENT_JS
        .replace(MODULES_PLACEHOLDER, &modules)
        .split(TOKEN_PLACEHOLDER)
        .map(str::to_string)
        .collect()
});

/// The agent with its page modules spliced in and `token` set.
pub fn page_script(token: &str) -> String {
    AROUND_TOKEN.join(token)
}
