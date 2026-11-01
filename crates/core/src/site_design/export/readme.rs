//! README.md: what each file of the export holds.

use std::fmt::Write as _;

use super::plural;
use crate::site_design::tokens::Tokens;

const LICENSE: &str = "Assets and fonts belong to their owners; check licenses before shipping.";

fn describe(file: &str) -> Option<&'static str> {
    Some(match file {
        "DESIGN.md" => "the design system as Google Labs DESIGN.md (YAML tokens plus prose)",
        "tokens.json" => "design tokens in the DTCG format",
        "theme.css" => "a Tailwind v4 @theme block",
        "shadcn.css" => "shadcn/ui variables in oklch(), only those the page showed",
        "variables.css" => "the site's own custom properties, as found",
        "screenshot.png" => "the page as shown at export",
        "assets.json" => "every asset found, with where it was used and any error",
        "README.md" => "this file",
        _ => return None,
    })
}

pub fn readme(t: &Tokens, files: &[String]) -> String {
    let mut out = format!(
        "# {} design export\n\nSource: {}\nDate: {}\nExtracted by Sawit.\n\n## Files\n\n",
        t.site, t.url, t.date
    );
    let mut assets = 0;
    for f in files {
        if let Some(d) = describe(f) {
            let _ = writeln!(out, "- `{f}`: {d}");
        } else if f.starts_with("components/") {
            let _ = writeln!(
                out,
                "- `{f}`: one detected sample, outerHTML with computed styles inlined"
            );
        } else if f.starts_with("assets/") {
            assets += 1;
        }
    }
    if assets > 0 {
        let _ = writeln!(
            out,
            "- `assets/`: {} saved from the page (images, svg, icons, fonts, video, lottie)",
            plural(assets, "file", "files")
        );
    }
    let _ = write!(out, "\n{LICENSE}\n");
    out
}
