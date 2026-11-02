//! The New Project page in the window: its tab, the create it asks for (run in a terminal tab
//! of its own, then the project opened), an agent's create, and the page counting as at work
//! while it creates.

use sawit_core::stack_builder::Stack;
use serde_json::{Value, json};

use super::*;
use crate::new_project_page::{CreateJob, NewProjectEvent, NewProjectPage};

/// The page's tab key.
const KEY: &str = "new_project";

impl Workspace {
    /// Shows the New Project page (one tab), with the tab area, which may be hidden, and the
    /// keyboard in its filter.
    pub(super) fn open_new_project_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_new_project_page(true, window, cx);
    }

    /// Shows the New Project page; `focus` gives its filter the keyboard (not when an agent
    /// opens it: the keyboard stays where the user has it).
    pub(super) fn show_new_project_page(
        &mut self,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<NewProjectPage> {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key, .. } if key.as_ref() == KEY))
            && let PaneItem::Other { view, .. } = &self.tabs[ix].item
            && let Ok(page) = view.clone().downcast::<NewProjectPage>()
        {
            self.active = ix;
            if focus {
                page.update(cx, |page, cx| page.focus_filter(window, cx));
            }
            cx.notify();
            return page;
        }
        let open = self.root(cx);
        let page = cx.new(|cx| NewProjectPage::new(open, window, cx));
        if focus {
            page.update(cx, |page, cx| page.focus_filter(window, cx));
        }
        cx.subscribe_in(
            &page,
            window,
            |this, page, e: &NewProjectEvent, window, cx| {
                this.on_new_project_event(page, e, window, cx)
            },
        )
        .detach();
        self.push_tab(
            PaneItem::Other {
                view: page.clone().into(),
                key: KEY.into(),
                title: "New Project".into(),
                icon: Lucide::FolderPlus,
                file: None,
            },
            None,
            cx,
        );
        cx.notify();
        page
    }

    fn on_new_project_event(
        &mut self,
        page: &Entity<NewProjectPage>,
        event: &NewProjectEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            NewProjectEvent::Create(job) => {
                let _ = self.start_new_project(page, job.clone(), Origin::User, window, cx);
            }
            NewProjectEvent::Open(dir) => {
                // Reads every workspace, this one included: after this update, as the Welcome
                // screen's recent folders do.
                let (dir, handle) = (dir.clone(), window.window_handle());
                cx.defer(move |cx| crate::cli::open_folder_in(dir, Some(handle), cx));
            }
        }
    }

    /// Runs `job` in a terminal tab of its own, showing the dock, and has `page` watch it.
    /// Returns the tab and pane, or why the terminal couldn't start (the page says so too).
    fn start_new_project(
        &mut self,
        page: &Entity<NewProjectPage>,
        job: CreateJob,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let started = self.terminal.update(cx, |t, cx| {
            t.start_job(
                job.tag,
                job.location,
                &job.command,
                Some(origin),
                window,
                cx,
            )
        });
        let terminal = self.terminal.downgrade();
        page.update(cx, |page, cx| match &started {
            Ok(_) => page.watch(terminal, cx),
            Err(why) => page.not_started(why.clone(), cx),
        });
        cx.notify();
        started
    }

    /// Asks before closing the New Project tab while it creates: the create goes on in its
    /// terminal, but nothing would open the project when it's done.
    pub(super) fn confirm_close_new_project(
        &mut self,
        work: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close New Project while it creates?",
            Some(&format!(
                "{work}. The create goes on in its terminal, but Sawit won't open the project when it's done."
            )),
            &["Close Anyway", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(
                        |t| matches!(&t.item, PaneItem::Other { key, .. } if key.as_ref() == KEY),
                    ) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    /// An agent's create: the page shows `stack` in `location` (the keyboard left where the
    /// user has it) and creates it as Create Project does, or, with `open` false, as Create
    /// Project Only does. Answers the folder it makes, the command and the terminal it runs in.
    pub(super) fn agent_new_project(
        &mut self,
        location: PathBuf,
        stack: Stack,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let page = self.show_new_project_page(false, window, cx);
        let job = page.update(cx, |page, cx| {
            page.set_stack(stack, location, window, cx);
            page.begin_create(open, cx)
        })?;
        let (dir, command) = (job.dir.clone(), job.command.clone());
        let (tab, pane) = self.start_new_project(&page, job, Origin::Agent, window, cx)?;
        let pane = self.terminal.read(cx).pane_index(tab, pane, cx);
        Ok(json!({
            "creating": dir,
            "command": command,
            "terminal": tab,
            "pane": pane,
            "note": if open {
                "Sawit opens the folder as a project once it has a package.json. terminal_read shows the CLI's output; app_state lists the project when it is open."
            } else {
                "Sawit makes the project without opening it (Create Project Only): the page then shows Open Project. terminal_read shows the CLI's output; open it later with app_open."
            },
        }))
    }
}

/// What the New Project page in `item` is creating, when it is that page and it creates.
pub(super) fn new_project_work(item: &PaneItem, cx: &App) -> Option<String> {
    let PaneItem::Other { view, key, .. } = item else {
        return None;
    };
    if key.as_ref() != KEY {
        return None;
    }
    view.clone()
        .downcast::<NewProjectPage>()
        .ok()?
        .read(cx)
        .work()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::{KEY, PaneItem, new_project_work};
    use crate::new_project_page::NewProjectPage;
    use crate::tabs::{CloseOp, TabFlags, to_close};

    /// The New Project tab counts as unsaved while it creates: closing it alone asks first
    /// (nothing would open the project), closing others or all leaves it open, and closing the
    /// window or quitting lists it.
    #[gpui_kit::test]
    fn a_new_project_tab_that_creates_counts_as_at_work(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = crate::testing::scratch_dir("new-project-work").join("made");
        let (window, page) = cx
            .update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |window, cx| {
                    cx.new(|cx| NewProjectPage::creating_in(dir, window, cx))
                })
            })
            .unwrap();
        let item = PaneItem::Other {
            view: page.into(),
            key: KEY.into(),
            title: "New Project".into(),
            icon: gpui_kit::assets::IconName::FolderPlus,
            file: None,
        };
        cx.update(|cx| {
            let work = new_project_work(&item, cx);
            assert_eq!(work.as_deref(), Some("Creating made"));
            assert_eq!(new_project_work(&PaneItem::Server, cx), None);
            let flags = [
                TabFlags {
                    pinned: false,
                    dirty: false,
                },
                TabFlags {
                    pinned: false,
                    dirty: work.is_some(),
                },
            ];
            assert_eq!(to_close(CloseOp::All, 0, &flags), [0]);
            assert_eq!(
                crate::new_project_page::at_work(cx),
                [(window, "Creating made".to_string())]
            );
        });
    }
}
