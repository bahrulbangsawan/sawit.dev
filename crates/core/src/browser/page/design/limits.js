  const MAX_ELEMENTS = 6000;
  // Hidden subtrees don't count against the budget, but walking them does cost.
  const MAX_WALKED = 60000;
  const MAX_COLORS = 500;
  const MAX_TEXTS = 400;
  const MAX_ASSETS = 400;
  const MAX_VARIABLES = 500;
  const MAX_ANSWER = 2 * 1024 * 1024;
  const MAX_SVG = 100 * 1024;
  const MAX_DATA_URL = 100 * 1024;
  const MAX_HTML = 30 * 1024;
  const MAX_INLINED = 200;
  const STATES = ["hover", "focus", "focus-visible", "active"];

  // Sheets Sawit fetched and the page parsed (`rules`), kept for `style`.
  let fetchedSheets = [];
