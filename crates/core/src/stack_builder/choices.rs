//! A stack given by the CLI's flag names, as `sawit new` and the MCP tools take it.

use serde::Deserialize;

use super::catalog::{NATIVE_FRONTEND, WEB_FRONTEND};
use super::{
    Addon, Api, Auth, Backend, Category, Database, DbSetup, Deploy, Example, NativeFrontend, Orm,
    Payments, Pm, Preset, Runtime, Stack, WebFrontend, options, rules,
};

/// A stack given by upstream's CLI flag names, any of them left out: what `sawit new` and the
/// MCP tools take. Unknown ids are refused with the ones there are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Choices {
    /// A preset (`mern`, `pern`, `t3`, `uniwind`) to start from instead of the default stack.
    pub template: Option<String>,
    /// The project name (or a path relative to where it is created).
    pub name: Option<String>,
    /// Web and native frontends, as `--frontend` takes them (one of each at most), or `none`.
    pub frontend: Option<Vec<String>>,
    /// A backend id; `self` is the fullstack backend of the web frontend.
    pub backend: Option<String>,
    pub runtime: Option<String>,
    pub api: Option<String>,
    pub database: Option<String>,
    pub orm: Option<String>,
    pub db_setup: Option<String>,
    pub web_deploy: Option<String>,
    pub server_deploy: Option<String>,
    pub auth: Option<String>,
    pub payments: Option<String>,
    pub addons: Option<Vec<String>>,
    pub examples: Option<Vec<String>>,
    pub git: Option<bool>,
    pub install: Option<bool>,
    pub package_manager: Option<String>,
}

/// `id` as one of `category`'s options, or why not.
fn pick<T>(category: Category, id: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T, String> {
    let id = id.trim().to_ascii_lowercase();
    parse(&id).ok_or_else(|| {
        let ids: Vec<&str> = options(category).iter().map(|o| o.id).collect();
        format!(
            "“{id}” isn't one of the {} options: use one of {}.",
            category.label(),
            ids.join(", ")
        )
    })
}

/// A list's ids, `none` meaning an empty list.
fn pick_list<T: PartialEq>(
    category: Category,
    ids: &[String],
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    for id in ids.iter().flat_map(|i| i.split([',', ' '])) {
        let id = id.trim();
        if id.is_empty() || id.eq_ignore_ascii_case("none") {
            continue;
        }
        let value = pick(category, id, &parse)?;
        if !out.contains(&value) {
            out.push(value);
        }
    }
    Ok(out)
}

impl Choices {
    /// The stack these choices give: the preset's (or the default) with each choice given in
    /// its place, not yet resolved.
    pub fn stack(&self) -> Result<Stack, String> {
        let mut stack = match self.template.as_deref().map(str::trim) {
            None | Some("") | Some("none") => Stack::default(),
            Some(id) => Preset::find(id)
                .ok_or_else(|| {
                    format!("“{id}” isn't a preset: use one of mern, pern, t3, uniwind.")
                })?
                .stack(),
        };
        if let Some(name) = &self.name {
            stack.project_name = name.clone();
        }
        if let Some(frontends) = &self.frontend {
            let (mut web, mut native) = (Vec::new(), Vec::new());
            for id in frontends.iter().flat_map(|i| i.split([',', ' '])) {
                let id = id.trim().to_ascii_lowercase();
                if id.is_empty() || id == "none" {
                    continue;
                }
                if let Some(w) = WebFrontend::parse(&id) {
                    web.push(w);
                } else if let Some(n) = NativeFrontend::parse(&id) {
                    native.push(n);
                } else {
                    let ids: Vec<&str> = WEB_FRONTEND
                        .iter()
                        .chain(NATIVE_FRONTEND)
                        .map(|o| o.id)
                        .filter(|id| *id != "none")
                        .collect();
                    return Err(format!(
                        "“{id}” isn't a frontend: use one of {} or none.",
                        ids.join(", ")
                    ));
                }
            }
            if web.len() > 1 || native.len() > 1 {
                return Err(
                    "Pick one web frontend and one native frontend at most, as create-better-t-stack takes them."
                        .into(),
                );
            }
            stack.web_frontend = web.first().copied().unwrap_or(WebFrontend::None);
            stack.native_frontend = native.first().copied().unwrap_or(NativeFrontend::None);
        }
        if let Some(id) = &self.backend {
            stack.backend = if id.trim().eq_ignore_ascii_case("self") {
                rules::self_backend(stack.web_frontend).ok_or_else(|| {
                    "Backend self uses a fullstack web frontend's own routes: pick next, tanstack-start, nuxt, svelte, solid or astro as the frontend."
                        .to_string()
                })?
            } else {
                pick(Category::Backend, id, Backend::parse)?
            };
        }
        macro_rules! single {
            ($field:ident, $category:expr, $parse:expr) => {
                if let Some(id) = &self.$field {
                    stack.$field = pick($category, id, $parse)?;
                }
            };
        }
        single!(runtime, Category::Runtime, Runtime::parse);
        single!(api, Category::Api, Api::parse);
        single!(database, Category::Database, Database::parse);
        single!(orm, Category::Orm, Orm::parse);
        single!(db_setup, Category::DbSetup, DbSetup::parse);
        single!(web_deploy, Category::WebDeploy, Deploy::parse);
        single!(server_deploy, Category::ServerDeploy, Deploy::parse);
        single!(auth, Category::Auth, Auth::parse);
        single!(payments, Category::Payments, Payments::parse);
        single!(package_manager, Category::PackageManager, Pm::parse);
        if let Some(ids) = &self.addons {
            stack.addons = pick_list(Category::Addons, ids, Addon::parse)?;
        }
        if let Some(ids) = &self.examples {
            stack.examples = pick_list(Category::Examples, ids, Example::parse)?;
        }
        if let Some(git) = self.git {
            stack.git = git;
        }
        if let Some(install) = self.install {
            stack.install = install;
        }
        Ok(stack)
    }
}
