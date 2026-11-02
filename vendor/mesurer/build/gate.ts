// The page-world half of mesurer, as its Chrome extension runs it in the page's MAIN world
// (apps/extension/src/keyboard-gate.ts, imported as it is): mesurer's keyboard gate, which keeps
// keys typed into mesurer away from the page's own shortcuts and keeps the page from taking the
// focus back, isolated to mesurer's host (`mesurer-extension-host`); and the history patch that
// tells mesurer, in its own world, of the page's pushState/replaceState route changes
// (`mesurer:locationchange`). Both are idempotent: evaluating this twice installs nothing twice.
import "mesurer-ext/keyboard-gate";
