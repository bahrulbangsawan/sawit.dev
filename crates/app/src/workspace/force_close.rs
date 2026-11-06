//! Safe Close All and Force Close All for the center tabs (the user's request with item 22).
//! Safe Close All is the old Close All: it closes only tabs that lose nothing (no unsaved changes,
//! no browser notes not yet sent, no install going) and aren't pinned, and says what it kept.
//! Force Close All closes every tab, pinned ones too, and so asks first (the shared confirm),
//! naming what would be lost or stopped: "Close 7 tabs and discard changes to 2 files?".

use super::pages::{skills_work, tools_work};
use super::*;

/// What closing every tab would lose: files with unsaved changes, browsers' notes, work going on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Losing {
    pub files: Vec<String>,
    pub notes: usize,
    pub work: Vec<String>,
}

impl Losing {
    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.notes == 0 && self.work.is_empty()
    }
}

/// The question before Force Close All closes `tabs` tabs losing `losing`; `None` when nothing
/// would be lost.
pub(crate) fn question(tabs: usize, losing: &Losing) -> Option<crate::confirm::Confirm> {
    if losing.is_empty() {
        return None;
    }
    let tabs = if tabs == 1 {
        "1 tab".to_string()
    } else {
        format!("{tabs} tabs")
    };
    let mut lost = Vec::new();
    match losing.files.as_slice() {
        [] => {}
        [one] => lost.push(format!("discard changes to {one}")),
        many => lost.push(format!("discard changes to {} files", many.len())),
    }
    if losing.notes > 0 {
        lost.push("drop the browser's annotations".into());
    }
    if !losing.work.is_empty() {
        lost.push("stop what installs".into());
    }
    let title = format!("Close {tabs} and {}?", lost.join(" and "));
    let mut items: Vec<String> = losing.files.clone();
    items.extend(losing.work.iter().cloned());
    Some(
        crate::confirm::Confirm::new(title, "Force Close All")
            .detail("Unsaved changes are lost; pinned tabs close too.")
            .items(items),
    )
}

impl Workspace {
    fn losing_all(&self, cx: &App) -> Losing {
        let mut losing = Losing::default();
        for t in &self.tabs {
            match &t.item {
                PaneItem::File(e) if e.read(cx).dirty => losing.files.push(e.read(cx).title()),
                PaneItem::Other { view, .. } => {
                    if let Ok(b) = view.clone().downcast::<BrowserView>() {
                        losing.notes += b.read(cx).unsent_notes(cx);
                    }
                    losing.work.extend(tools_work(&t.item, cx));
                    losing.work.extend(skills_work(&t.item, cx));
                    losing
                        .work
                        .extend(new_project::new_project_work(&t.item, cx));
                }
                _ => {}
            }
        }
        losing
    }

    /// Force Close All: every tab, after asking when something would be lost.
    pub(super) fn force_close_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let losing = self.losing_all(cx);
        let Some(question) = question(self.tabs.len(), &losing) else {
            self.close_every_tab(cx);
            return;
        };
        let answer = question.ask(window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await {
                let _ = this.update(cx, |this, cx| this.close_every_tab(cx));
            }
        })
        .detach();
    }

    /// Closes every tab, discarding the files' unsaved changes.
    fn close_every_tab(&mut self, cx: &mut Context<Self>) {
        for t in &self.tabs {
            if let PaneItem::File(editor) = &t.item {
                editor.update(cx, |e, _| e.discard());
            }
        }
        let all: Vec<usize> = (0..self.tabs.len()).collect();
        self.remove_tabs(&all, cx);
    }

    /// An agent's `app_close_all_tabs`: safe (the tabs that lose nothing and aren't pinned) or
    /// `force` (every tab, refused without `confirm` while something would be lost). Never asks
    /// the user, and never moves the keyboard.
    pub(super) fn close_all_for_agent(
        &mut self,
        force: bool,
        confirm: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        let before = self.tabs.len();
        if !force {
            let target = self.active;
            self.close(CloseOp::All, target, window, cx);
            let kept = self.tabs.len();
            return Ok(serde_json::json!({ "closed": before - kept, "kept": kept }));
        }
        let losing = self.losing_all(cx);
        if !losing.is_empty() && !confirm {
            let what = question(before, &losing)
                .map(|q| q.title.to_string())
                .unwrap_or_default();
            return Err(format!(
                "Nothing was closed: {what} Pass confirm: true to close every tab anyway."
            ));
        }
        self.close_every_tab(cx);
        Ok(serde_json::json!({ "closed": before, "kept": 0 }))
    }

    /// Safe Close All, saying what it kept: tabs pinned or that would lose something.
    pub(super) fn safe_close_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.active;
        self.close(CloseOp::All, target, window, cx);
        let note = match self.tabs.len() {
            0 => return,
            1 => "Kept 1 tab: pinned, or with something to lose.".to_string(),
            n => format!("Kept {n} tabs: pinned, or with something to lose."),
        };
        self.show_close_note(note, cx);
    }

    /// Shows `note` in the status bar for a few seconds.
    pub(super) fn show_close_note(&mut self, note: String, cx: &mut Context<Self>) {
        self.close_note = Some(note.into());
        cx.notify();
        self._close_note_reset = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(4))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.close_note = None;
                cx.notify();
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Losing, question};

    #[test]
    fn force_close_all_names_what_is_lost() {
        assert!(question(3, &Losing::default()).is_none());
        let q = question(
            7,
            &Losing {
                files: vec!["a.rs".into(), "b.rs".into()],
                notes: 0,
                work: vec![],
            },
        )
        .unwrap();
        assert_eq!(
            q.title.as_ref(),
            "Close 7 tabs and discard changes to 2 files?"
        );
        assert_eq!(q.verb.as_ref(), "Force Close All");
        let q = question(
            1,
            &Losing {
                files: vec!["a.rs".into()],
                notes: 2,
                work: vec!["Installing Bun".into()],
            },
        )
        .unwrap();
        assert_eq!(
            q.title.as_ref(),
            "Close 1 tab and discard changes to a.rs and drop the browser's annotations and stop what installs?"
        );
    }
}
