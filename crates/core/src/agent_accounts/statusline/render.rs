//! Drawing a status line layout: each component's text from the session's values, themes and
//! colours, separators, Powerline segments, flexible space and alignment, then ANSI codes. The
//! same code draws the editor's preview, so what it shows is what Claude Code shows.

use serde_json::Value;

use super::theme::{THEMES, Theme, color_code, theme};
use super::{AccountLine, Config, Kind, Role, Widget};
use crate::agent_accounts::usage::{Report, Window};

/// What a status line run knows, for this run only: never written anywhere.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values {
    pub model: Option<String>,
    pub account: Option<String>,
    pub plan: Option<String>,
    pub profile: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub git_status: Option<GitStatus>,
    pub context_percent: Option<f64>,
    pub context_tokens: Option<i64>,
    pub context_size: Option<i64>,
    pub last_input: Option<i64>,
    pub last_output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub cost: Option<f64>,
    pub duration_ms: Option<f64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub session_name: Option<String>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub effort: Option<String>,
    pub vim_mode: Option<String>,
    pub output_style: Option<String>,
    pub version: Option<String>,
    pub original: Option<String>,
    pub now: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
    pub ahead: u32,
    pub behind: u32,
    pub conflicts: u32,
}

impl Values {
    /// The display fields of Claude Code's status line JSON, with the account's usage and name.
    pub fn from_payload(
        input: &[u8],
        report: &Report,
        line: &AccountLine,
        now: f64,
    ) -> Option<Values> {
        let v: Value = serde_json::from_slice(input).ok()?;
        let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(String::from);
        let f = |p: &str| v.pointer(p).and_then(Value::as_f64);
        let i = |p: &str| v.pointer(p).and_then(Value::as_i64);
        Some(Values {
            model: s("/model/display_name").or_else(|| s("/model/id")),
            cwd: s("/workspace/current_dir").or_else(|| s("/cwd")),
            session_name: s("/session_name"),
            version: s("/version"),
            output_style: s("/output_style/name"),
            cost: f("/cost/total_cost_usd"),
            duration_ms: f("/cost/total_duration_ms"),
            lines_added: i("/cost/total_lines_added"),
            lines_removed: i("/cost/total_lines_removed"),
            context_percent: f("/context_window/used_percentage"),
            context_tokens: i("/context_window/total_input_tokens"),
            context_size: i("/context_window/context_window_size"),
            last_input: i("/context_window/current_usage/input_tokens"),
            last_output: i("/context_window/current_usage/output_tokens"),
            cache_read: i("/context_window/current_usage/cache_read_input_tokens"),
            cache_write: i("/context_window/current_usage/cache_creation_input_tokens"),
            vim_mode: s("/vim/mode"),
            effort: s("/effort/level"),
            five_hour: report.five_hour,
            seven_day: report.seven_day,
            account: line.account.clone(),
            plan: line.plan.clone(),
            profile: line.profile_name.clone(),
            now,
            ..Default::default()
        })
    }
}

/// One coloured run of text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
}

fn role_ix(r: Role) -> usize {
    match r {
        Role::Model => 0,
        Role::Identity => 1,
        Role::Location => 2,
        Role::Git => 3,
        Role::Context => 4,
        Role::Session => 5,
        Role::Usage => 6,
        Role::Text => 7,
    }
}

fn compact(v: i64) -> String {
    match v {
        i64::MIN..1_000 => v.to_string(),
        1_000..1_000_000 => format!("{:.1}k", v as f64 / 1e3).replace(".0k", "k"),
        _ => format!("{:.1}M", v as f64 / 1e6).replace(".0M", "M"),
    }
}

fn duration(s: i64) -> String {
    let s = s.max(0);
    match s {
        86_400.. => format!("{}d {}h", s / 86_400, s % 86_400 / 3_600),
        3_600.. => format!("{}h {}m", s / 3_600, s % 3_600 / 60),
        60.. => format!("{}m", s / 60),
        _ => format!("{s}s"),
    }
}

/// Control characters out, at most 200 characters: a value can't move the cursor.
fn sanitize(t: &str) -> String {
    t.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// A component's text, its label first when asked; `None` when there is nothing to show.
pub fn text_for(w: &Widget, v: &Values) -> Option<String> {
    let usage = |win: Option<Window>| {
        win.filter(|w| w.resets_at > v.now)
            .map(|w| format!("{}%", w.used_percentage.round()))
    };
    let reset = |win: Option<Window>| {
        win.filter(|w| w.resets_at > v.now)
            .map(|w| duration((w.resets_at - v.now) as i64))
    };
    let raw: Option<String> = match w.kind {
        Kind::Model => v.model.clone(),
        Kind::Account => v.account.clone(),
        Kind::Plan => v.plan.clone(),
        Kind::Profile => v.profile.clone(),
        Kind::Cwd => v.cwd.as_ref().map(|p| {
            p.rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(p)
                .to_string()
        }),
        Kind::GitBranch => v.git_branch.clone(),
        Kind::GitStatus => v.git_status.map(|s| {
            let parts: Vec<String> = [
                ("!", s.conflicts),
                ("+", s.staged),
                ("~", s.modified),
                ("?", s.untracked),
                ("↑", s.ahead),
                ("↓", s.behind),
            ]
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(p, n)| format!("{p}{n}"))
            .collect();
            if parts.is_empty() {
                "✓".into()
            } else {
                parts.join(" ")
            }
        }),
        Kind::ContextPercent => v.context_percent.map(|p| format!("{}%", p.round())),
        Kind::ContextTokens => v.context_tokens.map(|t| match v.context_size {
            Some(s) if s > 0 => format!("{}/{}", compact(t), compact(s)),
            _ => compact(t),
        }),
        Kind::TokensInOut => v
            .last_input
            .zip(v.last_output)
            .map(|(i, o)| format!("↑{} ↓{}", compact(i), compact(o))),
        Kind::CacheTokens => v
            .cache_read
            .zip(v.cache_write)
            .map(|(r, w)| format!("r {} w {}", compact(r), compact(w))),
        Kind::SessionCost => v.cost.map(|c| format!("${c:.2}")),
        Kind::SessionDuration => v.duration_ms.map(|d| duration((d / 1000.0) as i64)),
        Kind::LinesChanged => v
            .lines_added
            .zip(v.lines_removed)
            .map(|(a, r)| format!("+{a} −{r}")),
        Kind::SessionName => v.session_name.clone(),
        Kind::FiveHourUsage => usage(v.five_hour),
        Kind::SevenDayUsage => usage(v.seven_day),
        Kind::FiveHourReset => reset(v.five_hour),
        Kind::SevenDayReset => reset(v.seven_day),
        Kind::Effort => v.effort.clone(),
        Kind::VimMode => v.vim_mode.clone(),
        Kind::OutputStyle => v.output_style.clone().filter(|s| s != "default"),
        Kind::Version => v.version.clone(),
        Kind::OriginalStatusLine => v.original.clone(),
        Kind::CustomText => w.text.clone(),
        Kind::Separator => Some(w.text.clone().unwrap_or_else(|| "│".into())),
        Kind::FlexSpacer => Some(String::new()),
    };
    let value = sanitize(&raw?);
    if value.is_empty() && w.kind != Kind::FlexSpacer {
        return None;
    }
    Some(if w.show_label && !w.kind.label().is_empty() {
        format!("{} {value}", w.kind.label())
    } else {
        value
    })
}

struct Piece {
    text: String,
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
    sep: bool,
    flex: bool,
}

/// The layout drawn with `v`: one list of segments per line that shows anything. `width` is the
/// terminal's, for flexible space and alignment.
pub fn segments(c: &Config, v: &Values, width: Option<usize>) -> Vec<Vec<Segment>> {
    let t = theme(&c.theme).unwrap_or(&THEMES[0]);
    c.lines
        .iter()
        .map(|l| line(c, t, l, v, width))
        .filter(|l| !l.is_empty())
        .collect()
}

fn line(
    c: &Config,
    t: &Theme,
    widgets: &[Widget],
    v: &Values,
    width: Option<usize>,
) -> Vec<Segment> {
    let mut pieces: Vec<Piece> = Vec::new();
    for w in widgets {
        let Some(text) = text_for(w, v) else { continue };
        let r = role_ix(w.kind.role());
        let sep = w.kind == Kind::Separator;
        let piece = Piece {
            text,
            fg: w
                .foreground
                .clone()
                .or_else(|| Some(if sep { t.separator } else { t.fg[r] }.to_string())),
            bg: w.background.clone().or_else(|| {
                if w.kind.is_layout() {
                    None
                } else {
                    t.bg.map(|b| b[r].to_string())
                }
            }),
            bold: w.bold || c.global_bold,
            sep,
            flex: w.kind == Kind::FlexSpacer,
        };
        // No separator at an edge or beside another.
        if piece.sep && pieces.last().is_none_or(|p| p.sep || p.flex) {
            continue;
        }
        pieces.push(piece);
    }
    while pieces.last().is_some_and(|p| p.sep) {
        pieces.pop();
    }
    if !pieces.iter().any(|p| !p.flex) {
        return Vec::new();
    }
    let mut groups: Vec<Vec<Piece>> = vec![Vec::new()];
    for p in pieces {
        if p.flex {
            groups.push(Vec::new());
        } else if let Some(g) = groups.last_mut() {
            g.push(p);
        }
    }
    let had_flex = groups.len() > 1;
    let drawn: Vec<Vec<Segment>> = groups
        .iter()
        .map(|g| group(c, t, g))
        .filter(|g| !g.is_empty())
        .collect();
    if drawn.is_empty() {
        return Vec::new();
    }
    let len = |s: &[Segment]| s.iter().map(|x| x.text.chars().count()).sum::<usize>();
    let content: usize = drawn.iter().map(|g| len(g)).sum();
    let available = width.map(|w| w.saturating_sub(4));
    let mut out = Vec::new();
    match available {
        Some(avail) if had_flex && drawn.len() > 1 => {
            let gaps = drawn.len() - 1;
            let free = gaps.max(avail.saturating_sub(content));
            for (i, g) in drawn.into_iter().enumerate() {
                out.extend(g);
                if i < gaps {
                    let share = free / gaps + usize::from(i < free % gaps);
                    out.push(Segment {
                        text: " ".repeat(share.max(1)),
                        ..Default::default()
                    });
                }
            }
        }
        _ => {
            for (i, g) in drawn.into_iter().enumerate() {
                if i > 0 {
                    out.push(Segment {
                        text: " ".into(),
                        ..Default::default()
                    });
                }
                out.extend(g);
            }
        }
    }
    if let Some(avail) = available
        && c.alignment != "left"
    {
        let free = avail.saturating_sub(len(&out));
        let lead = if c.alignment == "right" {
            free
        } else {
            free / 2
        };
        if lead > 0 {
            out.insert(
                0,
                Segment {
                    text: " ".repeat(lead),
                    ..Default::default()
                },
            );
        }
    }
    out
}

fn group(c: &Config, t: &Theme, pieces: &[Piece]) -> Vec<Segment> {
    let pad = " ".repeat(c.padding.min(4) as usize);
    let mut out = Vec::new();
    if c.powerline.enabled {
        let content: Vec<&Piece> = pieces.iter().filter(|p| !p.sep).collect();
        let Some(first) = content.first() else {
            return out;
        };
        if !c.powerline.start_cap.is_empty() {
            out.push(Segment {
                text: c.powerline.start_cap.clone(),
                fg: first.bg.clone().or(first.fg.clone()),
                ..Default::default()
            });
        }
        for (i, p) in content.iter().enumerate() {
            out.push(Segment {
                text: format!("{pad}{}{pad}", p.text),
                fg: p.fg.clone(),
                bg: p.bg.clone(),
                bold: p.bold,
            });
            match content.get(i + 1) {
                Some(next) => out.push(Segment {
                    text: c.powerline.separator.clone(),
                    fg: p.bg.clone().or(p.fg.clone()),
                    bg: next.bg.clone(),
                    bold: false,
                }),
                None if !c.powerline.end_cap.is_empty() => out.push(Segment {
                    text: c.powerline.end_cap.clone(),
                    fg: p.bg.clone().or(p.fg.clone()),
                    ..Default::default()
                }),
                None => {}
            }
        }
        return out;
    }
    for (i, p) in pieces.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| pieces.get(j));
        if let Some(prev) = prev
            && !p.sep
            && !prev.sep
            && !c.separator.is_empty()
        {
            out.push(Segment {
                text: c.separator.clone(),
                fg: Some(t.separator.into()),
                ..Default::default()
            });
        }
        if p.sep {
            out.push(Segment {
                text: format!(" {} ", p.text),
                fg: p.fg.clone(),
                bold: p.bold,
                ..Default::default()
            });
        } else {
            let text = if p.bg.is_some() {
                format!("{pad}{}{pad}", p.text)
            } else {
                p.text.clone()
            };
            out.push(Segment {
                text,
                fg: p.fg.clone(),
                bg: p.bg.clone(),
                bold: p.bold,
            });
        }
    }
    out
}

/// The layout as ANSI text, a line per line.
pub fn ansi(c: &Config, v: &Values, width: Option<usize>) -> String {
    segments(c, v, width)
        .iter()
        .map(|l| l.iter().map(|s| ansi_segment(c, s)).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ansi_segment(c: &Config, s: &Segment) -> String {
    let mut codes: Vec<String> = Vec::new();
    if s.bold {
        codes.push("1".into());
    }
    if let Some(code) = s.fg.as_deref().and_then(|f| color_code(c, f, false)) {
        codes.push(code);
    }
    if let Some(code) = s.bg.as_deref().and_then(|b| color_code(c, b, true)) {
        codes.push(code);
    }
    if codes.is_empty() {
        return s.text.clone();
    }
    format!("\u{1b}[{}m{}\u{1b}[0m", codes.join(";"), s.text)
}
