//! The Config Files panel: Save to GitHub with the one sign-in, the questions it asks, and the
//! list's keys kept from the search box.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{Action, AppContext as _, FocusHandle, Focusable as _, TestAppContext};
use sawit_core::CoreError;

use super::{OpenConfig, ShellConfigPanel};
use crate::files::Trash;

fn me() -> sawit_core::github::Account {
    sawit_core::github::Account {
        login: "me".into(),
        ..Default::default()
    }
}

/// Save Shell Files to GitHub is off while signed out, saying why, with Sign In to GitHub
/// beside it; the one sign-in turns it on in every panel at once, here two windows'.
#[gpui_kit::test]
fn saving_waits_for_the_one_sign_in(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let panels: Vec<_> = (0..2)
        .map(|_| {
            cx.update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |window, cx| {
                    cx.new(|cx| ShellConfigPanel::new(window, cx))
                })
                .unwrap()
                .1
            })
        })
        .collect();
    cx.run_until_parked();
    for panel in &panels {
        cx.update(|cx| {
            assert_eq!(
                panel.read(cx).save_blocked(cx).as_deref(),
                Some("Sign in to GitHub to save your shell files to a private repository")
            )
        });
    }
    let account = cx.update(crate::git::account::global);
    account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
    cx.run_until_parked();
    for panel in &panels {
        cx.update(|cx| assert_eq!(panel.read(cx).save_blocked(cx), None));
    }
}

/// Final seams m5: GitHub refusing the sign-in Sawit saved (revoked, expired) is like having
/// none: the app signs out everywhere and says so once (the account's notice), and Save
/// turns off with Sign In to GitHub beside it, so the panel says nothing of its own. Other
/// failures still say why, and sign nobody out.
#[gpui_kit::test]
fn a_refused_github_sign_in_signs_out_everywhere(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let (_, panel) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| ShellConfigPanel::new(window, cx))
        })
        .unwrap()
    });
    let account = cx.update(crate::git::account::global);
    account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
    let refused = CoreError::GitHub(sawit_core::github::REVOKED.into());
    panel.update(cx, |p, cx| p.github_failed(refused, "gho_1", cx));
    cx.run_until_parked();
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.error, None, "it said more than the account's notice");
        assert!(p.save_blocked(cx).is_some(), "Save is still on");
        assert_eq!(
            account.read(cx).notice(),
            Some(crate::git::account::REVOKED_NOTICE)
        );
    });

    account.update(cx, |a, cx| a.signed_in("gho_2".into(), me(), None, cx));
    let offline = "Unable to reach GitHub. Check your connection and try again.";
    panel.update(cx, |p, cx| {
        p.github_failed(CoreError::GitHub(offline.into()), "gho_2", cx)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(panel.read(cx).error.as_deref(), Some(offline));
        assert!(account.read(cx).is_signed_in());
    });
}

/// The panel lists many files of one name (each profile's `settings.json`, a project's
/// `.claude/settings.json`): deleting one for good says which, by its path.
#[test]
fn deleting_names_the_file_by_its_path() {
    use std::path::Path;
    let home = Some(Path::new("/Users/me"));
    assert_eq!(
        super::commands::delete_question(Path::new("/Users/me/.claude-work/settings.json"), home),
        "Delete “~/.claude-work/settings.json” permanently?"
    );
    assert_eq!(
        super::commands::delete_question(Path::new("/work/app/.claude/settings.json"), home),
        "Delete “/work/app/.claude/settings.json” permanently?"
    );
}

/// A key added to `~/.zshrc` after the first save is pointed out on the next save too,
/// into the repository that's there by then (or one the user made), not only the first.
#[test]
fn every_save_with_secrets_asks_first() {
    let plan = |repo: bool, secrets: &[&str]| sawit_core::shell_config::BackupPlan {
        login: "me".into(),
        repo: repo.then(|| sawit_core::github::OwnRepo {
            private: true,
            default_branch: "main".into(),
            html_url: "https://github.com/me/sawit-shell-config".into(),
        }),
        secrets: secrets.iter().map(|s| s.to_string()).collect(),
    };
    let (title, detail) =
        super::github::secrets_question(&plan(true, &["~/.zshrc:12 ANTHROPIC_KEY"]))
            .expect("didn't ask");
    assert_eq!(title, "Save lines that look like secrets?");
    assert!(
        detail.starts_with("~/.zshrc:12 ANTHROPIC_KEY\n\n"),
        "{detail}"
    );
    assert!(
        detail.contains("github.com/me/sawit-shell-config"),
        "{detail}"
    );
    assert!(!detail.contains("new private"), "{detail}");
    let (_, first) =
        super::github::secrets_question(&plan(false, &["~/.zshrc:1 X_TOKEN"])).unwrap();
    assert!(first.contains("the new private repository"), "{first}");
    assert_eq!(super::github::secrets_question(&plan(true, &[])), None);
}

/// Typing a search runs none of the list's commands. The search box passes on ↩, and ⌫ with
/// nothing before the cursor (an emptied box), to the bindings around it, where the list's
/// would open the selected file or move it to the Trash.
#[gpui_kit::test]
fn typing_in_the_search_box_runs_no_file_command(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let (window, panel) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| ShellConfigPanel::new(window, cx))
        })
        .unwrap()
    });
    let ran = Rc::new(RefCell::new(Vec::new()));
    let _observer = cx.update(|cx| {
        let ran = ran.clone();
        cx.observe_keystrokes(move |e, _, _| {
            if let Some(action) = &e.action {
                let name = action.name();
                if name.starts_with("files::") || name.starts_with("config_files::") {
                    ran.borrow_mut().push(name);
                }
            }
        })
    });
    let focus = |cx: &mut TestAppContext, handle: FocusHandle| {
        cx.update_window(window, |_, window, cx| window.focus(&handle, cx))
            .unwrap();
        cx.run_until_parked();
    };

    let search = cx.update(|cx| panel.read(cx).filter.focus_handle(cx));
    focus(cx, search);
    cx.simulate_keystrokes(
        window,
        "z s h backspace backspace backspace backspace enter",
    );
    cx.run_until_parked();
    assert_eq!(*ran.borrow(), Vec::<&str>::new(), "the search box ran them");

    // From the list they are the selected file's (none here, so nothing changes).
    let list = cx.update(|cx| panel.read(cx).focus.clone());
    focus(cx, list.clone());
    cx.simulate_keystrokes(window, "enter backspace");
    cx.run_until_parked();
    assert_eq!(*ran.borrow(), ["config_files::OpenConfig", "files::Trash"]);

    // The right-click menu, which looks its keys up from the list, still shows them.
    let key = |cx: &mut TestAppContext, action: &dyn Action| {
        cx.update_window(window, |_, window, _| {
            window
                .highest_precedence_binding_for_action_in(action, &list)
                .map(|b| b.keystrokes()[0].key().to_string())
        })
        .unwrap()
    };
    assert_eq!(key(cx, &OpenConfig).as_deref(), Some("enter"));
    assert_eq!(key(cx, &Trash).as_deref(), Some("backspace"));
}

/// ⌫ asks before it moves a config file to the Trash, as in the Files tab (FILES-1): one
/// key meant for a text field moved `~/.zshrc` away without a word, and nothing here undoes
/// a Trash.
#[gpui_kit::test]
fn backspace_asks_before_moving_a_config_file_to_the_trash(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::files::init(cx);
        super::init(cx);
    });
    let (window, panel) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| ShellConfigPanel::new(window, cx))
        })
        .unwrap()
    });
    let zshrc = std::path::PathBuf::from("/sawit-test-no-such-home/.zshrc");
    cx.update(|cx| {
        panel.update(cx, |p, _| {
            p.configs = vec![sawit_core::shell_config::ShellConfig {
                path: zshrc.clone(),
                shell: "zsh",
                note: "Interactive zsh",
                scope: sawit_core::shell_config::Scope::User,
                writable: true,
            }];
            p.selected = Some(zshrc.clone());
        })
    });
    let list = cx.update(|cx| panel.read(cx).focus.clone());
    cx.update_window(window, |_, window, cx| window.focus(&list, cx))
        .unwrap();
    cx.run_until_parked();

    // A Trash this started would fail the test at once: no runtime is installed.
    cx.simulate_keystrokes(window, "backspace");
    let (question, _) = cx.pending_prompt().expect("⌫ asks");
    assert_eq!(question, "Move “.zshrc” to the Trash?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(panel.read(cx).error, None));
}
