//! Copies of the world, so that a broken one is not the end of the server.
//!
//! Worlds break. A mod update rewrites chunks it cannot read back, a crash
//! lands mid-save, someone loads the wrong modpack once. Every one of those is
//! recoverable from a zip and unrecoverable without one, and until now this
//! app had no answer at all.
//!
//! Two decisions worth stating:
//!
//! * **Backups live beside the app's own metadata, not inside the server
//!   folder.** An imported server belongs to the user; writing gigabytes into
//!   their directory uninvited is not this app's call. It also means deleting
//!   a server does not silently delete its only copies.
//! * **Restoring takes a backup first.** Restore is the one destructive button
//!   here, and it is pressed by people who are already having a bad day.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::types::{CoreError, CoreResult};

/// One saved world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    /// File name, and the handle for restore and delete.
    pub name: String,
    pub bytes: u64,
    /// Unix seconds. Formatted by the UI, which knows the user's locale.
    pub created_secs: u64,
    /// Written automatically before a restore rather than asked for.
    pub automatic: bool,
}

/// Prefix marking a copy taken by `restore` rather than by the user.
const AUTO_PREFIX: &str = "before-restore-";

/// Everything saved for one server, newest first.
pub fn list(store: &Path) -> CoreResult<Vec<Backup>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(store) else {
        return Ok(out); // nothing backed up yet is not an error
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "zip") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        out.push(Backup {
            created_secs: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            bytes: meta.len(),
            automatic: name.starts_with(AUTO_PREFIX),
            name,
        });
    }

    out.sort_by_key(|b| std::cmp::Reverse(b.created_secs));
    Ok(out)
}

/// Zip `world` into the store. Returns the file name it wrote.
///
/// `keep` bounds the store afterwards: worlds are large, and a backup button
/// with no ceiling fills a disk quietly. Automatic copies are never counted or
/// pruned — the one taken just before a restore is the one most likely to be
/// wanted, and least likely to be asked for by name.
pub fn create(world: &Path, store: &Path, keep: usize, prefix: &str) -> CoreResult<String> {
    if !world.is_dir() {
        return Err(CoreError::Precondition {
            message: "這座伺服器還沒有世界資料夾，跑過一次之後才有東西可以備份。".into(),
        });
    }
    std::fs::create_dir_all(store)?;

    let name = format!("{prefix}{}.zip", now_secs());
    let path = store.join(&name);

    // Written to a temporary name and renamed, so an interrupted run cannot
    // leave a half-written zip that lists as a restorable backup.
    let partial = path.with_extension("partial");
    zip_dir(world, &partial)?;
    std::fs::rename(&partial, &path)?;

    prune(store, keep)?;
    Ok(name)
}

/// Replace the world with the contents of a backup.
///
/// The existing world is not deleted until its own copy is safely written: a
/// restore that fails halfway must not be the reason someone loses a world.
pub fn restore(world: &Path, store: &Path, name: &str) -> CoreResult<()> {
    let archive = safe_entry(store, name)?;
    if !archive.is_file() {
        return Err(CoreError::Precondition {
            message: format!("找不到備份 {name}。"),
        });
    }

    if world.is_dir() {
        create(world, store, usize::MAX, AUTO_PREFIX)?;
    }

    // Unpack beside the world, then swap. Extracting over the top would leave
    // files from the old world that the backup does not have.
    let staging = world.with_extension("restoring");
    let _ = std::fs::remove_dir_all(&staging);
    unzip_dir(&archive, &staging)?;

    if world.is_dir() {
        std::fs::remove_dir_all(world)?;
    }
    std::fs::rename(&staging, world)?;
    Ok(())
}

pub fn delete(store: &Path, name: &str) -> CoreResult<()> {
    std::fs::remove_file(safe_entry(store, name)?)?;
    Ok(())
}

/// Resolve a backup name to a path inside `store`, and nowhere else.
///
/// The name crosses the IPC boundary, so it is a string the UI supplies rather
/// than one this module chose. Anything with a separator or a parent component
/// is refused outright instead of being sanitised into something plausible.
fn safe_entry(store: &Path, name: &str) -> CoreResult<PathBuf> {
    let bad = name.is_empty()
        || name.len() > 128
        || name.contains(['/', '\\', ':'])
        || name.contains("..")
        || !name.ends_with(".zip");
    if bad {
        return Err(CoreError::Precondition {
            message: "備份名稱不合法。".into(),
        });
    }
    Ok(store.join(name))
}

/// Drop the oldest user-made backups past `keep`.
fn prune(store: &Path, keep: usize) -> CoreResult<()> {
    let mine: Vec<Backup> = list(store)?.into_iter().filter(|b| !b.automatic).collect();
    for old in mine.into_iter().skip(keep) {
        let _ = std::fs::remove_file(store.join(old.name));
    }
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Write named blobs into one zip. Used for the diagnostics bundle, which is
/// assembled from files in several places rather than copied from a tree.
pub fn zip_blobs(files: &[(String, Vec<u8>)], out: &Path) -> CoreResult<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        zip.start_file(name.clone(), options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}

/// Zip a directory tree, storing paths relative to its root.
fn zip_dir(root: &Path, out: &Path) -> CoreResult<()> {
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    // Deflate at the default level. A world is mostly already-compressed
    // region files, so a higher setting costs minutes and saves almost nothing.
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut stack = vec![root.to_path_buf()];
    let mut buf = Vec::new();

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let name = rel.to_string_lossy().replace('\\', "/");

            if path.is_dir() {
                zip.add_directory(format!("{name}/"), options)?;
                stack.push(path);
                continue;
            }
            // A world in use has files the server holds open; skipping one is
            // better than failing the whole backup, and this path is refused
            // while the server is running anyway.
            let Ok(mut file) = File::open(&path) else {
                continue;
            };
            buf.clear();
            if file.read_to_end(&mut buf).is_err() {
                continue;
            }
            zip.start_file(name, options)?;
            zip.write_all(&buf)?;
        }
    }

    zip.finish()?;
    Ok(())
}

/// Unpack into `dest`, refusing any entry that points outside it.
fn unzip_dir(archive: &Path, dest: &Path) -> CoreResult<()> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    std::fs::create_dir_all(dest)?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` is the zip crate's own traversal check: it returns
        // `None` for absolute paths and anything climbing out with `..`.
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let path = dest.join(rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&path)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

impl From<zip::result::ZipError> for CoreError {
    fn from(e: zip::result::ZipError) -> Self {
        CoreError::Io {
            message: format!("備份壓縮檔錯誤：{e}"),
        }
    }
}

/// Read a whole zip back as (name, bytes) pairs. Test helper.
#[cfg(test)]
fn entries(archive: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.push((name, bytes));
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with(dir: &Path, files: &[(&str, &str)]) {
        for (name, body) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
    }

    #[test]
    fn a_backup_round_trips_the_whole_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let world = tmp.path().join("world");
        let store = tmp.path().join("backups");
        world_with(&world, &[("level.dat", "root"), ("region/r.0.0.mca", "chunk")]);

        let name = create(&world, &store, 5, "world-").unwrap();
        assert_eq!(
            entries(&store.join(&name)),
            vec![
                ("level.dat".to_owned(), b"root".to_vec()),
                ("region/r.0.0.mca".to_owned(), b"chunk".to_vec()),
            ]
        );

        // A file added after the backup must be gone once it is restored:
        // restore replaces the world, it does not merge into it.
        std::fs::write(world.join("stray.dat"), "later").unwrap();
        restore(&world, &store, &name).unwrap();
        assert!(!world.join("stray.dat").exists());
        assert_eq!(
            std::fs::read_to_string(world.join("region/r.0.0.mca")).unwrap(),
            "chunk"
        );
    }

    #[test]
    fn restoring_saves_the_world_it_is_about_to_replace() {
        let tmp = tempfile::tempdir().unwrap();
        let world = tmp.path().join("world");
        let store = tmp.path().join("backups");
        world_with(&world, &[("level.dat", "first")]);

        let name = create(&world, &store, 5, "world-").unwrap();
        std::fs::write(world.join("level.dat"), "second").unwrap();
        restore(&world, &store, &name).unwrap();

        let auto: Vec<Backup> = list(&store).unwrap().into_iter().filter(|b| b.automatic).collect();
        assert_eq!(auto.len(), 1, "the replaced world was kept");
        assert_eq!(
            entries(&store.join(&auto[0].name)),
            vec![("level.dat".to_owned(), b"second".to_vec())],
            "and it is the one that was on disk, not the one restored"
        );
    }

    #[test]
    fn pruning_keeps_the_newest_and_never_touches_automatic_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("backups");
        std::fs::create_dir_all(&store).unwrap();

        // Written by hand: `create` stamps names by the second, and the test
        // must not depend on how fast it runs.
        for (name, secs) in [
            ("world-100.zip", 100),
            ("world-200.zip", 200),
            ("world-300.zip", 300),
            ("before-restore-50.zip", 50),
        ] {
            let path = store.join(name);
            std::fs::write(&path, b"x").unwrap();
            let time = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }

        prune(&store, 2).unwrap();
        let left: Vec<String> = list(&store).unwrap().into_iter().map(|b| b.name).collect();
        assert_eq!(
            left,
            vec!["world-300.zip", "world-200.zip", "before-restore-50.zip"]
        );
    }

    #[test]
    fn a_crafted_backup_name_cannot_reach_outside_the_store() {
        let store = Path::new("C:/store");
        for bad in ["../evil.zip", "sub/evil.zip", "C:/evil.zip", "evil.txt", ""] {
            assert!(safe_entry(store, bad).is_err(), "{bad} was allowed");
        }
        assert!(safe_entry(store, "world-1.zip").is_ok());
    }

    #[test]
    fn backing_up_a_server_that_never_ran_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let err = create(
            &tmp.path().join("world"),
            &tmp.path().join("backups"),
            5,
            "world-",
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::Precondition { .. }));
    }
}
