//! The prompt library (`prompts.json`): search, read, write and fill
//! `{{variables}}`.

use std::collections::HashMap;
use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Store;
use sawit_core::prompts::{self, Library, Prompt};

use crate::{Sawit, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    #[schemars(description = "Words to match in title, body or tags; `#tag` filters by tag")]
    pub query: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct IdArgs {
    #[schemars(description = "Prompt id (see list_prompts)")]
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateArgs {
    pub title: String,
    #[schemars(description = "Prompt text; {{name}} marks a variable")]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateArgs {
    #[schemars(description = "Prompt id")]
    pub id: String,
    pub title: Option<String>,
    pub body: Option<String>,
    #[schemars(description = "Replaces the tags")]
    pub tags: Option<Vec<String>>,
    pub favorite: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FillArgs {
    #[schemars(description = "Prompt id")]
    pub id: String,
    #[schemars(description = "Variable values by name, e.g. {\"language\": \"Rust\"}")]
    #[serde(default)]
    pub values: HashMap<String, String>,
}

fn summary(p: &Prompt) -> String {
    let mut line = format!("- {} · id {}", p.title, p.id);
    if p.favorite {
        line.push_str(" · ★");
    }
    if !p.tags.is_empty() {
        line.push_str(&format!(
            " · {}",
            p.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")
        ));
    }
    let vars = prompts::variables(&p.body);
    if !vars.is_empty() {
        line.push_str(&format!(" · variables: {}", vars.join(", ")));
    }
    line
}

/// The filled prompt, and the variables left without a value.
pub(crate) fn fill(body: &str, values: &HashMap<String, String>) -> (String, Vec<String>) {
    let missing = prompts::variables(body)
        .into_iter()
        .filter(|v| !values.contains_key(v))
        .collect();
    (prompts::fill(body, values), missing)
}

impl Sawit {
    fn prompts_file(&self) -> PathBuf {
        self.paths.root.join("prompts.json")
    }

    fn library(&self) -> Result<Library, String> {
        Library::load(&self.prompts_file()).map_err(|e| {
            format!("{e}\nSawit won't save over prompts.json until it is fixed or moved.")
        })
    }

    async fn save_library(&self, lib: &Library) -> Result<(), String> {
        lib.save(&self.prompts_file()).map_err(|e| e.to_string())?;
        self.notify(Store::Prompts).await;
        Ok(())
    }

    fn prompt<'a>(lib: &'a Library, id: &str) -> Result<&'a Prompt, String> {
        lib.get(id)
            .ok_or_else(|| format!("No prompt with id {id}. See list_prompts."))
    }

    async fn list_prompts_tool(&self, query: Option<&str>) -> CallToolResult {
        let lib = tri!(self.library());
        let found = lib.search(query.unwrap_or_default());
        if found.is_empty() {
            return ok(if lib.items.is_empty() {
                "The prompt library is empty."
            } else {
                "No prompts match."
            });
        }
        ok(found.iter().map(|p| summary(p)).collect::<Vec<_>>().join("\n"))
    }

    async fn get_prompt_tool(&self, id: &str) -> CallToolResult {
        let lib = tri!(self.library());
        let p = tri!(Self::prompt(&lib, id));
        ok(format!("{}\n\n{}", summary(p).trim_start_matches("- "), p.body))
    }

    async fn create_prompt_tool(&self, a: CreateArgs) -> CallToolResult {
        if a.title.trim().is_empty() {
            return fail("Give the prompt a title.");
        }
        let mut lib = tri!(self.library());
        let id = lib.add(Prompt::new(&a.title, &a.body, &a.tags), prompts::now());
        tri!(self.save_library(&lib).await);
        ok(format!("Created prompt “{}” with id {id}.", a.title.trim()))
    }

    async fn update_prompt_tool(&self, a: UpdateArgs) -> CallToolResult {
        let mut lib = tri!(self.library());
        tri!(Self::prompt(&lib, &a.id));
        if a.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return fail("The title can't be empty.");
        }
        let changed = lib.update(&a.id, prompts::now(), |p| {
            if let Some(t) = a.title {
                p.title = t.trim().to_string();
            }
            if let Some(b) = a.body {
                p.body = b;
            }
            if let Some(tags) = a.tags {
                p.tags = prompts::normalize_tags(&tags);
            }
            if let Some(f) = a.favorite {
                p.favorite = f;
            }
        });
        if !changed {
            return ok("Nothing changed.");
        }
        tri!(self.save_library(&lib).await);
        let p = tri!(Self::prompt(&lib, &a.id));
        ok(format!("Updated.\n{}", summary(p)))
    }

    async fn delete_prompt_tool(&self, id: &str) -> CallToolResult {
        let mut lib = tri!(self.library());
        let title = tri!(Self::prompt(&lib, id)).title.clone();
        lib.remove(id);
        tri!(self.save_library(&lib).await);
        ok(format!("Deleted prompt “{title}”."))
    }

    async fn fill_prompt_tool(&self, a: FillArgs) -> CallToolResult {
        let mut lib = tri!(self.library());
        let body = tri!(Self::prompt(&lib, &a.id)).body.clone();
        let (text, missing) = fill(&body, &a.values);
        lib.mark_used(&a.id, prompts::now());
        let _ = self.save_library(&lib).await;
        if missing.is_empty() {
            ok(text)
        } else {
            ok(format!(
                "{text}\n\n(no value given for: {}; left as {{{{name}}}})",
                missing.join(", ")
            ))
        }
    }
}

#[tool_router(router = prompts_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List prompts in Sawit's prompt library (favourites and recent first), with ids, tags and variables; optional search query"
    )]
    async fn list_prompts(&self, Parameters(a): Parameters<ListArgs>) -> CallToolResult {
        self.list_prompts_tool(a.query.as_deref()).await
    }

    #[tool(description = "Get one prompt's full text, tags and {{variables}}")]
    async fn get_prompt(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        self.get_prompt_tool(&a.id).await
    }

    #[tool(description = "Add a prompt to the library ({{name}} in the body marks a variable)")]
    async fn create_prompt(&self, Parameters(a): Parameters<CreateArgs>) -> CallToolResult {
        self.create_prompt_tool(a).await
    }

    #[tool(description = "Change a prompt's title, body, tags or favourite flag")]
    async fn update_prompt(&self, Parameters(a): Parameters<UpdateArgs>) -> CallToolResult {
        self.update_prompt_tool(a).await
    }

    #[tool(description = "Delete a prompt from the library")]
    async fn delete_prompt(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        self.delete_prompt_tool(&a.id).await
    }

    #[tool(
        description = "Fill a prompt's {{variables}} with values and return the finished text (marks the prompt used)"
    )]
    async fn fill_prompt(&self, Parameters(a): Parameters<FillArgs>) -> CallToolResult {
        self.fill_prompt_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawit_core::instances::AppPaths;

    #[test]
    fn fill_reports_missing_variables() {
        let values = HashMap::from([("lang".to_string(), "Rust".to_string())]);
        let (text, missing) = fill("Write {{lang}} for {{ task }}", &values);
        assert_eq!(text, "Write Rust for {{ task }}");
        assert_eq!(missing, ["task"]);
    }

    #[tokio::test]
    async fn create_update_fill_delete() {
        let dir = tempfile::tempdir().unwrap();
        let sawit = Sawit::new(AppPaths {
            root: dir.path().to_path_buf(),
        });
        let text = |r: CallToolResult| format!("{:?}", r.content);
        let r = sawit
            .create_prompt_tool(CreateArgs {
                title: "Review".into(),
                body: "Review {{file}}".into(),
                tags: vec!["Code".into()],
            })
            .await;
        assert_ne!(r.is_error, Some(true));
        let lib = sawit.library().unwrap();
        let id = lib.items[0].id.clone();
        let listed = text(sawit.list_prompts_tool(Some("#code")).await);
        assert!(listed.contains("variables: file"), "{listed}");
        sawit
            .update_prompt_tool(UpdateArgs {
                id: id.clone(),
                title: None,
                body: Some("Check {{file}}".into()),
                tags: None,
                favorite: Some(true),
            })
            .await;
        let filled = text(
            sawit
                .fill_prompt_tool(FillArgs {
                    id: id.clone(),
                    values: HashMap::from([("file".into(), "main.rs".into())]),
                })
                .await,
        );
        assert!(filled.contains("Check main.rs"), "{filled}");
        assert_eq!(sawit.library().unwrap().items[0].used_count, 1);
        sawit.delete_prompt_tool(&id).await;
        assert!(sawit.library().unwrap().items.is_empty());
    }
}
