//! Diagrams with Pele (Sawit's built-in Mermaid renderer): Mermaid to SVG,
//! and database schemas as Mermaid, DBML or SVG.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::db::{self, diagram};
use sawit_core::mermaid::{self, Diagram, Palette};

use crate::{Sawit, blocking, expand_home, fail, ok, tilde, tri};

/// SVGs up to this size are returned inline.
const INLINE_SVG: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SchemaFormat {
    #[default]
    Mermaid,
    Dbml,
    Svg,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RenderArgs {
    #[schemars(description = "Mermaid source (flowchart, sequenceDiagram, erDiagram, …)")]
    pub source: String,
    #[schemars(
        description = "Where to write the .svg (absolute). Default: a new file in Sawit's cache folder"
    )]
    pub output_path: Option<String>,
    #[schemars(description = "light (default) or dark")]
    #[serde(default)]
    pub theme: Theme,
    #[schemars(description = "Also return the SVG text (it is returned anyway when small)")]
    #[serde(default)]
    pub inline: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SchemaDiagramArgs {
    #[schemars(description = "Sawit instance name or saved connection name")]
    pub target: String,
    #[schemars(description = "Only this schema (default: all user schemas)")]
    pub schema: Option<String>,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(description = "mermaid (erDiagram text, default), dbml (dbdiagram.io) or svg (drawn)")]
    #[serde(default)]
    pub format: SchemaFormat,
    #[schemars(
        description = "File to write (absolute). Required for nothing; svg defaults to Sawit's cache folder"
    )]
    pub output_path: Option<String>,
    #[schemars(description = "light (default) or dark; svg only")]
    #[serde(default)]
    pub theme: Theme,
}

/// Pele's colors: its light defaults, or the app's Zed One Dark theme
/// (as `crates/app/src/editor/diagrams.rs` derives them).
pub(crate) fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Light => Palette::default(),
        Theme::Dark => Palette {
            background: "#282c33".into(),
            foreground: "#dce0e5".into(),
            muted: "#a9afbc".into(),
            line: "#afb3b9".into(),
            surface: "#2e343e".into(),
            surface_alt: "#363c46".into(),
            border: "#757b85".into(),
            accent: "#74ade8".into(),
            critical: "#d07277".into(),
        },
    }
}

impl Sawit {
    /// `output_path`, or a fresh file in `<cache>/diagrams`.
    fn output_file(&self, output_path: Option<&str>, stem: &str, ext: &str) -> Result<PathBuf, String> {
        if let Some(p) = output_path.map(str::trim).filter(|p| !p.is_empty()) {
            let path = expand_home(p);
            if !path.is_absolute() {
                return Err(format!("output_path “{p}” must be absolute."));
            }
            return Ok(path);
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        Ok(self
            .paths
            .cache_dir()
            .join("diagrams")
            .join(format!("{stem}-{stamp}.{ext}")))
    }

    async fn write_file(path: PathBuf, text: String) -> Result<(), String> {
        let shown = path.display().to_string();
        blocking(move || {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, text)
        })
        .await?
        .map_err(|e| format!("Unable to write {shown}: {e}"))
    }

    async fn render(source: String, theme: Theme) -> Result<Diagram, String> {
        blocking(move || mermaid::render(&source, &palette(theme))).await?
    }

    pub async fn render_mermaid_tool(&self, a: RenderArgs) -> CallToolResult {
        if a.source.trim().is_empty() {
            return fail("The Mermaid source is empty.");
        }
        let path = tri!(self.output_file(a.output_path.as_deref(), "diagram", "svg"));
        let diagram = tri!(Self::render(a.source, a.theme).await);
        tri!(Self::write_file(path.clone(), diagram.svg.clone()).await);
        ok(describe_svg(&diagram, &path, a.inline))
    }

    pub async fn schema_diagram_tool(&self, a: SchemaDiagramArgs) -> CallToolResult {
        let client = tri!(self.connect_target(&a.target, a.database.as_deref()).await);
        let schemas = tri!(db::load_schema(&client).await.map_err(|e| e.to_string()));
        let only = a.schema.as_deref().filter(|s| !s.trim().is_empty());
        let tables = diagram::table_count(&schemas, only);
        if tables == 0 {
            return fail(match only {
                Some(s) => format!("Schema \"{s}\" has no tables to draw."),
                None => "The database has no tables to draw.".to_string(),
            });
        }
        let text = match a.format {
            SchemaFormat::Dbml => diagram::to_dbml(&schemas, only),
            SchemaFormat::Mermaid | SchemaFormat::Svg => diagram::to_mermaid(&schemas, only),
        };
        match a.format {
            SchemaFormat::Svg => {
                let path = tri!(self.output_file(a.output_path.as_deref(), "schema", "svg"));
                let diagram = tri!(Self::render(text, a.theme).await);
                tri!(Self::write_file(path.clone(), diagram.svg.clone()).await);
                ok(format!("{tables} tables. {}", describe_svg(&diagram, &path, false)))
            }
            SchemaFormat::Mermaid | SchemaFormat::Dbml => {
                let Some(out) = a.output_path.as_deref().filter(|p| !p.trim().is_empty()) else {
                    return ok(text);
                };
                let ext = if matches!(a.format, SchemaFormat::Dbml) { "dbml" } else { "mmd" };
                let path = tri!(self.output_file(Some(out), "schema", ext));
                tri!(Self::write_file(path.clone(), text.clone()).await);
                ok(format!("{tables} tables written to {}\n\n{text}", tilde(&path)))
            }
        }
    }
}

/// What was rendered and where; the SVG itself when small or asked for.
pub(crate) fn describe_svg(d: &Diagram, path: &std::path::Path, inline: bool) -> String {
    let mut out = format!(
        "Rendered a {} diagram ({:.0}×{:.0}) to {} ({} bytes).",
        d.kind,
        d.width,
        d.height,
        path.display(),
        d.svg.len()
    );
    if inline || d.svg.len() <= INLINE_SVG {
        out.push_str("\n\n");
        out.push_str(&d.svg);
    }
    out
}

#[tool_router(router = diagrams_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Render Mermaid source to a standalone SVG file with Sawit's offline renderer (Pele). Returns the file path, diagram kind and size; the SVG text too when small or inline=true"
    )]
    async fn render_mermaid(&self, Parameters(a): Parameters<RenderArgs>) -> CallToolResult {
        self.render_mermaid_tool(a).await
    }

    #[tool(
        description = "Draw a database's tables and foreign keys: as Mermaid erDiagram text, DBML (dbdiagram.io) text, or a rendered SVG file"
    )]
    async fn schema_diagram(
        &self,
        Parameters(a): Parameters<SchemaDiagramArgs>,
    ) -> CallToolResult {
        self.schema_diagram_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_palette_differs_from_light() {
        assert_eq!(palette(Theme::Light), Palette::default());
        assert_eq!(palette(Theme::Dark).background, "#282c33");
    }

    #[test]
    fn small_svgs_are_inline() {
        let d = Diagram {
            svg: "<svg/>".into(),
            width: 10.,
            height: 20.,
            kind: "flowchart".into(),
        };
        let text = describe_svg(&d, std::path::Path::new("/tmp/x.svg"), false);
        assert!(text.contains("flowchart diagram (10×20)"));
        assert!(text.ends_with("<svg/>"));
        let big = Diagram {
            svg: "x".repeat(INLINE_SVG + 1),
            ..d
        };
        assert!(!describe_svg(&big, std::path::Path::new("/x.svg"), false).contains("xxx"));
        assert!(describe_svg(&big, std::path::Path::new("/x.svg"), true).contains("xxx"));
    }

    #[test]
    fn renders_mermaid() {
        let d = mermaid::render("flowchart LR\n  a --> b", &palette(Theme::Dark)).unwrap();
        assert!(d.svg.contains("<svg"));
    }
}
