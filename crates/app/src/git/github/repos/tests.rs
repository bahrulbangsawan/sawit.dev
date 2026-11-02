//! Your Repositories' tests: the tabs count what the search and the owner leave, the owner menu
//! lists yours first, each sort's order, the search, tab and owner narrowing the list together,
//! the list filling the window as the catalog pages' do, and what turns it back to its first
//! page or clears its filters.

use gpui_kit::{
    AppContext as _, Bounds, Entity, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions, point, px, size,
};
use sawit_core::github::{Account, Repo};

use super::filter::{Filter, Sort, counts, in_scope, owners, shown};
use crate::git::github::{GitHubView, Load};
use crate::git::{GitStore, account};

/// The sign-in the views in these tests hold.
const TOKEN: &str = "gho_1";

/// A public repository `full_name`, its owner the name before the slash, pushed at `pushed_at`.
fn repo(full_name: &str, pushed_at: i64) -> Repo {
    Repo {
        full_name: full_name.into(),
        owner: full_name.split_once('/').unwrap().0.into(),
        description: None,
        private: false,
        fork: false,
        archived: false,
        language: None,
        stars: 0,
        clone_url: format!("https://github.com/{full_name}.git"),
        pushed_at,
    }
}

/// Mine and two organizations', newest push first as GitHub sends them: two private, one a
/// fork, one archived, starred unevenly.
fn repos() -> Vec<Repo> {
    vec![
        Repo {
            private: true,
            stars: 3,
            language: Some("Rust".into()),
            description: Some("Postgres, an editor and a terminal".into()),
            ..repo("me/sawit", 500)
        },
        Repo {
            fork: true,
            stars: 12,
            description: Some("A code editor".into()),
            ..repo("me/zed", 400)
        },
        Repo {
            private: true,
            description: Some("Invoices".into()),
            ..repo("acme/Billing", 300)
        },
        Repo {
            archived: true,
            stars: 3,
            description: Some("The site before the redesign".into()),
            ..repo("acme/old-site", 200)
        },
        Repo {
            stars: 12,
            ..repo("Zeta/tools", 100)
        },
    ]
}

/// The names the list shows for `query`, `owner`, `filter` and `sort`, in its order.
fn listed<'a>(
    repos: &'a [Repo],
    query: &str,
    owner: Option<&str>,
    filter: Filter,
    sort: Sort,
) -> Vec<&'a str> {
    shown(in_scope(repos, query, owner), filter, sort)
        .into_iter()
        .map(|r| r.full_name.as_str())
        .collect()
}

/// The tabs count every repository the search and the owner leave, under each tab it belongs to:
/// a fork counts under Public and Forks both.
#[test]
fn the_tabs_count_what_the_search_and_the_owner_leave() {
    let repos = repos();
    assert_eq!(
        counts(&in_scope(&repos, "", None)),
        [
            ("All", 5),
            ("Public", 3),
            ("Private", 2),
            ("Forks", 1),
            ("Archived", 1)
        ]
    );
    assert_eq!(
        counts(&in_scope(&repos, "", Some("acme"))),
        [
            ("All", 2),
            ("Public", 1),
            ("Private", 1),
            ("Forks", 0),
            ("Archived", 1)
        ]
    );
    // "edit" is in two descriptions: a private repository's and a fork's.
    assert_eq!(
        counts(&in_scope(&repos, "  Edit ", None)),
        [
            ("All", 2),
            ("Public", 1),
            ("Private", 1),
            ("Forks", 1),
            ("Archived", 0)
        ]
    );
    assert!(
        counts(&in_scope(&repos, "nothing like it", None))
            .iter()
            .all(|(_, n)| *n == 0)
    );
}

/// The owner menu offers each owner once: the account signed in first, whatever the case of its
/// login, then the others A to Z whatever their case. Signed in as none of them, or not known,
/// all A to Z.
#[test]
fn the_owner_menu_lists_yours_first_then_the_rest_a_to_z() {
    let repos = repos();
    assert_eq!(owners(&repos, Some("Me")), ["me", "acme", "Zeta"]);
    assert_eq!(owners(&repos, Some("someone")), ["acme", "me", "Zeta"]);
    assert_eq!(owners(&repos, None), ["acme", "me", "Zeta"]);
    assert!(owners(&[], Some("me")).is_empty());
}

/// Last Pushed lists the newest push first, whatever order the repositories came in; Name lists
/// A to Z whatever the case; Stars lists the most starred first, equals newest push first.
/// Repositories equal in the sort keep GitHub's order.
#[test]
fn each_sort_orders_the_list_its_own_way() {
    let mut repos = repos();
    repos.reverse();
    let sorted = |sort| listed(&repos, "", None, Filter::All, sort);
    assert_eq!(
        sorted(Sort::Pushed),
        [
            "me/sawit",
            "me/zed",
            "acme/Billing",
            "acme/old-site",
            "Zeta/tools"
        ]
    );
    assert_eq!(
        sorted(Sort::Name),
        [
            "acme/Billing",
            "acme/old-site",
            "me/sawit",
            "me/zed",
            "Zeta/tools"
        ]
    );
    assert_eq!(
        sorted(Sort::Stars),
        [
            "me/zed",
            "Zeta/tools",
            "me/sawit",
            "acme/old-site",
            "acme/Billing"
        ]
    );
    let same_time = [repo("me/b", 7), repo("me/a", 7), repo("me/c", 9)];
    assert_eq!(
        listed(&same_time, "", None, Filter::All, Sort::Pushed),
        ["me/c", "me/b", "me/a"]
    );
    assert_eq!(
        listed(&same_time, "", None, Filter::All, Sort::Stars),
        ["me/c", "me/b", "me/a"]
    );
}

/// The search (in names and descriptions, whatever the case, the spaces around it ignored), the
/// tab and the owner narrow the list together.
#[test]
fn the_search_tab_and_owner_narrow_the_list_together() {
    let repos = repos();
    let list = |query, owner, filter| listed(&repos, query, owner, filter, Sort::Pushed);
    assert_eq!(
        list("EDIT", Some("me"), Filter::All),
        ["me/sawit", "me/zed"]
    );
    assert_eq!(list("EDIT", Some("me"), Filter::Private), ["me/sawit"]);
    assert_eq!(list("EDIT", Some("me"), Filter::Forks), ["me/zed"]);
    assert!(list("EDIT", Some("me"), Filter::Archived).is_empty());
    assert!(list("edit", Some("acme"), Filter::All).is_empty());
    assert_eq!(list("  site ", None, Filter::All), ["acme/old-site"]);
    assert_eq!(list("site", None, Filter::Public), ["acme/old-site"]);
    assert!(list("site", None, Filter::Private).is_empty());
    assert_eq!(
        list("invoices", Some("acme"), Filter::Private),
        ["acme/Billing"]
    );
    assert_eq!(list("", Some("Zeta"), Filter::Public), ["Zeta/tools"]);
}

/// Your Repositories in a window `width` wide, holding `repos` as if GitHub had sent them, signed
/// in as `me`. Nothing is asked of GitHub: the repositories are in place before the sign-in.
fn open_at(
    cx: &mut TestAppContext,
    width: f32,
    repos: Vec<Repo>,
) -> (Entity<GitHubView>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(width), px(900.)),
        })),
        ..Default::default()
    };
    let (window, view) = cx
        .update(|cx| {
            gpui_kit::open_window(options, cx, |window, cx| {
                let store = cx.new(GitStore::new);
                cx.new(|cx| {
                    let mut view = GitHubView::new(store, window, cx);
                    view.loaded_with = Some(TOKEN.into());
                    view.repos = Load::Ready(repos);
                    view
                })
            })
        })
        .unwrap();
    let me = Account {
        login: "me".into(),
        ..Default::default()
    };
    let account = cx.update(account::global);
    account.update(cx, |a, cx| a.signed_in(TOKEN.into(), me, None, cx));
    let cx = VisualTestContext::from_window(window, cx).into_mut();
    cx.run_until_parked();
    (view, cx)
}

/// The page fills the window as the catalog pages do: the list spans it inside the 24 px inset,
/// at the display's width and at the window's narrowest; no column caps it.
#[gpui_kit::test]
fn the_list_fills_the_window(cx: &mut TestAppContext) {
    let (_view, cx) = open_at(cx, 1470., repos());
    for width in [1470., 720.] {
        cx.simulate_resize(size(px(width), px(480.)));
        cx.run_until_parked();
        let list = cx.debug_bounds("gh-repos-list").expect("the list is drawn");
        assert_eq!(f32::from(list.origin.x).round(), 24., "{width}");
        assert_eq!(f32::from(list.size.width).round(), width - 48., "{width}");
        assert!(cx.debug_bounds("gh-repos-count").is_some(), "{width}");
    }
}

/// Typing in the search turns the list back to its first page.
#[gpui_kit::test]
fn a_new_search_starts_from_the_first_page(cx: &mut TestAppContext) {
    let many = (0..20)
        .map(|i| repo(&format!("me/r{i}"), 100 - i))
        .collect();
    let (view, cx) = open_at(cx, 1200., many);
    view.update(cx, |view, _| view.repo_paging.page = 2);
    cx.update(|window, cx| {
        let search = view.read(cx).repo_search.clone();
        search.update(cx, |s, cx| s.focus(window, cx));
    });
    cx.run_until_parked();
    cx.simulate_input("r1");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert_eq!(view.repo_search.read(cx).value(), "r1");
        assert_eq!(view.repo_paging.page, 0);
    });
}

/// With a search, a tab and an owner that leave nothing, the card says so; Clear Filters brings
/// every repository back from the first page, in the order picked.
#[gpui_kit::test]
fn clear_filters_brings_every_repository_back(cx: &mut TestAppContext) {
    let (view, cx) = open_at(cx, 1200., repos());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.repo_search
                .update(cx, |s, cx| s.set_value("site", window, cx));
            view.repo_filter = Filter::Private;
            view.repo_owner = Some("acme".into());
            view.repo_sort = Sort::Stars;
            view.repo_paging.page = 1;
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("gh-repos-list").is_some());
    assert!(cx.debug_bounds("gh-repos-count").is_none(), "no pager");
    cx.update(|window, cx| view.update(cx, |view, cx| view.clear_repo_filters(window, cx)));
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert_eq!(view.repo_search.read(cx).value(), "");
        assert_eq!(view.repo_filter, Filter::All);
        assert_eq!(view.repo_owner, None);
        assert_eq!(view.repo_sort, Sort::Stars, "the order stays");
        assert_eq!(view.repo_paging.page, 0);
    });
    assert!(cx.debug_bounds("gh-repos-count").is_some());
}
