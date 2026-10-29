//! One saved server in a center tab: address, sign-in, jump host, start
//! directory, startup command and port forwards, saved as you type. Shows
//! the exact `ssh` command and can check the server before connecting.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ssh::{Auth, Probe, SshHost, plain_command, split_forwards};

use super::store::{ProbeState, SshStore, saved_key};
use crate::states::Banner;

pub enum SshHostEvent {
    Connect(String),
}

impl EventEmitter<SshHostEvent> for SshHostView {}

pub struct SshHostView {
    pub id: String,
    store: Entity<SshStore>,
    name: Entity<InputState>,
    group: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    identity: Entity<InputState>,
    password: Entity<InputState>,
    jump: Entity<InputState>,
    remote_dir: Entity<InputState>,
    startup: Entity<InputState>,
    forwards: Entity<InputState>,
    port_error: bool,
    copied: bool,
    _subscriptions: Vec<Subscription>,
}

impl SshHostView {
    pub fn new(
        id: String,
        store: Entity<SshStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let h = store
            .read(cx)
            .list
            .get(&id)
            .cloned()
            .unwrap_or_else(|| SshHost::new("", ""));
        let mut input = |placeholder: &str, value: String| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let name = input("My VPS", h.name.clone());
        let group = input("Production", h.group.clone());
        let host = input("203.0.113.7 or vps.example.com", h.host.clone());
        let port = input("22", h.port.map(|p| p.to_string()).unwrap_or_default());
        let user = input("root", h.user.clone());
        let identity = input("~/.ssh/id_ed25519", h.identity_file.clone());
        let jump = input("user@bastion.example.com:22", h.jump.clone());
        let remote_dir = input("~/apps/web", h.remote_dir.clone());
        let startup = input("tmux new -A -s main", h.startup.clone());
        let forwards = input("5432, 8080:localhost:80", h.forwards.join(", "));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });

        let mut subscriptions = Vec::new();
        let mut bind = |input: &Entity<InputState>, apply: fn(&mut SshHost, String, &mut bool)| {
            subscriptions.push(cx.subscribe(input, move |this, input, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    let v = input.read(cx).value().to_string();
                    let mut port_error = false;
                    let id = this.id.clone();
                    this.store.update(cx, |s, cx| {
                        s.update(&id, cx, |h| apply(h, v, &mut port_error))
                    });
                    this.port_error = port_error;
                    cx.notify();
                }
            }));
        };
        bind(&name, |h, v, _| h.name = v);
        bind(&group, |h, v, _| h.group = v.trim().to_string());
        bind(&host, |h, v, _| h.host = v.trim().to_string());
        bind(&port, |h, v, err| {
            let v = v.trim();
            h.port = if v.is_empty() {
                None
            } else {
                match v.parse::<u16>() {
                    Ok(p) if p > 0 => Some(p),
                    _ => {
                        *err = true;
                        None
                    }
                }
            };
        });
        bind(&user, |h, v, _| h.user = v.trim().to_string());
        bind(&identity, |h, v, _| h.identity_file = v.trim().to_string());
        bind(&jump, |h, v, _| h.jump = v.trim().to_string());
        bind(&remote_dir, |h, v, _| h.remote_dir = v.trim().to_string());
        bind(&startup, |h, v, _| h.startup = v.trim().to_string());
        bind(&forwards, |h, v, _| h.forwards = split_forwards(&v));
        subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));

        Self {
            id,
            store,
            name,
            group,
            host,
            port,
            user,
            identity,
            password,
            jump,
            remote_dir,
            startup,
            forwards,
            port_error: false,
            copied: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn title(&self, cx: &App) -> String {
        self.store
            .read(cx)
            .list
            .get(&self.id)
            .map(SshHost::label)
            .unwrap_or_else(|| "(deleted)".into())
    }

    fn host(&self, cx: &App) -> Option<SshHost> {
        self.store.read(cx).list.get(&self.id).cloned()
    }

    fn set_auth(&mut self, auth: Auth, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.store
            .update(cx, |s, cx| s.update(&id, cx, |h| h.auth = auth));
    }

    fn browse_key(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Private Key".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text = tilde(&path.to_string_lossy());
            let _ = this.update(cx, |this, cx| {
                let input = this.identity.clone();
                if let Some(window) = cx.active_window() {
                    let _ = window.update(cx, |_, window, cx| {
                        input.update(cx, |s, cx| s.set_value(text.clone(), window, cx));
                    });
                }
            });
        })
        .detach();
    }

    fn save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.password.read(cx).value().to_string();
        if value.is_empty() {
            return;
        }
        let id = self.id.clone();
        self.store
            .update(cx, |s, cx| s.set_password(&id, value, cx));
        self.password
            .update(cx, |s, cx| s.set_value("", window, cx));
    }

    fn copy_command(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.host(cx) else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(plain_command(&h)));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn render_auth(&self, h: &SshHost, has_password: bool, cx: &mut Context<Self>) -> AnyElement {
        let choice = |id: &'static str, label: &'static str, auth: Auth| {
            Button::new(id)
                .small()
                .label(label)
                .selected(h.auth == auth)
                .on_click(cx.listener(move |this, _, _, cx| this.set_auth(auth, cx)))
        };
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(choice("auth-default", "SSH Agent & Keys", Auth::Default))
                    .child(choice("auth-key", "Key File", Auth::Key))
                    .child(choice("auth-password", "Password", Auth::Password)),
            )
            .map(|el| match h.auth {
                Auth::Default => el.child(div().text_xs().text_color(muted).child(
                    "Uses your SSH agent and the keys in ~/.ssh, like the ssh command does.",
                )),
                Auth::Key => el.child(
                    h_flex()
                        .gap_1()
                        .child(div().flex_1().child(Input::new(&self.identity).small()))
                        .child(
                            Button::new("browse-key")
                                .small()
                                .label("Choose…")
                                .on_click(cx.listener(|this, _, _, cx| this.browse_key(cx))),
                        ),
                ),
                Auth::Password => el.child(if has_password {
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(
                            Icon::new(Lucide::KeyRound)
                                .small()
                                .text_color(cx.theme().success),
                        )
                        .child(
                            div()
                                .flex_1()
                                .child("Saved in the Keychain. Sawit signs in for you."),
                        )
                        .child(
                            Button::new("forget-password")
                                .small()
                                .ghost()
                                .label("Forget Password")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let id = this.id.clone();
                                    this.store
                                        .update(cx, |s, cx| s.set_password(&id, String::new(), cx));
                                })),
                        )
                        .into_any_element()
                } else {
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_1()
                                .child(div().flex_1().child(Input::new(&self.password).small()))
                                .child(
                                    Button::new("save-password")
                                        .small()
                                        .label("Save in Keychain")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_password(window, cx)
                                        })),
                                ),
                        )
                        .child(
                            div().text_xs().text_color(muted).child(
                                "Or leave it empty and type the password when the server asks.",
                            ),
                        )
                        .into_any_element()
                }),
            })
            .into_any_element()
    }
}

/// `~/…` for paths in the home folder.
fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

fn field(
    label: &'static str,
    hint: Option<&'static str>,
    child: impl IntoElement,
    cx: &App,
) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_xs().font_semibold().child(label))
        .child(child)
        .when_some(hint, |el, hint| {
            el.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
        })
}

impl Render for SshHostView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(h) = self.host(cx) else {
            return div()
                .p_4()
                .child("This server was deleted.")
                .into_any_element();
        };
        let store = self.store.read(cx);
        let has_password = store.passwords.contains(&self.id);
        let probe = store.probes.get(&saved_key(&self.id)).cloned();
        let problem = if self.port_error {
            Some("The port must be a number from 1 to 65535.".to_string())
        } else {
            h.problem()
        };
        let command = plain_command(&h);
        let theme = cx.theme();
        let (muted, border, radius) = (theme.muted_foreground, theme.border, theme.radius);
        let probe_line = probe.map(|p| match p {
            ProbeState::Checking => (Lucide::RefreshCw, muted, "Checking…".to_string()),
            ProbeState::Done(Probe::Up { latency_ms, banner }) => (
                Lucide::CircleCheck,
                theme.success,
                format!("Reachable in {latency_ms} ms · {banner}"),
            ),
            ProbeState::Done(Probe::Indirect) => (
                Lucide::Waypoints,
                muted,
                "Reached through a jump host; not checked directly.".into(),
            ),
            ProbeState::Done(Probe::Down(e)) => (Lucide::CircleAlert, theme.danger, e),
        });
        let can_connect = problem.is_none() && !self.port_error;
        let key = saved_key(&self.id);

        div()
            .id("ssh-host-view")
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .max_w(px(640.))
                    .p_6()
                    .gap_4()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Icon::new(Lucide::Server).text_color(muted))
                            .child(div().flex_1().text_lg().font_semibold().truncate().child(h.label()))
                            .child(
                                Button::new("ssh-check")
                                    .small()
                                    .icon(Lucide::RefreshCw)
                                    .label("Check")
                                    .tooltip("Check That the Server Answers")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let key = saved_key(&this.id);
                                        this.store.update(cx, |s, cx| s.probe(&key, cx));
                                    })),
                            )
                            .child(
                                Button::new("ssh-connect-main")
                                    .small()
                                    .primary()
                                    .icon(Lucide::LogIn)
                                    .label("Connect")
                                    .disabled(!can_connect)
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(SshHostEvent::Connect(key.clone()))
                                    })),
                            ),
                    )
                    .when_some(problem, |el, p| el.child(Banner::info("ssh-problem", p)))
                    .when_some(probe_line, |el, (icon, color, text)| {
                        el.child(
                            h_flex()
                                .gap_2()
                                .text_sm()
                                .text_color(color)
                                .child(Icon::new(icon).small())
                                .child(div().flex_1().whitespace_normal().child(text)),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().flex_1().child(field("Name", None, Input::new(&self.name).small(), cx)))
                            .child(div().w(px(200.)).child(field("Group", None, Input::new(&self.group).small(), cx))),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().flex_1().child(field("Address", None, Input::new(&self.host).small(), cx)))
                            .child(div().w(px(80.)).child(field("Port", None, Input::new(&self.port).small(), cx)))
                            .child(div().w(px(140.)).child(field("User", None, Input::new(&self.user).small(), cx))),
                    )
                    .child(field("Sign In With", None, self.render_auth(&h, has_password, cx), cx))
                    .child(field(
                        "Jump Host",
                        Some("Optional. Connect through this server first (ProxyJump)."),
                        Input::new(&self.jump).small(),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_3()
                            .items_start()
                            .child(div().flex_1().child(field(
                                "Start In",
                                Some("Folder to open on the server."),
                                Input::new(&self.remote_dir).small(),
                                cx,
                            )))
                            .child(div().flex_1().child(field(
                                "Run After Sign-In",
                                Some("Your shell opens after it ends."),
                                Input::new(&self.startup).small(),
                                cx,
                            ))),
                    )
                    .child(field(
                        "Port Forwards",
                        Some("Reach a server port on this Mac: 5432 forwards localhost:5432, 8080:localhost:80 maps 8080 to 80."),
                        Input::new(&self.forwards).small(),
                        cx,
                    ))
                    .child(field(
                        "Command",
                        None,
                        h_flex()
                            .gap_2()
                            .p_2()
                            .rounded(radius)
                            .border_1()
                            .border_color(border)
                            .bg(cx.theme().secondary)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_xs()
                                    .whitespace_normal()
                                    .child(command),
                            )
                            .child(
                                Button::new("ssh-copy-command")
                                    .ghost()
                                    .xsmall()
                                    .icon(if self.copied { Lucide::Check } else { Lucide::Copy })
                                    .tooltip("Copy Command"),
                            )
                            .id("ssh-command")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.copy_command(cx))),
                        cx,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Sessions to the same server share one connection, so new tabs open instantly. Changes are saved as you type."),
                    ),
            )
            .into_any_element()
    }
}
