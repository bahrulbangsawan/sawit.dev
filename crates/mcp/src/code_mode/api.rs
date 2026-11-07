//! The typed Sawit API that code mode's scripts call: one function per tool, its signature made
//! from the same `#[tool]` definition (name, description, input schema) the server lists, so the
//! two can't drift apart. `search_api` finds functions by keyword and shows their signatures.

use rmcp::model::Tool;
use serde_json::{Map, Value};

/// The tools code mode adds, which a script doesn't call (it is already inside `run`).
pub const CODE_MODE_TOOLS: &[&str] = &["search_api", "run"];

/// One function of the API: a tool, as a script calls it (`sawit.<name>(args)`).
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub description: String,
    /// The tool's input schema (a JSON Schema object).
    pub schema: Map<String, Value>,
    /// MCP's hints, as words: "read-only", "destructive", "open world".
    pub hints: Vec<&'static str>,
}

impl Function {
    pub fn from_tool(tool: &Tool) -> Self {
        let mut hints = Vec::new();
        if let Some(a) = &tool.annotations {
            if a.read_only_hint == Some(true) {
                hints.push("read-only");
            }
            if a.destructive_hint == Some(true) {
                hints.push("destructive");
            }
            if a.open_world_hint == Some(true) {
                hints.push("open world");
            }
        }
        Self {
            name: tool.name.to_string(),
            description: tool.description.as_deref().unwrap_or_default().to_string(),
            schema: (*tool.input_schema).clone(),
            hints,
        }
    }

    /// The area the function belongs to: its name's first word (`git`, `canvas`, `browser`…),
    /// or the noun of a `list_x` / `create_x` style name.
    pub fn area(&self) -> &str {
        area_of(&self.name)
    }

    /// The function as a TypeScript declaration: its description, then each argument with its
    /// type, whether it is optional and what it means.
    pub fn signature(&self) -> String {
        let defs = self
            .schema
            .get("$defs")
            .or_else(|| self.schema.get("definitions"))
            .and_then(Value::as_object);
        let mut out = String::new();
        out.push_str(&doc_comment(&self.description, ""));
        if !self.hints.is_empty() {
            out.push_str(&format!("// {}\n", self.hints.join(", ")));
        }
        let props = self.schema.get("properties").and_then(Value::as_object);
        match props {
            Some(props) if !props.is_empty() => {
                let all_optional = required(&self.schema).is_empty();
                out.push_str(&format!(
                    "{}(args{}: {}): any",
                    self.name,
                    if all_optional { "?" } else { "" },
                    object_type(&self.schema, defs, "")
                ));
            }
            _ => out.push_str(&format!("{}(): any", self.name)),
        }
        out
    }

    /// The function on one line: its name, its arguments' names (`?` when optional) and the
    /// first sentence of its description.
    pub fn summary(&self) -> String {
        let req = required(&self.schema);
        let args: Vec<String> = self
            .schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|p| {
                p.keys()
                    .map(|k| {
                        if req.contains(&k.as_str()) {
                            k.clone()
                        } else {
                            format!("{k}?")
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "{}({{{}}}): {}",
            self.name,
            args.join(", "),
            first_sentence(&self.description)
        )
    }

    /// How well the function matches `words` (lowercase): 0 when it doesn't.
    fn score(&self, words: &[String]) -> u32 {
        let name = self.name.to_lowercase();
        let desc = self.description.to_lowercase();
        let args: Vec<String> = self
            .schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|p| p.keys().map(|k| k.to_lowercase()).collect())
            .unwrap_or_default();
        let mut total = 0;
        for word in words {
            let mut score = 0;
            if name == *word {
                score += 100;
            }
            if name.split('_').any(|part| part == word) {
                score += 30;
            } else if name.contains(word.as_str()) {
                score += 15;
            }
            if args.iter().any(|a| a == word) {
                score += 4;
            }
            let hits = desc.matches(word.as_str()).count() as u32;
            score += hits.min(3) * 3;
            if score == 0 {
                // Every word must match somewhere; a word that doesn't rules the function out.
                return 0;
            }
            total += score;
        }
        total
    }
}

/// The API: every tool the server has but code mode's own, by name.
#[derive(Debug, Clone)]
pub struct Api {
    pub functions: Vec<Function>,
}

impl Api {
    pub fn new(tools: &[Tool]) -> Self {
        let mut functions: Vec<Function> = tools
            .iter()
            .filter(|t| !CODE_MODE_TOOLS.contains(&t.name.as_ref()))
            .map(Function::from_tool)
            .collect();
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        Self { functions }
    }

    pub fn get(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// The functions that match `query` (every word of it, in the name, the arguments or the
    /// description), best first. A word that names an area (`git`, `canvas`) matches its
    /// functions by name.
    pub fn search(&self, query: &str) -> Vec<&Function> {
        let words: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .filter(|w| !STOP_WORDS.contains(&w.as_str()))
            .collect();
        if words.is_empty() {
            return self.functions.iter().collect();
        }
        // An exact name first, alone, then the rest by score.
        let mut scored: Vec<(u32, &Function)> = self
            .functions
            .iter()
            .map(|f| (f.score(&words), f))
            .filter(|(s, _)| *s > 0)
            .collect();
        if scored.is_empty() && words.len() > 1 {
            // No function has every word: rank by the words each has instead.
            scored = self
                .functions
                .iter()
                .map(|f| {
                    let s: u32 = words.iter().map(|w| f.score(std::slice::from_ref(w))).sum();
                    (s, f)
                })
                .filter(|(s, _)| *s > 0)
                .collect();
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
        scored.into_iter().map(|(_, f)| f).collect()
    }

    /// Every function's name, by area: what an empty search answers.
    pub fn overview(&self) -> String {
        let mut areas: Vec<(&str, Vec<&str>)> = Vec::new();
        for f in &self.functions {
            match areas.iter_mut().find(|(a, _)| *a == f.area()) {
                Some((_, names)) => names.push(&f.name),
                None => areas.push((f.area(), vec![&f.name])),
            }
        }
        areas.sort_by(|a, b| a.0.cmp(b.0));
        let mut out = format!(
            "{} functions. Search with words from what you want to do (\"git commit\", \"terminal read\") to get their typed signatures.\n",
            self.functions.len()
        );
        for (area, names) in areas {
            out.push_str(&format!("{area}: {}\n", names.join(", ")));
        }
        out
    }
}

/// Words a search ignores.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "for", "and", "or", "with", "how", "do", "i", "my",
    "is", "it", "from", "by", "sawit",
];

/// The area of a tool name: `git_status` → `git`, `list_prompts` → `prompts`.
pub fn area_of(name: &str) -> &str {
    const VERBS: &[&str] = &[
        "list",
        "create",
        "remove",
        "add",
        "update",
        "delete",
        "get",
        "install",
        "uninstall",
        "upgrade",
        "start",
        "stop",
        "restart",
        "describe",
        "drop",
        "export",
        "import",
        "save",
        "check",
        "detect",
        "fill",
        "render",
        "backup",
        "restore",
        "probe",
        "write",
        "truncate",
        "refresh",
        "edit",
        "use",
        "open",
        "sync",
        "diagnose",
        "search",
        "execute",
    ];
    let mut parts = name.split('_');
    let first = parts.next().unwrap_or(name);
    if VERBS.contains(&first) {
        let rest = &name[first.len()..].trim_start_matches('_');
        // `list_ssh_hosts` → `ssh`, `install_dev_tools` → `dev`: the noun's first word, one
        // area for `list_prompts` and `create_prompt`.
        let noun = rest
            .split('_')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(first);
        return singular(noun);
    }
    first
}

/// `prompts` → `prompt`, `processes` → `process`, `instances` → `instance`.
fn singular(noun: &str) -> &str {
    if let Some(stem) = noun.strip_suffix("es")
        && stem.ends_with("ss")
    {
        return stem;
    }
    match noun.strip_suffix('s') {
        Some(stem) if stem.len() > 2 && !stem.ends_with('s') => stem,
        _ => noun,
    }
}

fn required(schema: &Map<String, Value>) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// `text` as a `/** … */` comment at `indent`, wrapped at about 100 columns.
fn doc_comment(text: &str, indent: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let width = 100usize.saturating_sub(indent.len() + 3).max(40);
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().expect("a line");
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    // A `*/` in the text would end the comment early.
    let lines: Vec<String> = lines.into_iter().map(|l| l.replace("*/", "*\\/")).collect();
    if lines.len() == 1 {
        return format!("{indent}/** {} */\n", lines[0]);
    }
    let mut out = format!("{indent}/**\n");
    for line in lines {
        out.push_str(&format!("{indent} * {line}\n"));
    }
    out.push_str(&format!("{indent} */\n"));
    out
}

fn first_sentence(text: &str) -> String {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|&(i, c)| {
            (c == '.' || c == ':' || c == ';')
                && text[i + c.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let mut s: String = text[..end].to_string();
    if s.chars().count() > 140 {
        s = s.chars().take(137).collect::<String>() + "…";
    }
    s
}

/// An object schema as a TypeScript object type, each property on its own line with its
/// description as a comment.
fn object_type(
    schema: &Map<String, Value>,
    defs: Option<&Map<String, Value>>,
    indent: &str,
) -> String {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return "Record<string, any>".into();
    };
    if props.is_empty() {
        return "{}".into();
    }
    let req = required(schema);
    let inner = format!("{indent}  ");
    let mut out = String::from("{\n");
    for (name, prop) in props {
        let prop = prop.as_object();
        if let Some(desc) = prop
            .and_then(|p| p.get("description"))
            .and_then(Value::as_str)
        {
            out.push_str(&doc_comment(desc, &inner));
        }
        let ty = prop
            .map(|p| ts_type(p, defs, &inner, 0))
            .unwrap_or_else(|| "any".into());
        let optional = !req.contains(&name.as_str());
        let key = if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            name.clone()
        } else {
            format!("{name:?}")
        };
        out.push_str(&format!(
            "{inner}{key}{}: {ty};\n",
            if optional { "?" } else { "" }
        ));
    }
    out.push_str(indent);
    out.push('}');
    out
}

/// A JSON Schema as a TypeScript type. `null` drops out of unions: an optional argument is
/// already marked `?`.
fn ts_type(
    schema: &Map<String, Value>,
    defs: Option<&Map<String, Value>>,
    indent: &str,
    depth: u8,
) -> String {
    if depth > 6 {
        return "any".into();
    }
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let name = r.rsplit('/').next().unwrap_or(r);
        return match defs.and_then(|d| d.get(name)).and_then(Value::as_object) {
            Some(def) => ts_type(def, defs, indent, depth + 1),
            None => "any".into(),
        };
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let names: Vec<String> = values
            .iter()
            .filter(|v| !v.is_null())
            .map(|v| v.to_string())
            .collect();
        if !names.is_empty() {
            return names.join(" | ");
        }
    }
    if let Some(c) = schema.get("const") {
        return c.to_string();
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(alts) = schema.get(key).and_then(Value::as_array) {
            let mut types: Vec<String> = Vec::new();
            for alt in alts.iter().filter_map(Value::as_object) {
                let t = ts_type(alt, defs, indent, depth + 1);
                if t != "null" && !types.contains(&t) {
                    types.push(t);
                }
            }
            return if types.is_empty() {
                "null".into()
            } else {
                types.join(" | ")
            };
        }
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    let mut out: Vec<String> = Vec::new();
    for t in types.iter().filter(|t| **t != "null") {
        let ty = match *t {
            "string" => "string".to_string(),
            "integer" | "number" => "number".to_string(),
            "boolean" => "boolean".to_string(),
            "array" => {
                let item = schema
                    .get("items")
                    .and_then(Value::as_object)
                    .map(|i| ts_type(i, defs, indent, depth + 1))
                    .unwrap_or_else(|| "any".into());
                if item.contains(' ') {
                    format!("({item})[]")
                } else {
                    format!("{item}[]")
                }
            }
            "object" => {
                if schema.get("properties").is_some() {
                    object_type(schema, defs, indent)
                } else {
                    "Record<string, any>".into()
                }
            }
            _ => "any".to_string(),
        };
        if !out.contains(&ty) {
            out.push(ty);
        }
    }
    if out.is_empty() {
        if types.contains(&"null") {
            return "null".into();
        }
        if schema.get("properties").is_some() {
            return object_type(schema, defs, indent);
        }
        return "any".into();
    }
    out.join(" | ")
}
