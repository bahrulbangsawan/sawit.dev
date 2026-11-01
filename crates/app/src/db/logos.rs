//! Each kind of database's logo (assets/db_logos), for the sidebar's
//! connection rows, its add menu, the SQL editor's header and the welcome
//! screen. A logo never stands alone: the kind's name is in the row's tooltip
//! and accessible name (`ConnProfile::accessible_name`).

use gpui_kit::*;
use sawit_core::db::DbKind;

/// The "Cloudflare D1" group's header logo.
pub const CLOUDFLARE: &str = "agent_logos/cloudflare-color.svg";

/// Cloudflare's orange, which D1's one-colour product icon is drawn in.
const CLOUDFLARE_ORANGE: u32 = 0xF6821F;

/// The asset path of `kind`'s logo, and whether it has its own colours (else it is drawn in
/// Cloudflare orange).
pub fn logo_path(kind: DbKind) -> (&'static str, bool) {
    match kind {
        DbKind::Postgres => ("db_logos/postgresql-color.svg", true),
        DbKind::D1Local | DbKind::D1Remote => ("db_logos/d1.svg", false),
        DbKind::Sqlite => ("db_logos/sqlite-color.svg", true),
    }
}

/// `kind`'s logo, `size` px square.
pub fn logo(kind: DbKind, size: f32) -> AnyElement {
    match logo_path(kind) {
        (path, true) => img(path).size(px(size)).flex_none().into_any_element(),
        (path, false) => svg()
            .path(path)
            .size(px(size))
            .flex_none()
            .text_color(rgb(CLOUDFLARE_ORANGE))
            .into_any_element(),
    }
}

/// The Cloudflare logo, `size` px square.
pub fn cloudflare(size: f32) -> AnyElement {
    img(CLOUDFLARE)
        .size(px(size))
        .flex_none()
        .into_any_element()
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::{CLOUDFLARE, logo_path};
    use crate::assets::AppAssets;
    use gpui_kit::AssetSource as _;
    use sawit_core::db::DbKind;
    use sawit_core::db::{ConnProfile, d1::LocalD1};

    #[test]
    fn every_kind_has_a_logo_that_loads() {
        for kind in [
            DbKind::Postgres,
            DbKind::D1Local,
            DbKind::D1Remote,
            DbKind::Sqlite,
        ] {
            let (path, _) = logo_path(kind);
            let svg = AppAssets
                .load(path)
                .unwrap()
                .unwrap_or_else(|| panic!("{path}"));
            assert!(String::from_utf8_lossy(&svg).contains("<svg"), "{path}");
        }
        assert!(AppAssets.load(CLOUDFLARE).unwrap().is_some());
        assert_eq!(
            logo_path(DbKind::Postgres).0,
            "db_logos/postgresql-color.svg"
        );
        // No logo for kinds Sawit doesn't open.
        assert!(AppAssets.load("db_logos/mysql.svg").is_err());
    }

    #[test]
    fn accessible_names_say_the_kind_first() {
        let pg = ConnProfile {
            label: Some("kyzn".into()),
            ..ConnProfile::new("localhost", 5432, "kyzn", "me")
        };
        assert_eq!(pg.accessible_name(Some("16")), "PostgreSQL 16 · kyzn");
        assert_eq!(pg.accessible_name(None), "PostgreSQL · kyzn");
        let d1 = ConnProfile::d1_local(&LocalD1 {
            project: "/p".into(),
            worker: None,
            binding: "DB".into(),
            database_name: Some("shop".into()),
            miniflare_id: "x".into(),
            file: "/p/x.sqlite".into(),
            exists: true,
        });
        assert_eq!(d1.accessible_name(None), "Cloudflare D1 (local) · DB");
        let remote = ConnProfile::d1_remote(std::path::Path::new("/p"), "DB", "shop");
        assert_eq!(remote.accessible_name(None), "Cloudflare D1 (remote) · DB");
        let file = ConnProfile::sqlite_file(std::path::Path::new("/tmp/notes.sqlite"));
        assert_eq!(file.accessible_name(None), "SQLite · notes.sqlite");
    }
}
