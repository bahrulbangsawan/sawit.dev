//! The stack and the choices around it: the sections and cards the filter leaves, why an option
//! can't be picked, picking and taking out, presets and reset, the location and the command
//! copied, all kept for the next opening; and the keys that move through the cards.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::*;
use sawit_core::new_project::{self, Remembered};
use sawit_core::stack_builder::{self, Category, Change, Pm, Preset, Stack, StackOption};

use super::{Creating, NewProjectPage};
use crate::catalog;

impl NewProjectPage {
    /// The sections shown: each category with the options the filter leaves, in upstream's
    /// order; a category whose name matches keeps all its options.
    pub(super) fn sections(&self, query: &str) -> Vec<(Category, Vec<&'static StackOption>)> {
        Category::ALL
            .iter()
            .filter_map(|&category| {
                let all = stack_builder::options(category);
                let options: Vec<&'static StackOption> =
                    if query.is_empty() || category.label().to_lowercase().contains(query) {
                        all.iter().collect()
                    } else {
                        all.iter()
                            .filter(|o| {
                                o.name.to_lowercase().contains(query)
                                    || o.id.contains(query)
                                    || o.description.to_lowercase().contains(query)
                            })
                            .collect()
                    };
                (!options.is_empty()).then_some((category, options))
            })
            .collect()
    }

    /// The cards shown, in order: what ↑, ↓ and ↩ move through.
    pub(super) fn cards(&self, query: &str) -> Vec<(Category, &'static StackOption)> {
        self.sections(query)
            .into_iter()
            .flat_map(|(c, options)| options.into_iter().map(move |o| (c, o)))
            .collect()
    }

    /// Why option `id` of `category` can't be picked: upstream's rule, or, for a package
    /// manager, that this Mac doesn't have it.
    pub(super) fn disabled(&self, category: Category, id: &str) -> Option<String> {
        if category == Category::PackageManager
            && let Some(pms) = &self.pms
            && let Some(pm) = Pm::parse(id)
            && !pms.iter().any(|a| a.pm == pm.package_manager())
        {
            return Some(format!("{id} isn't installed on this Mac"));
        }
        stack_builder::disabled_reason(&self.stack, category, id)
    }

    /// Picks option `id` of `category`, as a click on its card does.
    pub(super) fn pick(&mut self, category: Category, id: &str, cx: &mut Context<Self>) {
        if self.disabled(category, id).is_some() || self.busy() {
            return;
        }
        let resolved = stack_builder::select(&self.stack, category, id);
        if category == Category::PackageManager {
            crate::package_managers::remember(resolved.stack.package_manager.package_manager());
        }
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Takes option `id` of `category` out of the stack, as the × on its tag does.
    pub(super) fn unpick(&mut self, category: Category, id: &str, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let resolved = stack_builder::remove(&self.stack, category, id);
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Starts from preset `preset`, keeping the name typed and the package manager picked.
    pub(super) fn apply_preset(&mut self, preset: &Preset, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let stack = Stack {
            project_name: self.stack.project_name.clone(),
            package_manager: self.stack.package_manager,
            ..preset.stack()
        };
        let resolved = stack_builder::resolve(&stack);
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Back to upstream's default stack, keeping the name typed and the package manager picked.
    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let stack = Stack {
            project_name: self.stack.project_name.clone(),
            package_manager: self.stack.package_manager,
            ..Stack::default()
        };
        self.settle(stack, Vec::new(), cx);
    }

    /// Takes `stack` as the page's, with what changed, and keeps it for next time.
    fn settle(&mut self, stack: Stack, note: Vec<Change>, cx: &mut Context<Self>) {
        self.stack = stack;
        self.note = note;
        self.problem = None;
        if matches!(self.creating, Creating::Failed { .. } | Creating::Done(_)) {
            self.creating = Creating::Idle;
        }
        self.keep(cx);
        cx.notify();
    }

    /// Writes the stack, location and package manager for the next opening (best effort).
    pub(super) fn keep(&self, cx: &mut Context<Self>) {
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let remembered = Remembered {
            stack: self.stack.clone(),
            location: Some(self.location.clone()),
        };
        cx.background_spawn(async move {
            if let Err(e) = new_project::save(&paths, &remembered) {
                log::warn!("sawit: can't keep the New Project choices: {e}");
            }
        })
        .detach();
    }

    /// Asks for the folder to create the project in.
    pub(super) fn choose_location(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            if let Some(dir) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| this.set_location(dir, cx));
            }
        })
        .detach();
    }

    /// Creates in `dir` from now on.
    pub fn set_location(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.location = dir;
        self.problem = None;
        self.keep(cx);
        cx.notify();
    }

    /// Takes the stack an agent asked for (named as it is), in `location`.
    pub fn set_stack(
        &mut self,
        stack: Stack,
        location: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let resolved = stack_builder::resolve(&stack);
        if let Some(name) = &self.name {
            let value = resolved.stack.project_name.clone();
            name.update(cx, |s, cx| s.set_value(value, window, cx));
        }
        self.location = location;
        self.settle(resolved.stack, resolved.changes, cx);
    }

    /// Copies the command, and says so for a moment.
    pub(super) fn copy_command(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(stack_builder::command(
            &self.stack,
        )));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Selects the next (or previous) card, scrolling its section into view, and gives the
    /// page the keyboard.
    pub(super) fn step(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let cards = self.cards(&query);
        self.cursor = catalog::step(self.cursor, cards.len(), down);
        if let Some(&(category, _)) = self.cursor.and_then(|ix| cards.get(ix)) {
            self.scroll_to(category, &query);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// ↩ on a card: picks it.
    pub(super) fn confirm(&mut self, cx: &mut Context<Self>) {
        let cards = self.cards(&self.query(cx));
        if let Some(&(category, option)) = self.cursor.and_then(|ix| cards.get(ix)) {
            self.pick(category, option.id, cx);
        }
    }

    /// Scrolls `category`'s section to the top of the cards.
    pub(super) fn scroll_to(&self, category: Category, query: &str) {
        let Some(ix) = self
            .sections(query)
            .iter()
            .position(|(c, _)| *c == category)
        else {
            return;
        };
        // Narrow, the page's own top block comes before the sections.
        let ix = if self.wide() { ix } else { ix + 1 };
        self.scroll.scroll_to_top_of_item(ix);
    }
}
