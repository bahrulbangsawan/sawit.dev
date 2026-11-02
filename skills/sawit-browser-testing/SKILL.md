---
name: sawit-browser-testing
description: Test and debug web apps in Sawit's browser instead of Chrome, Playwright or Puppeteer. Use it to check that a page or flow works after a change, reproduce a UI bug, fill and submit a form, read console errors and failed requests, try a phone or tablet size, take a screenshot, or act on the notes the user left on a page, through the sawit MCP server's browser tools (browser_navigate, browser_snapshot, browser_click, browser_console).
---

# Testing web apps in Sawit's browser

Sawit has a WebKit browser tab for each project that you drive and the user sees. Use it instead of launching Chrome, installing Playwright or Puppeteer, or writing a throwaway browser script. The browser tools need the app: when one says Sawit isn't running, call `launch_app` with the project folder as `path`.

## The loop

1. Start the dev server first if none runs (`sawit-terminal`; `list_ports` shows what listens).
2. `browser_navigate` with no `url` opens the project's running dev server; or pass one such as `localhost:5173`. Its answer counts console errors and failed requests.
3. `browser_snapshot` reads the page as an outline with refs (`e12`). Act by `ref`: `browser_click`, `browser_type` (`clear`, `submit`), `browser_select_option`, `browser_press_key`, `browser_hover`, `browser_scroll`. A CSS `selector` works too.
4. Refs reset after every snapshot and navigation: take a new snapshot after the page changes, before the next action.
5. Wait with `browser_wait_for` (`text`, `text_gone`, `selector`), not fixed sleeps.
6. Check the result: a new `browser_snapshot`, `browser_console` (`level`: error), `browser_network` (`failed_only`: true), and `browser_evaluate` for a value the outline doesn't show.
7. `browser_screenshot` when the user should see it; `output_path` keeps the PNG.

## More

- `browser_resize` with a `device` (iPhone 15, Pixel 8, iPad mini, Laptop, Desktop), or a `width` and `height`; responsive fills the tab again.
- `browser_annotations` reads the notes the user drew on the page: what to change, with each element's selector, source file and styles. Read them when the user says they left notes, and pass `clear` once you have dealt with them.
- `browser_inspect` gives one element's box, type, colors and contrast; `browser_design` the page's colors, fonts and tokens.
- What is typed into password fields reads as [redacted] in `browser_network`; don't look for it another way.
- Report what you checked and what failed, with the console and network lines.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says; `sawit browse 5173` opens a page in Sawit for them to look at. Then test with what the project already has (its own test runner) rather than installing a browser automation stack, unless the user asks for one.
