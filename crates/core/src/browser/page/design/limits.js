  // How much of the page a design op reads: the walk's budget and the biggest reads it keeps
  // (an inline SVG, a data: address, an element's HTML). They come from Rust with every op
  // (site_design::page::Limits), which also ranks and caps what the scan found.
  let LIMITS = null;
  const useLimits = (op) => {
    const limits = op && op.limits;
    if (!limits || typeof limits !== "object") throw new Error("A design op comes with its limits.");
    LIMITS = limits;
  };
  const STATES = ["hover", "focus", "focus-visible", "active"];

  // Sheets Sawit fetched and the page parsed (`rules`), kept for `style`.
  let fetchedSheets = [];
