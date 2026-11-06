//! Sawit's status line for Claude Code: a layout of lines of components (model, folder, git,
//! context, cost, 5h/7d usage, the account's own status line, text…) with a theme, drawn by the
//! collector for the accounts that use it. The layout file has Claude Pool's format, so a layout
//! made there imports as it is: `accounts/statusline/config.json` (the last 10 applied kept in
//! `history/` for Undo) and `profile-<id>.json` per account (on/off, and the account's name,
//! email and plan the collector shows).

mod files;
mod probe;
mod render;
mod theme;

pub use files::{
    AccountLine, account_path, apply, config_path, dir, load_account, load_config, presets,
    render_managed, save_account, undo,
};

pub use probe::{git_probe, parse_git, terminal_width};
pub use render::{GitStatus, Segment, Values, ansi, segments, text_for};
pub use theme::{THEMES, Theme, rgb, theme};

use serde::{Deserialize, Serialize};

/// One kind of component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Model,
    Account,
    Plan,
    Profile,
    Cwd,
    GitBranch,
    GitStatus,
    ContextPercent,
    ContextTokens,
    TokensInOut,
    CacheTokens,
    SessionCost,
    SessionDuration,
    LinesChanged,
    SessionName,
    FiveHourUsage,
    SevenDayUsage,
    FiveHourReset,
    SevenDayReset,
    Effort,
    VimMode,
    OutputStyle,
    Version,
    OriginalStatusLine,
    CustomText,
    Separator,
    FlexSpacer,
}

/// What a component is, for its colour in a theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Model,
    Identity,
    Location,
    Git,
    Context,
    Session,
    Usage,
    Text,
}

impl Kind {
    pub const ALL: &[Kind] = &[
        Kind::Model,
        Kind::Account,
        Kind::Plan,
        Kind::Profile,
        Kind::Cwd,
        Kind::GitBranch,
        Kind::GitStatus,
        Kind::ContextPercent,
        Kind::ContextTokens,
        Kind::TokensInOut,
        Kind::CacheTokens,
        Kind::SessionCost,
        Kind::SessionDuration,
        Kind::LinesChanged,
        Kind::SessionName,
        Kind::FiveHourUsage,
        Kind::SevenDayUsage,
        Kind::FiveHourReset,
        Kind::SevenDayReset,
        Kind::Effort,
        Kind::VimMode,
        Kind::OutputStyle,
        Kind::Version,
        Kind::OriginalStatusLine,
        Kind::CustomText,
        Kind::Separator,
        Kind::FlexSpacer,
    ];

    /// As the editor lists it.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Model => "Model",
            Kind::Account => "Account",
            Kind::Plan => "Plan",
            Kind::Profile => "Account Name",
            Kind::Cwd => "Folder",
            Kind::GitBranch => "Git Branch",
            Kind::GitStatus => "Git Status",
            Kind::ContextPercent => "Context Used %",
            Kind::ContextTokens => "Context Tokens",
            Kind::TokensInOut => "Last Request In/Out",
            Kind::CacheTokens => "Last Request Cache",
            Kind::SessionCost => "Session Cost",
            Kind::SessionDuration => "Session Duration",
            Kind::LinesChanged => "Lines Changed",
            Kind::SessionName => "Session Name",
            Kind::FiveHourUsage => "5-Hour Usage",
            Kind::SevenDayUsage => "7-Day Usage",
            Kind::FiveHourReset => "5-Hour Reset",
            Kind::SevenDayReset => "7-Day Reset",
            Kind::Effort => "Effort",
            Kind::VimMode => "Vim Mode",
            Kind::OutputStyle => "Output Style",
            Kind::Version => "Claude Code Version",
            Kind::OriginalStatusLine => "Your Own Status Line",
            Kind::CustomText => "Text",
            Kind::Separator => "Separator",
            Kind::FlexSpacer => "Flexible Space",
        }
    }

    /// The short label shown before the value when asked for.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Model => "Model",
            Kind::Account => "Account",
            Kind::Plan => "Plan",
            Kind::Profile => "Profile",
            Kind::Cwd => "cwd",
            Kind::GitBranch => "⎇",
            Kind::GitStatus => "git",
            Kind::ContextPercent | Kind::ContextTokens => "Ctx",
            Kind::TokensInOut => "Tok",
            Kind::CacheTokens => "Cache",
            Kind::SessionCost => "Cost",
            Kind::SessionDuration => "Time",
            Kind::LinesChanged => "Lines",
            Kind::SessionName => "Session",
            Kind::FiveHourUsage => "5h",
            Kind::SevenDayUsage => "7d",
            Kind::FiveHourReset => "5h reset",
            Kind::SevenDayReset => "7d reset",
            Kind::Effort => "Effort",
            Kind::VimMode => "Vim",
            Kind::OutputStyle => "Style",
            Kind::Version => "v",
            Kind::OriginalStatusLine | Kind::CustomText | Kind::Separator | Kind::FlexSpacer => "",
        }
    }

    pub fn role(self) -> Role {
        match self {
            Kind::Model | Kind::Effort | Kind::OutputStyle | Kind::Version | Kind::VimMode => {
                Role::Model
            }
            Kind::Account | Kind::Plan | Kind::Profile => Role::Identity,
            Kind::Cwd => Role::Location,
            Kind::GitBranch | Kind::GitStatus => Role::Git,
            Kind::ContextPercent | Kind::ContextTokens | Kind::TokensInOut | Kind::CacheTokens => {
                Role::Context
            }
            Kind::SessionCost | Kind::SessionDuration | Kind::LinesChanged | Kind::SessionName => {
                Role::Session
            }
            Kind::FiveHourUsage
            | Kind::SevenDayUsage
            | Kind::FiveHourReset
            | Kind::SevenDayReset => Role::Usage,
            Kind::OriginalStatusLine | Kind::CustomText | Kind::Separator | Kind::FlexSpacer => {
                Role::Text
            }
        }
    }

    pub fn is_layout(self) -> bool {
        matches!(self, Kind::Separator | Kind::FlexSpacer)
    }
}

/// One component on a line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    #[serde(default = "super::new_id")]
    pub id: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub show_label: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Widget {
    pub fn new(kind: Kind) -> Widget {
        Widget {
            id: super::new_id(),
            kind,
            foreground: None,
            background: None,
            bold: false,
            show_label: false,
            text: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Powerline {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "arrow")]
    pub separator: String,
    #[serde(default)]
    pub start_cap: String,
    #[serde(default = "arrow")]
    pub end_cap: String,
}

fn arrow() -> String {
    "\u{E0B0}".into()
}

impl Default for Powerline {
    fn default() -> Self {
        Powerline {
            enabled: false,
            separator: arrow(),
            start_cap: String::new(),
            end_cap: arrow(),
        }
    }
}

/// The layout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub schema: u32,
    #[serde(deserialize_with = "lossy_lines")]
    pub lines: Vec<Vec<Widget>>,
    pub theme: String,
    pub separator: String,
    pub padding: u8,
    pub powerline: Powerline,
    /// `16`, `256` or `truecolor`.
    pub color_level: String,
    /// `left`, `center` or `right`.
    pub alignment: String,
    pub global_bold: bool,
}

/// Lines whose unknown components (from a newer version) are dropped, not the whole file.
fn lossy_lines<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<Vec<Widget>>, D::Error> {
    let raw: Vec<Vec<serde_json::Value>> = Deserialize::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|l| {
            l.into_iter()
                .filter_map(|w| serde_json::from_value(w).ok())
                .collect()
        })
        .collect())
}

impl Default for Config {
    /// "Detailed (two lines)", Claude Pool's own two-line layout.
    fn default() -> Self {
        let w = |kind: Kind, label: bool, bold: bool| Widget {
            show_label: label,
            bold,
            ..Widget::new(kind)
        };
        Config {
            schema: 1,
            lines: vec![
                vec![
                    w(Kind::Model, false, true),
                    w(Kind::Effort, true, false),
                    w(Kind::Cwd, false, false),
                    w(Kind::GitBranch, false, false),
                    w(Kind::GitStatus, false, false),
                ],
                vec![
                    w(Kind::Account, false, false),
                    w(Kind::Plan, false, false),
                    w(Kind::FiveHourUsage, true, false),
                    w(Kind::FiveHourReset, true, false),
                    w(Kind::SevenDayUsage, true, false),
                    w(Kind::ContextTokens, true, false),
                    w(Kind::SessionCost, true, false),
                ],
            ],
            theme: "nord".into(),
            separator: " │ ".into(),
            padding: 1,
            powerline: Powerline::default(),
            color_level: "truecolor".into(),
            alignment: "left".into(),
            global_bold: false,
        }
    }
}

pub const MAX_LINES: usize = 4;
pub const MAX_WIDGETS: usize = 24;

impl Config {
    /// What is wrong with the layout, for the editor; empty when it can be applied.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.lines.iter().all(Vec::is_empty) {
            out.push("Add at least one component.".into());
        }
        if self.lines.len() > MAX_LINES {
            out.push(format!("At most {MAX_LINES} lines."));
        }
        if self.lines.iter().any(|l| l.len() > MAX_WIDGETS) {
            out.push(format!("At most {MAX_WIDGETS} components per line."));
        }
        if self.padding > 4 {
            out.push("Padding is 0 to 4.".into());
        }
        if self.separator.chars().count() > 8 {
            out.push("The separator is at most 8 characters.".into());
        }
        for w in self.lines.iter().flatten() {
            for c in [&w.foreground, &w.background].into_iter().flatten() {
                if theme::rgb(c).is_none() {
                    out.push(format!("“{c}” isn't a colour name or #RRGGBB."));
                }
            }
            if let Some(t) = &w.text
                && (t.chars().count() > 120 || t.contains(['\n', '\r', '\u{1b}']))
            {
                out.push(
                    "Text is one line of at most 120 characters, without escape characters.".into(),
                );
            }
        }
        if theme::theme(&self.theme).is_none() {
            out.push(format!("Unknown theme “{}”.", self.theme));
        }
        out
    }

    pub fn uses_git(&self) -> bool {
        self.lines
            .iter()
            .flatten()
            .any(|w| matches!(w.kind, Kind::GitBranch | Kind::GitStatus))
    }

    pub fn uses_original(&self) -> bool {
        self.lines
            .iter()
            .flatten()
            .any(|w| w.kind == Kind::OriginalStatusLine)
    }
}
