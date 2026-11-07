//! The Accounts page (one tab), and the terminal tab that runs a command it asks for.

use super::*;

impl Workspace {
    /// Shows the Accounts page (one tab), for the user (it takes the keyboard).
    pub(super) fn open_accounts_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_accounts_page(window, cx);
        if let Some(PaneItem::Other { view, .. }) = self.tabs.get(self.active).map(|t| &t.item)
            && let Ok(page) = view.clone().downcast::<crate::accounts::AccountsPage>()
        {
            page.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    /// Shows the Accounts page; an agent's open leaves the keyboard where it is.
    pub(super) fn show_accounts_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "accounts".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let view = cx.new(|cx| crate::accounts::AccountsPage::new(window, cx));
            // The pool's Overview lists this window's agents: in its terminals, and MCP clients.
            let ws = cx.entity().downgrade();
            view.update(cx, |page, _| {
                page.set_agents_source(std::rc::Rc::new(move |cx: &App| {
                    let Some(ws) = ws.upgrade() else {
                        return Vec::new();
                    };
                    let ws = ws.read(cx);
                    let terminals = ws.terminal.read(cx).seen_agents(cx);
                    let mcp = ws.describe_agents(cx);
                    match cx.try_global::<crate::accounts::GlobalAccounts>() {
                        Some(g) => {
                            let m = g.0.read(cx);
                            crate::accounts::agents_seen_collect(
                                &terminals,
                                &mcp,
                                &m.accounts,
                                &m.home,
                            )
                        }
                        None => Vec::new(),
                    }
                }))
            });
            let sub = cx.subscribe_in(
                &view,
                window,
                |this, _, e: &crate::accounts::AccountsEvent, window, cx| {
                    let crate::accounts::AccountsEvent::OpenTerminal {
                        title,
                        command,
                        env,
                    } = e;
                    this.open_terminal_running(title, command, env, window, cx);
                },
            );
            self._subscriptions.push(sub);
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Accounts".into(),
                    icon: Lucide::Users,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// A new terminal tab titled `title` that runs `command` with `env`, shown to the user.
    pub(crate) fn open_terminal_running(
        &mut self,
        title: &str,
        command: &str,
        env: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(Layout::default(), cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let result = self.terminal.update(cx, |t, cx| {
            t.run_in_new_tab(title, command, env, window, cx)
        });
        if let Err(e) = result {
            log::warn!("couldn't open a terminal: {e}");
        }
        cx.notify();
    }
}
