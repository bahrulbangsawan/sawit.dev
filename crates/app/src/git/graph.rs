//! The commit graph in a center tab: every branch's history with lanes,
//! branch and tag labels, author and age. Clicking a commit opens it in
//! its own tab.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, Commit, Edge, GraphRow};
use sawit_core::github::{ago, now};

use super::store::{GitEvent, GitStore};
use crate::rt;
use crate::states::Placeholder;

const ROW_HEIGHT: f32 = 26.;
const LANE_WIDTH: f32 = 14.;
/// Lanes drawn before the graph column stops growing.
const MAX_LANES: usize = 12;
const PAGE: usize = 400;

pub enum GraphEvent {
    OpenCommit(Commit),
}

impl EventEmitter<GraphEvent> for GraphView {}

pub struct GraphView {
    store: Entity<GitStore>,
    /// History of just this file or folder, not the whole graph.
    path: Option<PathBuf>,
    commits: Vec<Commit>,
    rows: Vec<GraphRow>,
    lanes: usize,
    limit: usize,
    loading: bool,
    error: Option<String>,
    /// The commit last opened, highlighted.
    selected: Option<String>,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl GraphView {
    pub fn new(store: Entity<GitStore>, cx: &mut Context<Self>) -> Self {
        Self::build(store, None, cx)
    }

    /// The commits on the current branch that touch `path`.
    pub fn history(store: Entity<GitStore>, path: PathBuf, cx: &mut Context<Self>) -> Self {
        Self::build(store, Some(path), cx)
    }

    fn build(store: Entity<GitStore>, path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&store, |this, _, e: &GitEvent, cx| match e {
            GitEvent::HistoryChanged => this.reload(cx),
        })];
        let mut this = Self {
            store,
            path,
            commits: Vec::new(),
            rows: Vec::new(),
            lanes: 0,
            limit: PAGE,
            loading: true,
            error: None,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.store.read(cx).repo.clone() else {
            self.commits.clear();
            self.rows.clear();
            self.loading = false;
            cx.notify();
            return;
        };
        let limit = self.limit;
        let path = self.path.clone();
        self.loading = true;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            let rel = match &path {
                // `git` reports the repository's real path; the tree may not.
                Some(path) => {
                    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                    match real.strip_prefix(&repo) {
                        Ok(rel) if rel.as_os_str().is_empty() => Some(".".to_string()),
                        Ok(rel) => Some(rel.to_string_lossy().into_owned()),
                        Err(_) => Some(path.to_string_lossy().into_owned()),
                    }
                }
                None => None,
            };
            git::log(&repo, limit, rel.as_deref()).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(commits) => {
                        this.rows = git::graph(&commits);
                        this.lanes = git::lane_count(&this.rows).min(MAX_LANES);
                        this.commits = commits;
                        this.error = None;
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(commit) = self.commits.get(ix).cloned() else {
            return;
        };
        self.selected = Some(commit.hash.clone());
        cx.emit(GraphEvent::OpenCommit(commit));
        cx.notify();
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let (Some(commit), Some(row)) = (self.commits.get(ix), self.rows.get(ix)) else {
            return div().into_any_element();
        };
        let palette = [
            cx.theme().blue,
            cx.theme().magenta,
            cx.theme().green,
            cx.theme().yellow,
            cx.theme().cyan,
            cx.theme().red,
        ];
        let background = cx.theme().background;
        let row = row.clone();
        let merge = commit.parents.len() > 1;
        let selected = self.selected.as_ref() == Some(&commit.hash);
        let muted = cx.theme().muted_foreground;
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                paint_row(&row, merge, bounds, &palette, background, window)
            },
        )
        .w(px(LANE_WIDTH * self.lanes.max(1) as f32 + 6.))
        .h_full()
        .flex_none();
        let refs = commit
            .refs
            .iter()
            .filter(|r| !r.ends_with("/HEAD"))
            .map(|r| {
                let (label, color) = if let Some(branch) = r.strip_prefix("HEAD -> ") {
                    (branch.to_string(), cx.theme().primary)
                } else if let Some(tag) = r.strip_prefix("tag: ") {
                    (tag.to_string(), cx.theme().warning)
                } else if r == "HEAD" {
                    ("HEAD".to_string(), cx.theme().primary)
                } else {
                    (r.clone(), cx.theme().info)
                };
                div()
                    .flex_none()
                    .px_1()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(color)
                    .text_color(color)
                    .text_xs()
                    .child(label)
            });
        h_flex()
            .id(("graph-row", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pr_3()
            .gap_2()
            .text_sm()
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.open(ix, cx)))
            .child(graph)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .children(refs)
                    .child(div().min_w_0().truncate().child(commit.subject.clone())),
            )
            .child(
                div()
                    .w(px(140.))
                    .flex_none()
                    .truncate()
                    .text_color(muted)
                    .child(commit.author.clone()),
            )
            .child(
                div()
                    .w(px(96.))
                    .flex_none()
                    .truncate()
                    .text_color(muted)
                    .child(ago(commit.time, now())),
            )
            .child(
                div()
                    .w(px(60.))
                    .flex_none()
                    .text_color(muted)
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(commit.short().to_string()),
            )
            .into_any_element()
    }
}

/// Draws one row's lines and the commit's dot (a ring for merges).
fn paint_row(
    row: &GraphRow,
    merge: bool,
    bounds: Bounds<Pixels>,
    palette: &[Hsla; 6],
    background: Hsla,
    window: &mut Window,
) {
    let x = |lane: usize| {
        bounds.origin.x + px(LANE_WIDTH * (lane.min(MAX_LANES - 1) as f32 + 0.5) + 3.)
    };
    let color = |lane: usize| palette[lane % palette.len()];
    let top = bounds.origin.y;
    let bottom = top + bounds.size.height;
    let mid = top + bounds.size.height / 2.;
    let node = point(x(row.lane), mid);
    let line = |from: Point<Pixels>, to: Point<Pixels>, color: Hsla, window: &mut Window| {
        let mut path = PathBuilder::stroke(px(1.5));
        path.move_to(from);
        if from.x == to.x {
            path.line_to(to);
        } else {
            // A soft bend between lanes.
            let ctrl = point(to.x, from.y);
            path.curve_to(to, ctrl);
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    for edge in &row.edges {
        match *edge {
            Edge::Pass(l) => line(point(x(l), top), point(x(l), bottom), color(l), window),
            Edge::In(l) => line(point(x(l), top), node, color(l), window),
            Edge::Out(l) => line(node, point(x(l), bottom), color(l), window),
        }
    }
    let r = px(4.);
    let dot = Bounds::centered_at(node, size(r * 2., r * 2.));
    let c = color(row.lane);
    if merge {
        window.paint_quad(quad(dot, r, background, px(2.), c, BorderStyle::Solid));
    } else {
        window.paint_quad(fill(dot, c).corner_radii(r));
    }
}

impl Render for GraphView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let branch = store.status.as_ref().and_then(|s| s.branch.clone());
        let repo_name = self
            .path
            .as_deref()
            .or(store.repo.as_deref())
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        let count = self.commits.len();
        let more = count == self.limit;
        let toolbar = h_flex()
            .px_3()
            .py_1p5()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(Icon::new(Lucide::GitGraph).small())
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(repo_name.unwrap_or_else(|| "Commit Graph".into())),
            )
            .when_some(branch, |el, b| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("on {b}")),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{count} commit{}{}",
                        if count == 1 { "" } else { "s" },
                        if more { " shown" } else { "" }
                    )),
            )
            .when(more, |el| {
                el.child(
                    Button::new("graph-more")
                        .ghost()
                        .small()
                        .label("Show More")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.limit += PAGE;
                            this.reload(cx);
                        })),
                )
            })
            .child(
                Button::new("graph-refresh")
                    .ghost()
                    .small()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Refresh")
                    .loading(self.loading)
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            );
        let list: AnyElement = if let Some(e) = &self.error {
            Placeholder::error("Unable to read the history")
                .detail(e.clone())
                .into_any_element()
        } else if self.store.read(cx).repo.is_none() {
            Placeholder::new(Lucide::GitBranch, "No repository")
                .detail("Open a folder that's a Git repository to see its history.")
                .into_any_element()
        } else if count == 0 && self.loading {
            Placeholder::loading("Reading the history…").into_any_element()
        } else if count == 0 {
            match &self.path {
                Some(_) => Placeholder::new(Lucide::GitCommitHorizontal, "No history yet")
                    .detail("No commit on this branch has touched it."),
                None => Placeholder::new(Lucide::GitCommitHorizontal, "No commits yet")
                    .detail("Commit your first changes from the Git sidebar."),
            }
            .into_any_element()
        } else {
            uniform_list(
                "graph-rows",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| this.render_row(ix, cx)).collect()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full()
            .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(list))
    }
}
