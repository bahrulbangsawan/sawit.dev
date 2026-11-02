//! The footer's Sawit Skills button, beside Install CLI: one click installs (or updates) Sawit
//! Skills for every agent; once they are current it reads Skills Installed and shows them on the
//! Skills page. Its menu has Update, Add to This Project and Remove.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::sawit_skills::Level;

use super::{AddSawitSkillsToProject, Job, confirm_remove, run, try_status};
use crate::workspace::OpenSkills;

/// What the button shows for `level`: its icon, label, accessible name and tooltip.
pub(super) fn face(level: Level) -> (Lucide, &'static str, &'static str, &'static str) {
    match level {
        Level::NotInstalled => (
            Lucide::Download,
            "Install Skills",
            "Install Sawit Skills",
            "Teach Every Agent on This Mac to Use Sawit: Skills, Instructions and Sawit's MCP Server",
        ),
        Level::Update => (
            Lucide::RefreshCw,
            "Update Skills",
            "Update Sawit Skills",
            "Bring Sawit Skills Up to This Sawit's Version in Every Agent",
        ),
        Level::Installed => (
            Lucide::CircleCheck,
            "Skills Installed",
            "Sawit Skills Installed",
            "Sawit Skills Are Current in Every Agent: Show Them on the Skills Page",
        ),
    }
}

/// The menu both the footer and the Skills page offer: install or update, add to the project,
/// show on the Skills page (not from it), and remove. The footer knows its window's `project`
/// (`Some(None)`: none is open); the Skills page (`None`) asks its window's workspace.
pub(super) fn menu(
    menu: PopupMenu,
    level: Level,
    project: Option<Option<PathBuf>>,
    skills_page: bool,
) -> PopupMenu {
    let installed = level != Level::NotInstalled;
    let (icon, label) = match level {
        Level::NotInstalled => (Lucide::Download, "Install Skills"),
        _ => (Lucide::RefreshCw, "Update Skills"),
    };
    let menu = menu
        .item(
            PopupMenuItem::new(label)
                .icon(icon)
                .on_click(|_, window, cx| run(Job::Install, None, window.window_handle(), cx)),
        )
        .item(
            PopupMenuItem::new("Add to This Project")
                .icon(Lucide::FolderPlus)
                .disabled(project.as_ref().is_some_and(Option::is_none))
                .on_click(move |_, window, cx| match &project {
                    Some(dir) => run(Job::Project, dir.clone(), window.window_handle(), cx),
                    None => window.dispatch_action(Box::new(AddSawitSkillsToProject), cx),
                }),
        )
        .separator();
    let menu = if skills_page {
        menu
    } else {
        menu.item(
            PopupMenuItem::new("Show on the Skills Page")
                .icon(Lucide::Sparkles)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSkills), cx)),
        )
    };
    menu.item(
        PopupMenuItem::new("Remove Skills…")
            .icon(Lucide::Trash)
            .disabled(!installed)
            .on_click(|_, window, cx| confirm_remove(window, cx)),
    )
}

/// The footer's button and its menu, once the first detection has ended; `project` is the
/// window's project, for Add to This Project.
pub fn footer_button(project: Option<PathBuf>, cx: &App) -> Option<AnyElement> {
    let state = try_status(cx)?.read(cx);
    let level = state.level?;
    let running = state.running;
    let (icon, label, name, tooltip) = face(level);
    let icon = match level {
        Level::Installed => Icon::new(icon).text_color(cx.theme().success),
        _ => Icon::new(icon),
    };
    let primary = Button::new("sawit-skills")
        .ghost()
        .xsmall()
        .icon(icon)
        .label(running.map_or(label, Job::doing))
        .loading(running.is_some())
        .accessibility_label(running.map_or(name, Job::doing))
        .tooltip(tooltip)
        .on_click(move |_, window, cx| match level {
            Level::Installed => window.dispatch_action(Box::new(OpenSkills), cx),
            _ => run(Job::Install, None, window.window_handle(), cx),
        });
    Some(
        DropdownButton::new("sawit-skills-split")
            .ghost()
            .xsmall()
            .button(primary)
            .disabled(running.is_some())
            .menu_accessibility_label("Sawit Skills Actions")
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |m, _, _| {
                menu(m, level, Some(project.clone()), false)
            })
            .into_any_element(),
    )
}
