//! Project file tree (left panel), loaded lazily per folder.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

use pgdesk_core::files::{Entry, FileKind, classify, list_dir};

use crate::rt;

pub enum FilesEvent {
    OpenFile(PathBuf),
    RootChanged(PathBuf),
}

impl EventEmitter<FilesEvent> for FilesPanel {}

/// Icon for a file in the tree, by what it opens as.
pub fn file_icon(path: &Path) -> Lucide {
    match classify(path) {
        FileKind::Image | FileKind::Svg => Lucide::Image,
        FileKind::Pdf | FileKind::Markdown => Lucide::FileText,
        FileKind::Video => Lucide::Film,
        _ if path.extension().is_some_and(|e| e == "sql") => Lucide::Database,
        _ => Lucide::FileCode,
    }
}

/// How often open folders are re-read, so files made in the terminal show up.
const REFRESH: Duration = Duration::from_secs(3);
const ROW_HEIGHT: f32 = 22.;

pub struct FilesPanel {
    pub root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Entry>>,
    selected: Option<PathBuf>,
    scroll: UniformListScrollHandle,
    _refresh: Task<()>,
}

impl FilesPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    return;
                }
            }
        });
        Self {
            root: None,
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            _refresh: refresh,
        }
    }

    /// Re-reads every open folder; the tree only changes if they did.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let dirs: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|d| self.children.contains_key(*d))
            .cloned()
            .collect();
        for dir in dirs {
            self.load(dir, cx);
        }
    }

    pub fn prompt_open(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Buka folder proyek".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.set_root(dir, cx));
        })
        .detach();
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.children.clear();
        self.expanded.insert(root.clone());
        self.load(root.clone(), cx);
        cx.emit(FilesEvent::RootChanged(root.clone()));
        self.root = Some(root);
        cx.notify();
    }

    fn load(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let read = dir.clone();
        let rx = rt::spawn(cx, async move { list_dir(&read) });
        cx.spawn(async move |this, cx| {
            match rx.await {
                Ok(Ok(entries)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.children.get(&dir) != Some(&entries) {
                            this.children.insert(dir, entries);
                            cx.notify();
                        }
                    });
                }
                // The folder was deleted or renamed: forget it.
                Ok(Err(_)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.children.remove(&dir);
                        this.expanded.remove(&dir);
                        cx.notify();
                    });
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    fn toggle(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        self.selected = Some(entry.path.clone());
        if entry.is_dir {
            if !self.expanded.remove(&entry.path) {
                self.expanded.insert(entry.path.clone());
                if !self.children.contains_key(&entry.path) {
                    self.load(entry.path.clone(), cx);
                }
            }
        } else {
            cx.emit(FilesEvent::OpenFile(entry.path.clone()));
        }
        cx.notify();
    }

    fn rows(&self, dir: &Path, depth: usize, out: &mut Vec<(Entry, usize)>) {
        let Some(entries) = self.children.get(dir) else {
            return;
        };
        for entry in entries {
            out.push((entry.clone(), depth));
            if entry.is_dir && self.expanded.contains(&entry.path) {
                self.rows(&entry.path, depth + 1, out);
            }
        }
    }
}

impl FilesPanel {
    fn flat(&self) -> Vec<(Entry, usize)> {
        let mut flat = Vec::new();
        if let Some(root) = &self.root {
            self.rows(root, 0, &mut flat);
        }
        flat
    }

    fn render_row(
        &self,
        ix: usize,
        entry: Entry,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = self.selected.as_ref() == Some(&entry.path);
        let muted = cx.theme().muted_foreground;
        let open = entry.is_dir && self.expanded.contains(&entry.path);
        let (chevron, icon) = if entry.is_dir {
            (
                Some(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                }),
                Icon::new(if open {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                }),
            )
        } else {
            (None, Icon::new(file_icon(&entry.path)))
        };
        let name = entry.name.clone();
        h_flex()
            .id(("file-row", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(4. + depth as f32 * 12.))
            .pr_2()
            .gap_1()
            .text_sm()
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .children(chevron.map(|c| Icon::new(c).xsmall().text_color(muted))),
            )
            .child(icon.xsmall().text_color(muted))
            .child(div().truncate().child(name))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&entry, cx)))
    }
}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.flat().len();
        let title = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "FILES".into());

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(title),
                    )
                    .when(self.root.is_some(), |el| {
                        el.child(
                            Button::new("refresh-files")
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCw)
                                .tooltip("Muat ulang daftar file")
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        )
                    })
                    .child(
                        Button::new("open-folder")
                            .ghost()
                            .xsmall()
                            .icon(IconName::FolderOpen)
                            .tooltip("Buka folder proyek")
                            .on_click(cx.listener(|this, _, _, cx| this.prompt_open(cx))),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .when(self.root.is_some(), |el| {
                        // Virtual: only visible rows are built.
                        el.child(
                            uniform_list(
                                "file-rows",
                                count,
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    let flat = this.flat();
                                    range
                                        .filter_map(|ix| {
                                            let (entry, depth) = flat.get(ix)?.clone();
                                            Some(this.render_row(ix, entry, depth, cx))
                                        })
                                        .collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .when(self.root.is_none(), |el| {
                        el.child(
                            div()
                                .p_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Belum ada folder. Klik ikon folder untuk membuka proyek."),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::file_icon;
    use gpui_kit::assets::IconName as Lucide;
    use std::path::Path;

    #[test]
    fn icons_follow_the_file_kind() {
        let icon = |p: &str| file_icon(Path::new(p));
        assert_eq!(icon("a.png"), Lucide::Image);
        assert_eq!(icon("a.heic"), Lucide::Image);
        assert_eq!(icon("doc.pdf"), Lucide::FileText);
        assert_eq!(icon("README.md"), Lucide::FileText);
        assert_eq!(icon("clip.mov"), Lucide::Film);
        assert_eq!(icon("q.sql"), Lucide::Database);
        assert_eq!(icon("main.rs"), Lucide::FileCode);
        assert_eq!(icon("logo.svg"), Lucide::Image);
    }
}
