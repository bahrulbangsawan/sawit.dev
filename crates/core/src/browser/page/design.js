// Sawit's page design module: reads the page's styles, stylesheets and assets
// for the Design panel. Spliced into the page agent after the overlay module
// (see agent.js), so `resolve`, `clip`, `modules` and `helpers`
// (`helpers.selectorFor`) are in scope.
//
// Ops (their JSON is the contract in crates/core/src/site_design):
// - `scan` → `Scan`: colors, text styles, spacing, radii, shadows, sheets,
//   assets and component samples.
// - `rules` {sheets: [{href, text}]} → `SheetRules`: the same collectors over
//   sheets Sawit fetched because the page couldn't read them. They're kept for
//   `style`'s states.
// - `style` {ref | selector, html} → `StyleInfo`.
//
// Nothing here changes the page: it reads computed styles and sheets only.
