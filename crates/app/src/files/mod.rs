//! Project file tree (left panel), loaded lazily per folder.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::files::{Entry, list_dir};

use crate::rt;

pub enum FilesEvent {
    OpenFile(PathBuf),
    RootChanged(PathBuf),
}

impl EventEmitter<FilesEvent> for FilesPanel {}

pub struct FilesPanel {
    pub root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Entry>>,
    selected: Option<PathBuf>,
}

impl FilesPanel {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            root: None,
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
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
            if let Ok(Ok(entries)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.children.insert(dir, entries);
                    cx.notify();
                });
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

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut flat = Vec::new();
        if let Some(root) = &self.root {
            self.rows(root, 0, &mut flat);
        }
        let rows: Vec<_> = flat
            .into_iter()
            .enumerate()
            .map(|(ix, (entry, depth))| {
                let selected = self.selected.as_ref() == Some(&entry.path);
                let icon = if entry.is_dir {
                    if self.expanded.contains(&entry.path) {
                        Icon::new(IconName::FolderOpen)
                    } else {
                        Icon::new(IconName::Folder)
                    }
                } else if entry.name.ends_with(".sql") {
                    Icon::new(Lucide::Database)
                } else {
                    Icon::new(Lucide::FileCode)
                };
                let name = entry.name.clone();
                h_flex()
                    .id(("file-row", ix))
                    .h(px(22.))
                    .pl(px(8. + depth as f32 * 12.))
                    .pr_2()
                    .gap_1p5()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |el| el.bg(cx.theme().list_active))
                    .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
                    .child(icon.xsmall().text_color(cx.theme().muted_foreground))
                    .child(div().truncate().child(name))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&entry, cx)))
            })
            .collect();
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
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(title),
                    )
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
                    .id("file-rows")
                    .flex_1()
                    .py_1()
                    .overflow_y_scroll()
                    .children(rows)
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
