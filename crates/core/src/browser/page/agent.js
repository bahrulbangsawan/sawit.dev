// Sawit's browser agent, injected into every page of the browser tab before
// the page's own scripts (see crates/core/src/browser/page.rs).
//
// - Reports console messages, page errors, requests and URL changes to the
//   app over IPC, each message signed with the webview's token. The token is
//   held in this closure, and so are the functions that sign with it (`send`)
//   or pick (the overlay's listeners). They stay out of the page's reach only
//   while no page script can get one of them: messages go out through the
//   handler's own `postMessage` (see `post`), the picker's listeners only
//   through the listener methods taken at document start (see the overlay's
//   `listen`), and all of it is strict-mode code. JavaScriptCore answers
//   `Function.prototype.caller` for a sloppy function, arrows included, so a
//   page function the agent calls (a replaced `JSON.stringify`, a getter on an
//   event) would otherwise get `send` or `click` from its caller.
// - Answers the agent's ops through `window.__sawitAgent.run(op)`, which
//   returns a JSON string synchronously: `{"ok":true,…}` or
//   `{"ok":false,"error":…}`.
