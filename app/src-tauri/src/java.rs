//! Finding a JRE, and fetching one when there is none.
//!
//! Nothing is downloaded up front. A Java runtime is ~45 MB and most people
//! running a Minecraft server already have one; the app looks first, and only
//! when a launch would actually fail does it offer to fetch the exact major
//! that version needs.
//!
//! Two places to look, in order:
//!
//! 1. `<app data>/java/<major>/…/bin/java.exe` — what this app downloaded.
//! 2. `java` on `PATH`, if its version happens to match.
//!
//! The order matters: a machine with Java 8 on `PATH` (still common, Minecraft
//! shipped it for years) must not have that picked for a 1.20.6 server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::types::{CoreError, CoreResult};

/// Adoptium's redirect endpoint. Asking for `latest/ga` means the URL never
/// names a build number, so it does not go stale the way a pinned one would.
/// reqwest follows the redirect, so this needs no API response parsing.
fn adoptium_url(major: u8) -> String {
    format!(
        "https://api.adoptium.net/v3/binary/latest/{major}/ga/windows/x64/jre/hotspot/normal/eclipse"
    )
}

pub struct Java {
    /// Where downloaded runtimes are unpacked, one directory per major.
    root: PathBuf,
    /// Results of probing `java` on PATH, keyed by the major asked for.
    ///
    /// The probe runs a process and waits ~100 ms for it, from inside an async
    /// command. Once per major per session is tolerable; once per launch is
    /// not, and the answer cannot change while the app is open — except when
    /// this app installs a runtime, which clears the entry.
    probed: Mutex<HashMap<u8, Option<PathBuf>>>,
}

impl Java {
    pub fn new(root: impl Into<PathBuf>) -> CoreResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            probed: Mutex::new(HashMap::new()),
        })
    }

    /// A `java.exe` that can run `major`, or `None` if the machine has none.
    pub fn find(&self, major: u8) -> Option<PathBuf> {
        if let Some(found) = self.managed(major) {
            return Some(found);
        }
        let mut probed = self.probed.lock().expect("java mutex poisoned");
        probed
            .entry(major)
            .or_insert_with(|| path_java(major))
            .clone()
    }

    /// The archive for this major lands here.
    pub fn archive_path(&self, major: u8) -> PathBuf {
        self.root.join(format!("jre-{major}.zip"))
    }

    pub fn url(&self, major: u8) -> String {
        adoptium_url(major)
    }

    /// A runtime this app already unpacked.
    ///
    /// The archive contains a single versioned top-level directory
    /// (`jdk-17.0.9+9-jre/`) whose name changes with every release, so this
    /// scans for the `bin/java.exe` inside rather than guessing the name.
    fn managed(&self, major: u8) -> Option<PathBuf> {
        let dir = self.root.join(major.to_string());
        let direct = dir.join("bin").join("java.exe");
        if direct.is_file() {
            return Some(direct);
        }
        std::fs::read_dir(&dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|e| e.path().join("bin").join("java.exe"))
            .find(|p| p.is_file())
    }

    /// Unpack a downloaded archive into `<root>/<major>` and return the
    /// `java.exe` inside it.
    pub fn install(&self, archive: &Path, major: u8) -> CoreResult<PathBuf> {
        let dir = self.root.join(major.to_string());
        // A previous attempt may have left a half-extracted tree; starting from
        // empty is the only way to be sure of what is in there.
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;

        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| CoreError::Java {
            message: format!("JRE 壓縮檔讀取失敗：{e}"),
        })?;
        zip.extract(&dir).map_err(|e| CoreError::Java {
            message: format!("JRE 解壓縮失敗：{e}"),
        })?;
        // The archive is large and every byte of it is now on disk twice.
        let _ = std::fs::remove_file(archive);
        // A "no java for this major" answer from before the install is now
        // wrong; the managed lookup will answer first from here on anyway.
        self.probed
            .lock()
            .expect("java mutex poisoned")
            .remove(&major);

        self.managed(major).ok_or_else(|| CoreError::Java {
            message: "解壓縮後找不到 java.exe。".into(),
        })
    }
}

/// `java` on `PATH`, but only if it is the major we need.
fn path_java(major: u8) -> Option<PathBuf> {
    (probe_major(Path::new("java"))? == major).then(|| PathBuf::from("java"))
}

/// The major version a `java` binary reports, or `None` if it will not run.
///
/// `java -version` writes to stderr, in one of two shapes:
/// `openjdk version "17.0.9"` (9+) or `java version "1.8.0_202"` (8 and older).
pub fn probe_major(exe: &Path) -> Option<u8> {
    let mut command = Command::new(exe);
    command.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().ok()?;
    parse_major(&String::from_utf8_lossy(&out.stderr))
}

fn parse_major(text: &str) -> Option<u8> {
    let quoted = text.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-']);
    let first: u8 = parts.next()?.parse().ok()?;
    // "1.8.0_202" is Java 8: before 9 the major sat in the second position.
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_version_string_shapes() {
        assert_eq!(
            parse_major("openjdk version \"17.0.9\" 2023-10-17"),
            Some(17)
        );
        assert_eq!(parse_major("java version \"1.8.0_202\""), Some(8));
        assert_eq!(parse_major("openjdk version \"21\" 2023-09-19"), Some(21));
        assert_eq!(parse_major("openjdk version \"11.0.21+9\""), Some(11));
    }

    #[test]
    fn junk_output_is_not_a_version() {
        assert_eq!(parse_major(""), None);
        assert_eq!(parse_major("command not found"), None);
        assert_eq!(parse_major("version \"abc\""), None);
    }

    #[test]
    fn an_empty_root_finds_nothing_managed() {
        let tmp = tempfile::tempdir().unwrap();
        let java = Java::new(tmp.path().join("java")).unwrap();
        assert_eq!(java.managed(17), None);
    }

    #[test]
    fn a_managed_runtime_is_found_inside_its_versioned_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let java = Java::new(tmp.path().join("java")).unwrap();

        // Mirrors Adoptium's layout: one top-level directory whose name carries
        // the build number.
        let bin = tmp.path().join("java/17/jdk-17.0.9+9-jre/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("java.exe"), b"").unwrap();

        assert_eq!(java.managed(17), Some(bin.join("java.exe")));
        assert_eq!(java.managed(21), None);
    }
}
