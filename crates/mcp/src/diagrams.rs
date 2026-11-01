//! Diagrams with Pele (Sawit's built-in renderer of Mermaid syntax): a diagram
//! of any type Pele draws to SVG (`render_diagram`, and `render_mermaid`, its
//! old name), and database schemas as Mermaid, DBML or SVG.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::db::{self, diagram};
use sawit_core::mermaid::{self, Diagram, Palette};

use crate::{Sawit, blocking, fail, ok, tilde, tri};

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
    #[schemars(
        description = "The diagram in Mermaid syntax, starting with its type: flowchart, sequenceDiagram, architecture-beta, C4Context, venn-beta, … (any type Pele draws)"
    )]
    pub source: String,
    #[schemars(
        description = "Where to write the .svg (absolute). Default: a new file in the project's .nyawit/diagrams folder (Sawit's cache folder outside a project)"
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
    #[schemars(
        description = "Sawit instance name, saved connection name, or a D1/SQLite id from detect_databases"
    )]
    pub target: String,
    #[schemars(description = "Only this schema (default: all user schemas)")]
    pub schema: Option<String>,
    #[schemars(description = "Database to connect to (default as in describe_schema)")]
    pub database: Option<String>,
    #[schemars(
        description = "mermaid (erDiagram text, default), dbml (dbdiagram.io) or svg (drawn)"
    )]
    #[serde(default)]
    pub format: SchemaFormat,
    #[schemars(
        description = "File to write (absolute). Required for nothing; svg defaults to the project's .nyawit/diagrams folder"
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
    /// The file `named` ([`crate::output_path`]), or a fresh one in the `.nyawit/diagrams/` of the
    /// project the agent works in (else `<cache>/diagrams`): the project that holds its folder, as
    /// the canvas tools take it ([`crate::agent_project`]). A diagram an agent rendered in a
    /// subfolder made a `.nyawit/` there, which then took the agent's canvas too.
    fn output_file(&self, named: Option<PathBuf>, stem: &str, ext: &str) -> PathBuf {
        if let Some(path) = named {
            return path;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let name = format!("{stem}-{stamp}.{ext}");
        if let Some(project) = crate::agent_project(None) {
            let dir = sawit_core::nyawit::diagrams_dir(&project);
            if sawit_core::nyawit::ensure(&project, &dir).is_ok() {
                return dir.join(name);
            }
        }
        self.paths.cache_dir().join("diagrams").join(name)
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

    /// `render_diagram`, or `render_mermaid` (its old name), whichever `tool` is: what it refuses
    /// says it in the name the agent called it by.
    pub async fn render_diagram_tool(&self, a: RenderArgs, tool: &str) -> CallToolResult {
        if a.source.trim().is_empty() {
            return fail("The diagram's source is empty.");
        }
        let named = tri!(crate::output_path(tool, a.output_path.as_deref()));
        let path = self.output_file(named, "diagram", "svg");
        let diagram = tri!(Self::render(a.source, a.theme).await);
        tri!(Self::write_file(path.clone(), diagram.svg.clone()).await);
        ok(describe_svg(&diagram, &path, a.inline))
    }

    pub async fn schema_diagram_tool(&self, a: SchemaDiagramArgs) -> CallToolResult {
        let named = tri!(crate::output_path(
            "schema_diagram",
            a.output_path.as_deref()
        ));
        let schemas = match tri!(self.lite_conn(&a.target, true)) {
            Some(conn) => tri!(conn.load_schema().await.map_err(|e| e.to_string())),
            None => {
                let client = tri!(self.connect_target(&a.target, a.database.as_deref()).await);
                tri!(db::load_schema(&client).await.map_err(|e| e.to_string()))
            }
        };
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
                let path = self.output_file(named, "schema", "svg");
                let diagram = tri!(Self::render(text, a.theme).await);
                tri!(Self::write_file(path.clone(), diagram.svg.clone()).await);
                ok(format!(
                    "{tables} tables. {}",
                    describe_svg(&diagram, &path, false)
                ))
            }
            SchemaFormat::Mermaid | SchemaFormat::Dbml => {
                let Some(path) = named else {
                    return ok(text);
                };
                tri!(Self::write_file(path.clone(), text.clone()).await);
                ok(format!(
                    "{tables} tables written to {}\n\n{text}",
                    tilde(&path)
                ))
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

/// What `render_diagram` says it does, and `render_mermaid` after the word that it is the old name.
macro_rules! render_diagram_says {
    () => {
        concat!(
            "Render a diagram written in Mermaid syntax to a standalone SVG file, offline, with Pele \
             (https://pele.run, Sawit's built-in renderer; mermaid.js isn't used). Every type Pele \
             draws: ",
            diagram_types!(),
            " (ZenUML and other Mermaid plugins aren't). Returns the file path, diagram type and \
             size; the SVG text too when small or inline=true. A source that can't be drawn says \
             why, with the line"
        )
    };
}

#[tool_router(router = diagrams_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(description = render_diagram_says!())]
    async fn render_diagram(&self, Parameters(a): Parameters<RenderArgs>) -> CallToolResult {
        self.render_diagram_tool(a, "render_diagram").await
    }

    #[tool(description = concat!(
        "Deprecated: the old name of render_diagram, which takes the same arguments; use \
         render_diagram. ",
        render_diagram_says!()
    ))]
    async fn render_mermaid(&self, Parameters(a): Parameters<RenderArgs>) -> CallToolResult {
        self.render_diagram_tool(a, "render_mermaid").await
    }

    #[tool(
        description = "Draw a database's tables and foreign keys: as Mermaid erDiagram text, DBML (dbdiagram.io) text, or a rendered SVG file"
    )]
    async fn schema_diagram(&self, Parameters(a): Parameters<SchemaDiagramArgs>) -> CallToolResult {
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

    /// Neither diagram tool writes over a canvas file: each says so before it draws or reads a
    /// database, and the board is as it was. An SVG path still gets the drawing.
    #[tokio::test]
    async fn no_diagram_is_written_over_a_canvas_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = Sawit::new(sawit_core::instances::AppPaths {
            root: dir.path().join("home"),
        });
        let board = dir.path().join(".nyawit").join("canvas.sawit");
        std::fs::create_dir_all(board.parent().unwrap()).unwrap();
        std::fs::write(&board, b"the board").unwrap();
        let said = |result: &CallToolResult| result.content[0].as_text().unwrap().text.clone();
        let refused = format!("would overwrite a canvas file: {}", board.display());
        let source = "flowchart LR\n  a --> b".to_string();

        let render = |output: &std::path::Path| RenderArgs {
            source: source.clone(),
            output_path: Some(output.display().to_string()),
            theme: Theme::Light,
            inline: false,
        };
        let drawn = server
            .render_diagram_tool(render(&board), "render_mermaid")
            .await;
        assert_eq!(drawn.is_error, Some(true));
        assert_eq!(said(&drawn), format!("render_mermaid {refused}"));
        for format in [SchemaFormat::Svg, SchemaFormat::Mermaid, SchemaFormat::Dbml] {
            let schema = SchemaDiagramArgs {
                target: "nowhere".into(),
                schema: None,
                database: None,
                format,
                output_path: Some(board.display().to_string()),
                theme: Theme::Light,
            };
            let drawn = server.schema_diagram_tool(schema).await;
            assert_eq!(drawn.is_error, Some(true));
            assert_eq!(said(&drawn), format!("schema_diagram {refused}"));
        }
        assert_eq!(std::fs::read(&board).unwrap(), b"the board");

        let svg = dir.path().join(".nyawit").join("flow.svg");
        let drawn = server
            .render_diagram_tool(render(&svg), "render_diagram")
            .await;
        assert_ne!(drawn.is_error, Some(true), "{}", said(&drawn));
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
    }
}
