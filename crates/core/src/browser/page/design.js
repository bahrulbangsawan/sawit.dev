// Sawit's page design module: reads the page's styles, stylesheets and assets
// for the Design panel. Spliced into the page agent after the overlay module
// (see agent.js), so `resolve`, `clip`, `modules` and `helpers`
// (`helpers.selectorFor`) are in scope.
//
// Ops (their JSON is the contract in crates/core/src/site_design/page.rs, where
// the decisions on what the page reads are made: colours, ranking, limits):
// - `scan` → `PageScan`: colors, text styles, spacing, radii, shadows, sheets,
//   assets and the button looks.
// - `scan_samples` {button, secondary} → `PageSamples`: the component samples,
//   with the buttons Rust picked from the last scan.
// - `rules` {sheets: [{href, text}]} → `PageRules`: the same collectors over
//   sheets Sawit fetched because the page couldn't read them. They're kept for
//   `style`'s states.
// - `style` {ref | selector, html} → `PageStyle`.
//
// Nothing here changes the page: it reads computed styles and sheets only.
