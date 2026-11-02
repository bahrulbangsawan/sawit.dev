//! Your Repositories' toolbar: the search, the filter tabs with their counts, the owner and sort
//! menus, and Refresh.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

use super::filter::{self, Filter, Sort};
use crate::catalog;
use crate::git::github::{GitHubView, Load};

/// The owner menu's first choice, and its label while it is picked.
const ALL_OWNERS: &str = "All Owners";

impl GitHubView {
    /// The search; once there are repositories to narrow, the filter tabs, counting what the
    /// search and the owner leave, then on the right the owner and sort menus; Refresh last. Too
    /// narrow for one line, it wraps.
    pub(super) fn render_repo_toolbar(
        &self,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let repos = match &self.repos {
            Load::Ready(repos) => repos.as_slice(),
            _ => &[],
        };
        let narrow = (!repos.is_empty()).then(|| {
            let scope = filter::in_scope(repos, query, self.repo_owner.as_deref());
            let current = Filter::ALL
                .iter()
                .position(|f| *f == self.repo_filter)
                .unwrap_or(0);
            let tabs = catalog::tabs(
                "gh-repos-filter",
                filter::counts(&scope),
                current,
                cx.listener(|this, ix: &usize, _, cx| {
                    this.repo_filter = Filter::ALL[*ix];
                    this.repo_paging.page = 0;
                    cx.notify();
                }),
                cx,
            );
            let owners = filter::owners(repos, self.account.read(cx).login());
            let picked = self
                .repo_owner
                .as_ref()
                .and_then(|o| owners.iter().position(|x| x == o))
                .map_or(0, |ix| ix + 1);
            let mut choices = vec![ALL_OWNERS.to_string()];
            choices.extend(owners.iter().cloned());
            let owner = catalog::menu(
                "gh-repos-owner",
                Lucide::Funnel,
                self.repo_owner.clone().unwrap_or_else(|| ALL_OWNERS.into()),
                choices,
                picked,
                cx.listener(move |this, ix: &usize, _, cx| {
                    this.repo_owner = ix.checked_sub(1).and_then(|ix| owners.get(ix).cloned());
                    this.repo_paging.page = 0;
                    cx.notify();
                }),
            );
            let sort = catalog::menu(
                "gh-repos-sort",
                Lucide::ArrowUpDown,
                format!("Sort: {}", self.repo_sort.label()),
                Sort::ALL.iter().map(|s| s.label()).collect(),
                Sort::ALL
                    .iter()
                    .position(|s| *s == self.repo_sort)
                    .unwrap_or(0),
                cx.listener(|this, ix: &usize, _, cx| {
                    this.repo_sort = Sort::ALL[*ix];
                    this.repo_paging.page = 0;
                    cx.notify();
                }),
            );
            (tabs, [owner, sort])
        });
        let (tabs, menus) = narrow.unzip();
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(catalog::search(&self.repo_search, cx))
            .children(tabs)
            .child(
                h_flex()
                    .gap_2()
                    .ml_auto()
                    .children(menus.into_iter().flatten())
                    .child(
                        Button::new("gh-repos-refresh")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .accessibility_label("Refresh")
                            .tooltip("Refresh")
                            .loading(matches!(self.repos, Load::Loading))
                            .on_click(cx.listener(|this, _, _, cx| this.load_repos(cx))),
                    ),
            )
    }
}
