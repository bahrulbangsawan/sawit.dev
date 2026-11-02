//! The lines that install a pack Sawit ships (Built In) into the agents' skills folders: each of
//! its skills copied from the folder Sawit writes it out into, a copy that differs kept in Sawit's
//! data folder, and a folder of a skill's name that isn't Sawit's left alone.

use super::backups::backup;
use super::bundled::Held;
use super::commands::{Env, Place};

/// Where a place has a folder of one of `names` that isn't Sawit's skill: the place's label and
/// the folder's name. Sawit never replaces or removes it.
pub fn not_sawits<'a>(names: &[&'a str], places: &[Place]) -> Option<(String, &'a str)> {
    places.iter().find_map(|(target, installed)| {
        let name = names
            .iter()
            .find(|name| installed.bundled.get(**name) == Some(&Held::Other))?;
        Some((target.label.clone(), *name))
    })
}

/// Copies the skills `names` Sawit ships from the folders [`run`] writes them out into first, into
/// each place: the lines stop before anything is removed when one of those folders isn't there
/// (copied and run without Sawit); a copy that differs from this Sawit's version is kept in
/// Sawit's data folder ([`backup`]), an older one replaced; a folder of a skill's name that isn't
/// Sawit's is never touched, so that place fails.
pub(super) fn bundled_lines(
    names: &[&str],
    places: &[Place],
    env: &Env,
) -> Result<Vec<String>, String> {
    if let Some((label, name)) = not_sawits(names, places) {
        return Err(format!(
            "{label} has a {name} folder that isn't Sawit's skill (its SKILL.md names another, or none), and Sawit leaves it alone: move it away to install there."
        ));
    }
    // Its messages are in double quotes, holding nothing the shell reads there.
    let missing = "is missing: Sawit writes the skill out there as it installs it, so install it from Sawit (its Skills page, or install_skill).";
    let mut lines: Vec<String> = names
        .iter()
        .map(|name| {
            let from = env.path(&env.bundled.join(name));
            format!("[ -d {from} ] || {{ echo {from} \"{missing}\" >&2; exit 1; }}")
        })
        .collect();
    for (target, installed) in places {
        let dir = env.path(&target.dir);
        lines.push(format!("mkdir -p {dir}"));
        for name in names {
            let from = env.path(&env.bundled.join(name));
            let at = env.path(&target.dir.join(name));
            if installed.bundled.get(*name) == Some(&Held::Differs) {
                let kept = backup(env, target, name);
                let parent = env.path(kept.parent().unwrap_or(&kept));
                let bak = env.path(&kept);
                lines.push(format!("rm -rf {bak}"));
                lines.push(format!("mkdir -p {parent}"));
                lines.push(format!("mv {at} {bak}"));
                let kept = "Kept the copy that differs from this Sawit's version as";
                lines.push(format!("echo \"{kept}\" {bak}"));
            } else {
                lines.push(format!("rm -rf {at}"));
            }
            lines.push(format!("cp -R {from} {dir}/"));
        }
    }
    Ok(lines)
}
