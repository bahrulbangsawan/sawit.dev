  // ---- Comments (lane 2: mesurer-agents) ----
  //
  // Threads reach the Annotate panel through the workspaces mesurer saves; this posts what they
  // don't say: `comments.copied {text}` ("Copy for agent", ⌘K) and `comments.resolved {}`
  // ("Resolve all"). mesurer tells of both with a window event a page could forge, so the text is
  // what mesurer itself just copied through this world's clipboard (clipboard.js); an event with no
  // copy before it posts nothing.
  const COPY_FOR_AGENT_MS = 5000;
  let lastCopy = null;
  const noteCopy = (text) => {
    lastCopy = { text: String(text), at: performance.now() };
  };
  if (typeof Clipboard === "function" && Clipboard.prototype && typeof Clipboard.prototype.writeText === "function") {
    const writeText = Clipboard.prototype.writeText;
    Object.defineProperty(Clipboard.prototype, "writeText", {
      value: function (text) {
        noteCopy(text);
        return writeText.call(this, text);
      },
      configurable: true,
      writable: true,
    });
  } else if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    // Not a secure context: clipboard.js's own clipboard, made again around the copy.
    const own = navigator.clipboard;
    Object.defineProperty(navigator, "clipboard", {
      value: Object.freeze({
        writeText: (text) => {
          noteCopy(text);
          return own.writeText(text);
        },
        write: (items) => own.write(items),
      }),
      configurable: true,
    });
  }
  if (typeof addEventListener === "function") {
    addEventListener("mesurer:comments-copied", () => {
      const copy = lastCopy;
      lastCopy = null;
      if (!copy || performance.now() - copy.at > COPY_FOR_AGENT_MS) return;
      bridge("comments.copied", { text: copy.text }).catch((e) => log("warn", "comments.copied:", e));
    });
    addEventListener("mesurer:comments-resolved", () => {
      bridge("comments.resolved", {}).catch((e) => log("warn", "comments.resolved:", e));
    });
  }
