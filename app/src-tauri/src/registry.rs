//! On-disk server registry.
//!
//! One folder per server under `servers/`. The folder name is the `ServerId`
//! and never changes; everything mutable lives in files inside it:
//!
//! ```text
//! servers/<id>/manager.json        our settings (display name, memory, java)
//! servers/<id>/server.properties   Minecraft's, round-tripped key-by-key
//! servers/<id>/eula.txt            Mojang's
//! ```
//!
//! There is no index file. The filesystem *is* the index — a user who copies a
//! server folder in gets a working server, and a crash can never desync a
//! manifest from what is actually on disk.

use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{
    is_installed, CoreError, CoreResult, Difficulty, Gamemode, ModFile, RawProperties,
    ServerConfig, ServerFile, ServerId, ServerProperties, ServerState, ServerSummary,
};

const MANAGER_FILE: &str = "manager.json";
const PROPERTIES_FILE: &str = "server.properties";
const EULA_FILE: &str = "eula.txt";
const MODS_DIR: &str = "mods";

pub struct Registry {
    root: PathBuf,
}

impl Registry {
    /// `root` is the `servers/` directory; created if absent.
    pub fn new(root: impl Into<PathBuf>) -> CoreResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Where *our* metadata lives. Always inside the managed folder, even for
    /// an imported server, so the app never writes `manager.json` into a
    /// directory the user considers theirs.
    pub fn meta_dir(&self, id: &ServerId) -> PathBuf {
        self.root.join(&id.0)
    }

    /// Where the *server* lives — the imported path when there is one.
    ///
    /// Everything that touches Minecraft's own files (`server.properties`,
    /// `eula.txt`, the launch script, the world) goes through here.
    pub fn dir_of(&self, id: &ServerId) -> PathBuf {
        self.load_config(id)
            .ok()
            .and_then(|c| c.external_path)
            .unwrap_or_else(|| self.meta_dir(id))
    }

    /// Every server folder, sorted by display name.
    ///
    /// A folder whose `manager.json` is missing or corrupt is still listed,
    /// using defaults — a bad config file must not make a server disappear from
    /// the UI, because then the user has no way to fix it.
    pub fn list(&self) -> CoreResult<Vec<ServerSummary>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let id = ServerId(name);
            let config = self.load_config(&id).unwrap_or_else(|_| ServerConfig {
                name: id.0.clone(),
                ..Default::default()
            });
            // Absent until the server has run once; the card shows a dash for
            // these rather than inventing Minecraft's defaults.
            let raw = self.read_raw_properties(&id).unwrap_or_default();
            out.push(ServerSummary {
                light: ServerState::Stopped.light(),
                state: ServerState::Stopped,
                name: config.name,
                mc_version: config.mc_version,
                forge_version: config.forge_version,
                memory_mb: config.memory_mb,
                port: raw.get("server-port").and_then(|v| v.parse().ok()),
                max_players: raw.get("max-players").and_then(|v| v.parse().ok()),
                imported: config.external_path.is_some(),
                // Filled in by the caller, which is the only thing that can see
                // the supervisor.
                players_online: None,
                uptime_secs: None,
                id,
            });
        }
        out.sort_by_key(|s| s.name.to_lowercase());
        Ok(out)
    }

    /// Create a folder for `name`, de-duplicating the slug if it is taken.
    pub fn create(&self, name: &str) -> CoreResult<ServerId> {
        let id = self.free_id(&ServerId::from_name(name));
        fs::create_dir_all(self.meta_dir(&id))?;
        self.save_config(
            &id,
            &ServerConfig {
                name: name.trim().to_owned(),
                ..Default::default()
            },
        )?;
        Ok(id)
    }

    /// Adopt an existing installation without moving it.
    ///
    /// Only a `manager.json` stub is created; `source` is left untouched. The
    /// display name defaults to the folder's own name, which is what the user
    /// already calls it.
    pub fn import(&self, source: &Path) -> CoreResult<ServerId> {
        if !source.is_dir() {
            return Err(CoreError::Precondition {
                message: "選擇的路徑不是資料夾。".into(),
            });
        }
        if !is_installed(source) {
            // Refusing here beats importing something that cannot start: the
            // failure would otherwise surface much later, at the start button.
            return Err(CoreError::Precondition {
                message: "資料夾裡找不到 run.bat 或 server.jar，看起來還沒安裝 Forge。".into(),
            });
        }

        let canonical = source
            .canonicalize()
            .unwrap_or_else(|_| source.to_path_buf());
        for existing in self.list()? {
            if self
                .load_config(&existing.id)
                .ok()
                .and_then(|c| c.external_path)
                .is_some_and(|p| p == canonical)
            {
                return Err(CoreError::Precondition {
                    message: format!("這個資料夾已經匯入為「{}」。", existing.name),
                });
            }
        }

        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("imported server");
        // Read off disk rather than left blank: the version decides which Java
        // the server needs, and a folder with no recorded version cannot be
        // offered the right runtime when the launch fails.
        let (mc_version, forge_version) = match crate::forge::version_from_install(&canonical) {
            Some(version) => {
                let (mc, build) = crate::forge::split_version(&version);
                (Some(mc), Some(build))
            }
            None => (None, None),
        };

        let id = self.free_id(&ServerId::from_name(name));
        fs::create_dir_all(self.meta_dir(&id))?;
        self.save_config(
            &id,
            &ServerConfig {
                name: name.to_owned(),
                external_path: Some(canonical),
                mc_version,
                forge_version,
                ..Default::default()
            },
        )?;
        Ok(id)
    }

    /// Remove a server.
    ///
    /// An imported server is only unlinked — its folder belongs to the user and
    /// is left exactly as it was. Only servers this app created are deleted
    /// outright. Callers must stop the process first either way; removing the
    /// working directory of a live JVM corrupts the world save.
    pub fn delete(&self, id: &ServerId) -> CoreResult<()> {
        let meta = self.meta_dir(id);
        if meta.exists() {
            fs::remove_dir_all(meta)?;
        }
        Ok(())
    }

    fn free_id(&self, base: &ServerId) -> ServerId {
        let mut id = base.clone();
        let mut n = 2;
        while self.meta_dir(&id).exists() {
            id = ServerId(format!("{}-{n}", base.0));
            n += 1;
        }
        id
    }

    // ── manager.json ────────────────────────────────────────

    /// Reads from `meta_dir`, never `dir_of` — `dir_of` resolves the external
    /// path *out of* this file, so going through it here would recurse forever.
    pub fn load_config(&self, id: &ServerId) -> CoreResult<ServerConfig> {
        let path = self.meta_dir(id).join(MANAGER_FILE);
        let text = fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(|e| CoreError::Config {
            message: format!("{}: {e}", path.display()),
        })
    }

    pub fn save_config(&self, id: &ServerId, config: &ServerConfig) -> CoreResult<()> {
        let dir = self.meta_dir(id);
        fs::create_dir_all(&dir)?;
        let text = serde_json::to_string_pretty(config).map_err(|e| CoreError::Config {
            message: e.to_string(),
        })?;
        write_atomic(&dir.join(MANAGER_FILE), text.as_bytes())
    }

    // ── server.properties ───────────────────────────────────

    /// Parse `server.properties` into an ordered map. Comments and blank lines
    /// are dropped here but preserved on write, which reads the file again.
    pub fn read_raw_properties(&self, id: &ServerId) -> CoreResult<RawProperties> {
        let path = self.dir_of(id).join(PROPERTIES_FILE);
        if !path.exists() {
            return Ok(RawProperties::new());
        }
        let text = fs::read_to_string(&path)?;
        Ok(parse_properties(&text))
    }

    /// The typed subset the settings modal edits. Missing or unparseable keys
    /// fall back to Minecraft's own defaults rather than failing the whole read.
    pub fn read_properties(&self, id: &ServerId) -> CoreResult<ServerProperties> {
        let raw = self.read_raw_properties(id)?;
        let d = ServerProperties::default();
        Ok(ServerProperties {
            motd: raw.get("motd").cloned().unwrap_or(d.motd),
            port: parse_or(&raw, "server-port", d.port),
            max_players: parse_or(&raw, "max-players", d.max_players),
            gamemode: raw
                .get("gamemode")
                .and_then(|v| parse_gamemode(v))
                .unwrap_or(d.gamemode),
            difficulty: raw
                .get("difficulty")
                .and_then(|v| parse_difficulty(v))
                .unwrap_or(d.difficulty),
            pvp: parse_or(&raw, "pvp", d.pvp),
            online_mode: parse_or(&raw, "online-mode", d.online_mode),
            view_distance: parse_or(&raw, "view-distance", d.view_distance),
            white_list: parse_or(&raw, "white-list", d.white_list),
        })
    }

    /// Merge the typed subset back in, touching only those keys.
    ///
    /// Every other line — comments, the timestamp header, the ~50 keys we do
    /// not model — is copied through byte-for-byte. Rewriting the file from our
    /// struct would silently reset settings the user edited by hand.
    pub fn write_properties(&self, id: &ServerId, props: &ServerProperties) -> CoreResult<()> {
        let dir = self.dir_of(id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(PROPERTIES_FILE);
        let existing = if path.exists() {
            fs::read_to_string(&path)?
        } else {
            String::new()
        };

        let updates: Vec<(&str, String)> = vec![
            ("motd", props.motd.clone()),
            ("server-port", props.port.to_string()),
            ("max-players", props.max_players.to_string()),
            ("gamemode", gamemode_str(props.gamemode).into()),
            ("difficulty", difficulty_str(props.difficulty).into()),
            ("pvp", props.pvp.to_string()),
            ("online-mode", props.online_mode.to_string()),
            ("view-distance", props.view_distance.to_string()),
            ("white-list", props.white_list.to_string()),
        ];

        write_atomic(&path, merge_properties(&existing, &updates).as_bytes())
    }

    // ── raw file editing ────────────────────────────────────

    /// Read one of the two editable files verbatim. A file that does not exist
    /// yet reads as empty rather than failing — a server that has never run has
    /// no `server.properties`, and the editor should still open.
    pub fn read_file(&self, id: &ServerId, which: ServerFile) -> CoreResult<String> {
        let path = self.dir_of(id).join(which.filename());
        match fs::read_to_string(&path) {
            Ok(text) => Ok(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Write it back verbatim.
    ///
    /// Editing `user_jvm_args.txt` also parses `-Xmx` back into `memory_mb`.
    /// Without that, the next launch would rewrite the heap flags from the
    /// stored config and silently undo what the user just typed.
    pub fn write_file(&self, id: &ServerId, which: ServerFile, text: &str) -> CoreResult<()> {
        let dir = self.dir_of(id);
        fs::create_dir_all(&dir)?;
        write_atomic(&dir.join(which.filename()), text.as_bytes())?;

        if which == ServerFile::JvmArgs {
            if let Some(mb) = parse_xmx_mb(text) {
                let config = self.load_config(id)?;
                if config.memory_mb != mb {
                    self.save_config(
                        id,
                        &ServerConfig {
                            memory_mb: mb,
                            ..config
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    // ── mods/ ───────────────────────────────────────────────

    /// Jars in the server's `mods/` folder, alphabetical.
    ///
    /// A folder that has never had Forge run in it has no `mods/` yet; that
    /// reads as empty rather than failing, so the panel can offer to add one.
    pub fn list_mods(&self, id: &ServerId) -> CoreResult<Vec<ModFile>> {
        let dir = self.dir_of(id).join(MODS_DIR);
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(Vec::new());
        };
        let mut out: Vec<ModFile> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_owned();
                if !name.to_lowercase().ends_with(".jar") {
                    return None;
                }
                Some(ModFile {
                    bytes: e.metadata().ok()?.len(),
                    name,
                })
            })
            .collect();
        out.sort_by_key(|m| m.name.to_lowercase());
        Ok(out)
    }

    /// Copy a jar in, overwriting a file of the same name.
    ///
    /// Overwriting is what updating a mod looks like from the user's side —
    /// they pick the newer jar of the same name and expect it to replace the
    /// old one, not to sit beside it and get loaded twice.
    pub fn add_mod(&self, id: &ServerId, source: &Path) -> CoreResult<()> {
        if source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            != Some("jar".into())
        {
            return Err(CoreError::Precondition {
                message: "模組必須是 .jar 檔案。".into(),
            });
        }
        let name =
            source
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| CoreError::Precondition {
                    message: "檔名無法辨識。".into(),
                })?;

        let dir = self.dir_of(id).join(MODS_DIR);
        fs::create_dir_all(&dir)?;
        fs::copy(source, dir.join(name))?;
        Ok(())
    }

    pub fn delete_mod(&self, id: &ServerId, name: &str) -> CoreResult<()> {
        let dir = self.dir_of(id).join(MODS_DIR);
        fs::remove_file(dir.join(safe_mod_name(name)?))?;
        Ok(())
    }

    // ── eula.txt ────────────────────────────────────────────

    pub fn eula_accepted(&self, id: &ServerId) -> bool {
        let path = self.dir_of(id).join(EULA_FILE);
        fs::read_to_string(path)
            .map(|t| t.to_lowercase().contains("eula=true"))
            .unwrap_or(false)
    }

    pub fn set_eula(&self, id: &ServerId, accepted: bool) -> CoreResult<()> {
        let dir = self.dir_of(id);
        fs::create_dir_all(&dir)?;
        let body = format!(
            "# By changing the setting below to TRUE you are indicating your agreement to the EULA.\n\
             eula={}\n",
            accepted
        );
        write_atomic(&dir.join(EULA_FILE), body.as_bytes())
    }
}

// ─────────────────────────────────────────────────────────────
// helpers
// ─────────────────────────────────────────────────────────────

/// Write via a sibling `.tmp` then rename. A half-written `manager.json` from a
/// crash mid-save would make the server unreadable on next launch; rename is
/// atomic on the same volume, so the file is either the old one or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    // Windows rename fails onto an existing file; remove first.
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn parse_properties(text: &str) -> RawProperties {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// Replace the value of each key that already has a line; append the rest.
fn merge_properties(existing: &str, updates: &[(&str, String)]) -> String {
    let mut written: Vec<&str> = Vec::new();
    let mut lines: Vec<String> = Vec::new();

    for line in existing.lines() {
        let replacement = (!line.trim_start().starts_with('#'))
            .then(|| line.split_once('='))
            .flatten()
            .and_then(|(k, _)| {
                let key = k.trim();
                updates.iter().find(|(uk, _)| *uk == key)
            });
        match replacement {
            Some((key, value)) => {
                lines.push(format!("{key}={value}"));
                written.push(key);
            }
            None => lines.push(line.to_owned()),
        }
    }

    for (key, value) in updates {
        if !written.contains(key) {
            lines.push(format!("{key}={value}"));
        }
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Heap size out of a `user_jvm_args.txt` body, in MB. `None` when the file
/// names no `-Xmx` at all, which leaves the stored config as the authority.
/// A mod file name that can only ever name a file inside `mods/`.
///
/// The name crosses the IPC boundary, so it is checked here rather than
/// trusted: a `..` or a separator in it would otherwise reach any file on disk
/// through `remove_file`.
fn safe_mod_name(name: &str) -> CoreResult<&str> {
    let rejected = name.is_empty()
        || name.contains(['/', '\\', ':'])
        || name.contains("..")
        || !name.to_lowercase().ends_with(".jar");
    if rejected {
        return Err(CoreError::Precondition {
            message: "模組檔名無效。".into(),
        });
    }
    Ok(name)
}

fn parse_xmx_mb(text: &str) -> Option<u32> {
    text.lines()
        // Forge ships this file with its defaults commented out; a commented
        // -Xmx is documentation, not the value in force.
        .map(|line| line.split('#').next().unwrap_or(""))
        .flat_map(str::split_whitespace)
        .find_map(|t| {
            let v = t.strip_prefix("-Xmx")?;
            let (digits, scale) = match v.chars().last()? {
                'g' | 'G' => (&v[..v.len() - 1], 1024),
                'm' | 'M' => (&v[..v.len() - 1], 1),
                _ => (v, 1),
            };
            Some(digits.parse::<u32>().ok()?.saturating_mul(scale))
        })
}

fn parse_or<T: std::str::FromStr>(raw: &RawProperties, key: &str, fallback: T) -> T {
    raw.get(key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

fn parse_gamemode(v: &str) -> Option<Gamemode> {
    Some(match v.trim().to_lowercase().as_str() {
        "survival" | "0" => Gamemode::Survival,
        "creative" | "1" => Gamemode::Creative,
        "adventure" | "2" => Gamemode::Adventure,
        "spectator" | "3" => Gamemode::Spectator,
        _ => return None,
    })
}

fn parse_difficulty(v: &str) -> Option<Difficulty> {
    Some(match v.trim().to_lowercase().as_str() {
        "peaceful" | "0" => Difficulty::Peaceful,
        "easy" | "1" => Difficulty::Easy,
        "normal" | "2" => Difficulty::Normal,
        "hard" | "3" => Difficulty::Hard,
        _ => return None,
    })
}

fn gamemode_str(g: Gamemode) -> &'static str {
    match g {
        Gamemode::Survival => "survival",
        Gamemode::Creative => "creative",
        Gamemode::Adventure => "adventure",
        Gamemode::Spectator => "spectator",
    }
}

fn difficulty_str(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Peaceful => "peaceful",
        Difficulty::Easy => "easy",
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_registry() -> (tempfile::TempDir, Registry) {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registry::new(dir.path().join("servers")).unwrap();
        (dir, reg)
    }

    #[test]
    fn create_then_list_round_trips_the_display_name() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("My Server").unwrap();
        assert_eq!(id.0, "my-server");

        let listed = reg.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "My Server");
        assert_eq!(listed[0].id, id);
    }

    #[test]
    fn editing_jvm_args_updates_the_stored_heap_size() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("Server").unwrap();

        reg.write_file(
            &id,
            ServerFile::JvmArgs,
            "# comment -Xmx99M\n-XX:+UseG1GC\n-Xms1024M\n-Xmx6G\n",
        )
        .unwrap();

        // Otherwise the next launch would rewrite -Xmx from the old config and
        // undo the edit the user just made.
        assert_eq!(reg.load_config(&id).unwrap().memory_mb, 6144);
        assert!(reg
            .read_file(&id, ServerFile::JvmArgs)
            .unwrap()
            .contains("-XX:+UseG1GC"));
    }

    #[test]
    fn mods_round_trip_and_a_crafted_name_cannot_escape_the_folder() {
        let (tmp, reg) = temp_registry();
        let id = reg.create("Server").unwrap();
        assert!(reg.list_mods(&id).unwrap().is_empty(), "no mods/ yet");

        let jar = tmp.path().join("JEI-1.20.1.jar");
        std::fs::write(&jar, b"pretend jar").unwrap();
        reg.add_mod(&id, &jar).unwrap();

        let listed = reg.list_mods(&id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "JEI-1.20.1.jar");
        assert_eq!(listed[0].bytes, 11);

        // Anything that is not a jar, and any name that could point outside
        // mods/, is refused rather than resolved.
        let txt = tmp.path().join("notes.txt");
        std::fs::write(&txt, b"").unwrap();
        assert!(reg.add_mod(&id, &txt).is_err());
        for crafted in [
            "../../manager.json",
            r"..\eula.txt",
            "a/b.jar",
            "x.jar/../y",
        ] {
            assert!(
                reg.delete_mod(&id, crafted).is_err(),
                "must refuse {crafted}"
            );
        }

        reg.delete_mod(&id, "JEI-1.20.1.jar").unwrap();
        assert!(reg.list_mods(&id).unwrap().is_empty());
    }

    #[test]
    fn a_file_that_does_not_exist_reads_as_empty() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("Server").unwrap();
        assert_eq!(reg.read_file(&id, ServerFile::Properties).unwrap(), "");
    }

    #[test]
    fn duplicate_names_get_distinct_folders() {
        let (_tmp, reg) = temp_registry();
        assert_eq!(reg.create("Server").unwrap().0, "server");
        assert_eq!(reg.create("Server").unwrap().0, "server-2");
        assert_eq!(reg.create("Server").unwrap().0, "server-3");
    }

    #[test]
    fn rename_does_not_move_the_folder() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("Old Name").unwrap();
        let mut config = reg.load_config(&id).unwrap();
        config.name = "New Name".into();
        reg.save_config(&id, &config).unwrap();

        assert_eq!(reg.load_config(&id).unwrap().name, "New Name");
        assert!(reg.dir_of(&id).exists(), "folder id must be stable");
    }

    #[test]
    fn a_corrupt_config_still_lists_the_server() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("Broken").unwrap();
        fs::write(reg.dir_of(&id).join(MANAGER_FILE), "{ not json").unwrap();

        let listed = reg.list().unwrap();
        assert_eq!(listed.len(), 1, "must not vanish from the UI");
        assert_eq!(listed[0].name, "broken");
    }

    #[test]
    fn writing_properties_preserves_unmodelled_keys_and_comments() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("S").unwrap();
        fs::write(
            reg.dir_of(&id).join(PROPERTIES_FILE),
            "#Minecraft server properties\n\
             spawn-protection=16\n\
             motd=old motd\n\
             enable-rcon=false\n",
        )
        .unwrap();

        let mut props = reg.read_properties(&id).unwrap();
        assert_eq!(props.motd, "old motd");
        props.motd = "new motd".into();
        reg.write_properties(&id, &props).unwrap();

        let text = fs::read_to_string(reg.dir_of(&id).join(PROPERTIES_FILE)).unwrap();
        assert!(text.contains("#Minecraft server properties"));
        assert!(text.contains("spawn-protection=16"));
        assert!(text.contains("enable-rcon=false"));
        assert!(text.contains("motd=new motd"));
        assert!(!text.contains("old motd"));
    }

    #[test]
    fn numeric_gamemode_and_difficulty_are_understood() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("S").unwrap();
        fs::write(
            reg.dir_of(&id).join(PROPERTIES_FILE),
            "gamemode=1\ndifficulty=3\n",
        )
        .unwrap();

        let props = reg.read_properties(&id).unwrap();
        assert_eq!(props.gamemode, Gamemode::Creative);
        assert_eq!(props.difficulty, Difficulty::Hard);
    }

    #[test]
    fn eula_round_trips() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("S").unwrap();
        assert!(!reg.eula_accepted(&id));
        reg.set_eula(&id, true).unwrap();
        assert!(reg.eula_accepted(&id));
        reg.set_eula(&id, false).unwrap();
        assert!(!reg.eula_accepted(&id));
    }

    #[test]
    fn delete_removes_the_folder() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("Doomed").unwrap();
        reg.delete(&id).unwrap();
        assert!(reg.list().unwrap().is_empty());
        reg.delete(&id).unwrap(); // idempotent
    }

    // ── import ──────────────────────────────────────────────

    /// A folder that looks like an installed server, somewhere outside the
    /// managed directory.
    fn existing_install(parent: &Path, name: &str) -> PathBuf {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("run.bat"), b"@echo off").unwrap();
        fs::write(
            dir.join("server.properties"),
            "motd=imported\nserver-port=25599\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn import_registers_the_path_without_copying_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::new(tmp.path().join("servers")).unwrap();
        let source = existing_install(tmp.path(), "atm9");

        let id = reg.import(&source).unwrap();

        assert_eq!(reg.load_config(&id).unwrap().name, "atm9");
        assert_eq!(
            reg.dir_of(&id).canonicalize().unwrap(),
            source.canonicalize().unwrap(),
            "server files must be read from where they already are"
        );
        assert!(
            !reg.meta_dir(&id).join("run.bat").exists(),
            "importing must not copy the installation"
        );
        // Minecraft's own files are read through the external path.
        assert_eq!(reg.read_properties(&id).unwrap().port, 25599);
    }

    #[test]
    fn import_rejects_a_folder_with_no_server_in_it() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::new(tmp.path().join("servers")).unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        let err = reg.import(&empty).unwrap_err();
        assert!(
            matches!(err, CoreError::Precondition { .. }),
            "a folder that could never start must be refused at import time"
        );
        assert!(reg.list().unwrap().is_empty());
    }

    #[test]
    fn import_rejects_a_file_and_a_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::new(tmp.path().join("servers")).unwrap();
        let file = tmp.path().join("not-a-dir.txt");
        fs::write(&file, b"x").unwrap();

        assert!(reg.import(&file).is_err());
        assert!(reg.import(&tmp.path().join("nope")).is_err());
    }

    #[test]
    fn importing_the_same_folder_twice_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::new(tmp.path().join("servers")).unwrap();
        let source = existing_install(tmp.path(), "atm9");

        reg.import(&source).unwrap();
        let err = reg.import(&source).unwrap_err();
        assert!(matches!(err, CoreError::Precondition { .. }));
        assert_eq!(reg.list().unwrap().len(), 1, "no duplicate entry");
    }

    #[test]
    fn deleting_an_imported_server_leaves_the_users_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::new(tmp.path().join("servers")).unwrap();
        let source = existing_install(tmp.path(), "atm9");

        let id = reg.import(&source).unwrap();
        reg.delete(&id).unwrap();

        assert!(reg.list().unwrap().is_empty(), "unlinked from the app");
        assert!(
            source.join("run.bat").exists(),
            "an imported folder belongs to the user; delete must only unlink"
        );
    }

    #[test]
    fn deleting_an_app_created_server_does_remove_its_files() {
        let (_tmp, reg) = temp_registry();
        let id = reg.create("Mine").unwrap();
        fs::write(reg.dir_of(&id).join("server.jar"), b"x").unwrap();
        let dir = reg.dir_of(&id);

        reg.delete(&id).unwrap();
        assert!(!dir.exists());
    }
}
