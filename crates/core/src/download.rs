//! Downloads PostgreSQL binaries for Sawit to manage, so projects need
//! neither Docker nor Homebrew. Uses `postgresql_archive` (theseus builds).

use std::path::{Path, PathBuf};
use std::time::Duration;

use postgresql_archive::{Version, VersionReq};

use crate::binaries::{BinarySource, PgBinaries, PgVersion, parse_version};
use crate::{CoreError, Result, cmd};

pub use postgresql_archive::configuration::theseus::URL as THESEUS_URL;

/// Installs (or reuses) binaries matching `version_req`, e.g. `"=16"`, under
/// `<managed_root>/<version>/`.
pub async fn install_managed(version_req: &str, managed_root: &Path) -> Result<PgBinaries> {
    install_managed_from(THESEUS_URL, version_req, managed_root).await
}

pub async fn install_managed_from(
    url: &str,
    version_req: &str,
    managed_root: &Path,
) -> Result<PgBinaries> {
    let req = VersionReq::parse(version_req)
        .map_err(|e| CoreError::Config(format!("{version_req}: {e}")))?;
    if let Some(dir) = existing(&req, managed_root) {
        return verify(dir.join("bin")).await;
    }

    let (version, bytes) = postgresql_archive::get_archive(url, &req)
        .await
        .map_err(|e| CoreError::Download(e.to_string()))?;
    let target = managed_root.join(version.to_string());
    tokio::fs::create_dir_all(managed_root).await?;
    // The theseus extractor unpacks into a temp dir and renames it to `target`
    // atomically (with a lock file); it skips extraction if `target` exists.
    postgresql_archive::extract(url, &bytes, &target)
        .await
        .map_err(|e| CoreError::Download(e.to_string()))?;
    if !target.join("bin").is_dir() {
        let nested = single_subdir_with_bin(&target).ok_or_else(|| {
            CoreError::Download(format!(
                "The PostgreSQL {version} archive has no bin/ folder."
            ))
        })?;
        let staging = managed_root.join(format!(".staging-{version}"));
        tokio::fs::rename(&nested, &staging).await?;
        tokio::fs::remove_dir_all(&target).await?;
        tokio::fs::rename(&staging, &target).await?;
    }
    verify(target.join("bin")).await
}

/// Newest already-installed version directory that satisfies `req`.
fn existing(req: &VersionReq, managed_root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(managed_root).ok()?;
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((Version::parse(e.file_name().to_str()?).ok()?, e.path())))
        .filter(|(v, dir)| req.matches(v) && dir.join("bin/pg_ctl").is_file())
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, dir)| dir)
}

fn single_subdir_with_bin(dir: &Path) -> Option<PathBuf> {
    let mut dirs = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir());
    let only = dirs.next()?;
    (dirs.next().is_none() && only.join("bin").is_dir()).then_some(only)
}

async fn verify(bin_dir: PathBuf) -> Result<PgBinaries> {
    let out = cmd::run(
        bin_dir.join("pg_ctl"),
        ["--version"],
        Duration::from_secs(10),
    )
    .await?;
    let version: PgVersion = parse_version(&out.stdout).ok_or_else(|| {
        CoreError::Download(format!(
            "pg_ctl in {} reports: {}",
            bin_dir.display(),
            out.stdout
        ))
    })?;
    Ok(PgBinaries {
        bin_dir,
        version,
        source: BinarySource::Managed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn existing_install_is_reused_without_network() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("16.4.0/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let pg_ctl = bin.join("pg_ctl");
        std::fs::write(&pg_ctl, "#!/bin/sh\necho 'pg_ctl (PostgreSQL) 16.4'\n").unwrap();
        std::fs::set_permissions(&pg_ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(bin.join("initdb"), "").unwrap();

        // An unreachable URL proves no network access happens.
        let found = install_managed_from("http://127.0.0.1:9/none", "=16.4.0", root.path())
            .await
            .unwrap();
        assert_eq!(found.source, BinarySource::Managed);
        assert_eq!(
            found.version,
            PgVersion {
                major: 16,
                minor: 4
            }
        );
        assert_eq!(found.bin_dir, bin);
    }

    #[tokio::test]
    #[ignore = "downloads ~30 MB from GitHub"]
    async fn downloads_real_binaries() {
        let root = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let found = install_managed("=16", root.path()).await.unwrap();
        eprintln!(
            "installed {} into {:?} in {:?}",
            found.version,
            found.bin_dir,
            started.elapsed()
        );
        assert_eq!(found.version.major, 16);
        assert!(found.tool("initdb").is_file());
        assert!(found.tool("postgres").is_file());
    }
}
