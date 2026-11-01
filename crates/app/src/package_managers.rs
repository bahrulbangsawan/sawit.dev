//! The package manager menu the Skills, MCP and Dev Tools pages show beside
//! an install that goes through one: npm, pnpm, Bun or Yarn, only those on
//! this Mac that can do what the install needs, the one picked checked. A
//! pick is kept for every page ([`sawit_core::package_manager::remember`]).

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::package_manager::{self, Available, PackageManager, Use};

/// What a pick in the menu calls.
type OnPick = Rc<dyn Fn(&PackageManager, &mut Window, &mut App)>;

/// "bun (bunx)": the package manager and the command it runs for `what`.
pub fn label(pm: PackageManager, what: Use) -> &'static str {
    match (pm, what) {
        (PackageManager::Npm, Use::Run) => "npm (npx)",
        (PackageManager::Pnpm, Use::Run) => "pnpm (pnpm dlx)",
        (PackageManager::Bun, Use::Run) => "bun (bunx)",
        (PackageManager::Yarn, Use::Run) => "yarn (yarn dlx)",
        (PackageManager::Npm, Use::Global) => "npm (npm install -g)",
        (PackageManager::Pnpm, Use::Global) => "pnpm (pnpm add -g)",
        (PackageManager::Bun, Use::Global) => "bun (bun add -g)",
        (PackageManager::Yarn, Use::Global) => "yarn (yarn global add)",
    }
}

/// A small button naming the package manager picked, opening the menu of
/// those in `available` that can do `what`, the picked one checked; `on`
/// gets the one picked. `None` when none can.
pub fn menu(
    id: impl Into<ElementId>,
    available: &[Available],
    what: Use,
    current: Option<PackageManager>,
    on: impl Fn(&PackageManager, &mut Window, &mut App) + 'static,
) -> Option<AnyElement> {
    let usable: Vec<PackageManager> = package_manager::usable(available, what)
        .into_iter()
        .map(|a| a.pm)
        .collect();
    let shown = match current.filter(|pm| usable.contains(pm)) {
        Some(pm) => pm,
        None => *usable.first()?,
    };
    let on: OnPick = Rc::new(on);
    Some(
        Button::new(id)
            .outline()
            .xsmall()
            .icon(Lucide::Package)
            .label(shown.id())
            .tooltip("The Package Manager That Runs It")
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _, _| {
                for pm in &usable {
                    let (pm, on) = (*pm, on.clone());
                    menu = menu.item(
                        PopupMenuItem::new(label(pm, what))
                            .checked(pm == shown)
                            .on_click(move |_, window, cx| on(&pm, window, cx)),
                    );
                }
                menu
            })
            .into_any_element(),
    )
}

/// Keeps `pm` as the choice every page starts from (best effort: a choice not
/// kept is only not remembered).
pub fn remember(pm: PackageManager) {
    if let Err(e) = package_manager::remember(&AppPaths::default_macos(), pm) {
        log::warn!("sawit: can't keep the package manager picked: {e}");
    }
}

/// The package manager picked last, on any page.
pub fn remembered() -> Option<PackageManager> {
    package_manager::remembered(&AppPaths::default_macos())
}

#[cfg(test)]
mod tests {
    use sawit_core::package_manager::{PackageManager, Use};

    use super::label;

    /// The menu names each choice with the command it runs, so the pick says what will run.
    #[test]
    fn each_choice_names_its_command() {
        assert_eq!(label(PackageManager::Bun, Use::Run), "bun (bunx)");
        assert_eq!(label(PackageManager::Pnpm, Use::Run), "pnpm (pnpm dlx)");
        assert_eq!(
            label(PackageManager::Yarn, Use::Global),
            "yarn (yarn global add)"
        );
    }
}
