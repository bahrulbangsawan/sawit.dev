//! Picking and removing options as upstream's builder does (`use-stack-builder.ts`), the tags
//! a stack shows, and every option it disables.

use super::{
    Addon, Category, Example, Resolved, Stack, StackOption, disabled_reason, option, options,
    resolve, rules,
};

/// What picking option `id` of `category` makes of `stack`, as a click on its card does
/// upstream: nothing when it is disabled; a frontend toggles between itself and none; an addon
/// or an example toggles in its list (one task runner and one observability addon at most, the
/// list then sorted by id, as upstream sorts it); Git and Install toggle when picked again; any
/// other category takes it. The result is resolved, and its changes are what the rules did.
pub fn select(stack: &Stack, category: Category, id: &str) -> Resolved {
    let current = resolve(stack).stack;
    let mut next = current.clone();
    if disabled_reason(&current, category, id).is_none() {
        match category {
            Category::WebFrontend | Category::NativeFrontend => {
                let toggled = if current.is_picked(category, id) {
                    "none"
                } else {
                    id
                };
                next.set(category, toggled);
            }
            Category::Addons => {
                if let Some(addon) = Addon::parse(id) {
                    toggle(&mut next.addons, addon);
                    next.addons = rules::sanitize_addons(&next.addons);
                    next.addons.sort_by_key(|a| a.id());
                }
            }
            Category::Examples => {
                if let Some(example) = Example::parse(id) {
                    toggle(&mut next.examples, example);
                    next.examples = rules::dedupe(&next.examples);
                    next.examples.sort_by_key(|e| e.id());
                }
            }
            Category::Git if current.is_picked(category, id) => next.git = !current.git,
            Category::Install if current.is_picked(category, id) => next.install = !current.install,
            _ if !current.is_picked(category, id) => {
                next.set(category, id);
            }
            _ => {}
        }
    }
    resolve(&next)
}

/// What removing option `id` of `category` from the selected stack makes of `stack`, as the ×
/// on its tag does upstream: out of its list, or the category set to none when it has a none.
pub fn remove(stack: &Stack, category: Category, id: &str) -> Resolved {
    let current = resolve(stack).stack;
    let mut next = current.clone();
    match category {
        Category::Addons => next.addons.retain(|a| a.id() != id),
        Category::Examples => next.examples.retain(|e| e.id() != id),
        _ if current.is_picked(category, id) && option(category, "none").is_some() => {
            next.set(category, "none");
        }
        _ => {}
    }
    resolve(&next)
}

/// Takes `value` out of `list`, or adds it at the end (upstream's `toggleMulti`).
fn toggle<T: PartialEq>(list: &mut Vec<T>, value: T) {
    if list.contains(&value) {
        list.retain(|v| *v != value);
    } else {
        list.push(value);
    }
}

/// The stack as upstream's tags show it (`getSelectedTechs`): every option picked, in category
/// order, but none of the "none"s, "false"s, and Git or Install when on.
pub fn selected(stack: &Stack) -> Vec<(Category, &'static StackOption)> {
    let mut out = Vec::new();
    for &category in Category::ALL {
        for id in stack.picked(category) {
            let hidden = id == "none"
                || id == "false"
                || (matches!(category, Category::Git | Category::Install | Category::Auth)
                    && id == "true");
            if hidden {
                continue;
            }
            if let Some(option) = option(category, id) {
                out.push((category, option));
            }
        }
    }
    out
}

/// Every reason an option of `stack` is disabled, by category: what the page greys and
/// `stack_check` reports.
pub fn disabled(stack: &Stack) -> Vec<(Category, &'static str, String)> {
    let mut out = Vec::new();
    for &category in Category::ALL {
        for option in options(category) {
            if let Some(reason) = disabled_reason(stack, category, option.id) {
                out.push((category, option.id, reason));
            }
        }
    }
    out
}
