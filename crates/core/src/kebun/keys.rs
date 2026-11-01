//! `sawit list keys`: the key bindings, from the running app (the one list that needs it).

use std::time::Duration;

use crate::instances::AppPaths;

use super::ink::{Ink, clip, tilde};

/// `sawit list keys`: every key binding, as the Keymap Editor lists it, from the running app
/// (`sawit-mcp`'s `keymap_list` asks the same). The defaults are bound in the app's code, so only
/// the running app knows them: when it isn't running, this says so rather than start it.
pub(super) fn list_keys(ink: &Ink, paths: &AppPaths) -> i32 {
    let problem = Ink::stderr();
    let keymap = tilde(&paths.keymap_file());
    if !crate::app_control::is_running(paths) {
        eprintln!(
            "{} Sawit sedang padam, jadi peta pintasannya ikut gelap: pintasan bawaan hanya hidup di dalam aplikasi.\n   Nyalakan dulu dengan `sawit`, lalu ulangi `sawit list keys`. Pintasanmu sendiri ada di {keymap}.",
            problem.red("🔥")
        );
        return 1;
    }
    let request = crate::app_control::Request {
        project: None,
        agent_folder: None,
        agent: None,
        command: crate::app_control::Command::Keymap {
            op: crate::keymap::KeymapOp::List {
                filter: None,
                keys: None,
                unbound: false,
            },
        },
    };
    let list = crate::app_control::send(paths, &request, Duration::from_secs(10)).and_then(|v| {
        serde_json::from_value::<crate::keymap::KeymapList>(v)
            .map_err(|e| format!("Sawit menjawab dengan peta yang tidak terbaca: {e}"))
    });
    let list = match list {
        Ok(list) => list,
        Err(e) => {
            eprintln!("{} Gagal membaca peta pintasan: {e}", problem.red("🔥"));
            return 1;
        }
    };
    ink.section("⌨️  Pintasan (keys)");
    if list.rows.is_empty() {
        ink.empty("tidak ada satu pun pintasan. Lahannya kosong melompong.");
    }
    for row in &list.rows {
        println!("{}", key_line(ink, row));
    }
    for error in &list.errors {
        ink.warn(&format!("keymap.json: {error}"));
    }
    let count = |source: crate::keymap::KeySource| {
        list.rows.iter().filter(|row| row.source == source).count()
    };
    let clashing = list.rows.iter().filter(|row| row.conflict).count();
    println!(
        "  {}",
        ink.gray(&format!(
            "{} pintasan · {} milikmu · {} dicabut · {clashing} bentrok",
            list.rows.len(),
            count(crate::keymap::KeySource::User),
            count(crate::keymap::KeySource::Removed),
        ))
    );
    println!(
        "  {}",
        ink.gray(&format!(
            "“dicabut” = bawaan yang kamu matikan; “bentrok” = tombol yang sama di konteks yang sama, cuma satu yang jalan. Ubah di Keymap Editor (⌘K ⌘S) atau di {keymap}."
        ))
    );
    0
}

/// One binding as `sawit list keys` prints it: the keys as a Mac menu writes them and as
/// keymap.json does, what they run, where, and whose binding it is.
pub(super) fn key_line(ink: &Ink, row: &crate::keymap::KeymapRow) -> String {
    use crate::keymap::KeySource;
    let source = match row.source {
        KeySource::Default => ink.gray("bawaan"),
        KeySource::User => ink.green("milikmu"),
        KeySource::Removed => ink.yellow("dicabut"),
        KeySource::Unbound => ink.gray("tanpa pintasan"),
    };
    let clash = match row.conflict {
        true => format!(" {}", ink.red("bentrok")),
        false => String::new(),
    };
    let context = row.context.as_deref().unwrap_or("di mana saja");
    format!(
        "  {:<12} {:<44} {}  {source}{clash}",
        row.display,
        clip(&row.label, 44),
        ink.gray(&format!("{} · {}", row.keys, clip(context, 40))),
    )
}
