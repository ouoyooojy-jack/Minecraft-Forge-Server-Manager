//! The contract between the Rust core and the UI.
//!
//! Everything the UI can see or send lives in this file. Mirrored by hand in
//! `src/lib/types.ts` — change both together.
//!
//! ponytail: hand-mirrored TS types. Swap in `tauri-specta` codegen if the two
//! files start drifting; not worth an rc-version dependency yet.

// This module is the contract, so it necessarily describes more than today's
// callers use — the download, java, and supervisor halves have no consumer
// until those modules land. Remove this once they do; it must not outlive them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────
// Identity
// ─────────────────────────────────────────────────────────────

/// Stable server identity. This is the folder name under `servers/`, slugified
/// at creation and never changed afterwards — renaming a server only edits its
/// display `name`, so a rename can never race a running process or invalidate
/// an open modal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ServerId(pub String);

impl ServerId {
    /// Slugify a display name into a filesystem-safe id.
    pub fn from_name(name: &str) -> Self {
        let slug: String = name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let slug = slug.trim_matches('-').replace("--", "-");
        ServerId(if slug.is_empty() {
            "server".into()
        } else {
            slug
        })
    }
}

impl std::fmt::Display for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifies one in-flight download so progress events can be routed to the
/// right progress bar. Downloads outlive the modal that started them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DownloadId(pub u64);

// ─────────────────────────────────────────────────────────────
// Server lifecycle
// ─────────────────────────────────────────────────────────────

/// Where a server is in its lifecycle.
///
/// `Online` is only entered after the server log emits its "Done (…)!" line —
/// process-is-alive is not the same as server-is-accepting-players, and the
/// home screen's status light must mean the latter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerState {
    /// No process. The resting state.
    Stopped,
    /// Forge installer is running (`java -jar … --installServer`).
    Installing,
    /// Process spawned, still booting.
    Starting,
    /// Log emitted "Done"; accepting connections.
    Online,
    /// `stop` sent, waiting for the process to exit.
    Stopping,
    /// Process exited without being asked to.
    Crashed { code: Option<i32> },
}

impl ServerState {
    /// Which of the four status-light colours the home screen should draw.
    pub fn light(&self) -> StatusLight {
        match self {
            ServerState::Stopped => StatusLight::Offline,
            ServerState::Installing | ServerState::Starting | ServerState::Stopping => {
                StatusLight::Starting
            }
            ServerState::Online => StatusLight::Online,
            ServerState::Crashed { .. } => StatusLight::Error,
        }
    }
}

/// The four colours in the home screen legend. Derived from `ServerState` so
/// the UI never has to know the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusLight {
    Online,
    Offline,
    Starting,
    Error,
}

/// One home-screen card.
///
/// Carries everything a card draws, so rendering the grid is one call rather
/// than one call plus a `server.properties` read per server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSummary {
    pub id: ServerId,
    pub name: String,
    pub state: ServerState,
    pub light: StatusLight,
    /// `None` until Forge has been installed into the folder.
    pub mc_version: Option<String>,
    pub forge_version: Option<String>,
    pub memory_mb: u32,
    /// `None` when the server has never run, so `server.properties` does not
    /// exist yet. The card shows a dash rather than Minecraft's default, which
    /// would be a value the user has not actually got.
    pub port: Option<u16>,
    pub max_players: Option<u32>,
    /// An installation adopted from elsewhere on disk rather than created here.
    pub imported: bool,
    /// Connected players and seconds of uptime — `None` unless the server is
    /// running. Both come from the live process, not from disk.
    pub players_online: Option<u32>,
    pub uptime_secs: Option<u64>,
}

// ─────────────────────────────────────────────────────────────
// Per-server configuration
// ─────────────────────────────────────────────────────────────

/// Manager-owned settings, persisted to `servers/<id>/manager.json`.
///
/// Kept separate from `server.properties`: these are ours to define, those
/// belong to Minecraft and must survive round-tripping untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    /// Display name. Editable; the folder name (`ServerId`) never changes.
    pub name: String,
    pub memory_mb: u32,
    /// `None` means "use the JRE this app downloaded for the required major".
    /// `Some` pins a user-chosen java executable.
    pub java_path: Option<PathBuf>,
    pub jvm_args: Vec<String>,
    pub mc_version: Option<String>,
    pub forge_version: Option<String>,
    /// Set when the server lives outside the managed folder — an existing
    /// installation the user imported.
    ///
    /// Importing registers a path rather than copying: a modpack is gigabytes,
    /// and duplicating it to adopt it would be both slow and a second copy of
    /// the world to keep track of. The world files stay exactly where the user
    /// put them, and deleting the server here only unlinks it.
    #[serde(default)]
    pub external_path: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "New Server".into(),
            memory_mb: 2048,
            java_path: None,
            jvm_args: Vec::new(),
            mc_version: None,
            forge_version: None,
            external_path: None,
        }
    }
}

/// Whether a folder holds something this app can actually launch.
///
/// The same rule gates importing and starting, so it lives in one place: a
/// folder that imports cleanly but cannot start would be a worse outcome than
/// refusing the import.
pub fn is_installed(dir: &std::path::Path) -> bool {
    dir.join("run.bat").is_file()
        || dir.join("run.sh").is_file()
        || dir.join("server.jar").is_file()
}

/// The files the raw text editor is allowed to open.
///
/// An enum rather than a path: the UI names *which* file, and the core decides
/// where it lives, so no request from the front end can read or write anything
/// outside the server's own folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerFile {
    Properties,
    JvmArgs,
    /// Forge's launch script. Not executed by this app — it is read for the
    /// argument-file paths inside it — but editing it is still the way people
    /// tweak a server, so it is editable here and remotely.
    RunScript,
}

impl ServerFile {
    pub fn filename(self) -> &'static str {
        match self {
            ServerFile::Properties => "server.properties",
            ServerFile::JvmArgs => "user_jvm_args.txt",
            ServerFile::RunScript => "run.bat",
        }
    }
}

/// A machine reachable over SSH — a friend's PC on the Hamachi network, in
/// practice — whose server files this app can edit.
///
/// Holds no credentials. Authentication is the system `ssh` client's business:
/// it uses the keys and `known_hosts` already on this machine, which is also
/// why this struct can be written to disk as plain JSON without care.
/// How this app reaches a remote machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    /// The other machine runs this app with remote access switched on. Nothing
    /// to install on their side, and a pairing code instead of a key.
    #[default]
    App,
    /// The system `ssh` client. For a machine that does not run this app, or
    /// one where you already have key access.
    Ssh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteHost {
    /// Stable id, assigned on first save.
    pub id: String,
    /// What to call it in the list — "小明的電腦", not an IP.
    pub label: String,
    /// Hostname or address. A Hamachi address looks like `25.x.x.x`.
    pub host: String,
    #[serde(default)]
    pub transport: Transport,
    /// The pairing code shown by the other machine's app. `App` transport only.
    #[serde(default)]
    pub token: Option<String>,
    /// `Ssh` transport only.
    #[serde(default)]
    pub user: String,
    pub port: u16,
    /// Private key to use. `None` lets ssh pick from its own defaults.
    pub key_path: Option<PathBuf>,
    /// The server folder on that machine, e.g. `C:/servers/smp`. `Ssh`
    /// transport only — over `App` the other side names its own servers.
    #[serde(default)]
    pub dir: String,
}

/// The subset of `server.properties` the settings modal exposes.
///
/// Reads pull these keys out of the file; writes merge them back in place,
/// leaving the ~60 keys we don't model untouched. Never rewrite the whole file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProperties {
    pub motd: String,
    pub port: u16,
    pub max_players: u32,
    pub gamemode: Gamemode,
    pub difficulty: Difficulty,
    pub pvp: bool,
    /// `online-mode`. Off means cracked clients can join.
    pub online_mode: bool,
    pub view_distance: u32,
    pub white_list: bool,
}

impl Default for ServerProperties {
    fn default() -> Self {
        Self {
            motd: "A Minecraft Server".into(),
            port: 25565,
            max_players: 20,
            gamemode: Gamemode::Survival,
            difficulty: Difficulty::Easy,
            pvp: true,
            online_mode: true,
            view_distance: 10,
            white_list: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gamemode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// Raw `server.properties` contents, key order preserved for round-tripping.
pub type RawProperties = BTreeMap<String, String>;

// ─────────────────────────────────────────────────────────────
// Console
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogStream {
    Stdout,
    Stderr,
    /// Emitted by the manager itself ("starting…", "installer exited 1"), not
    /// by the server process. Rendered in a distinct colour.
    System,
}

/// One console line.
///
/// `seq` is monotonic per server, never reused, and starts at 1. The UI
/// compares the first `seq` it receives against the last it holds: a gap means
/// the ring buffer evicted lines while the console modal was closed, and the UI
/// can say so instead of silently splicing unrelated output together. Starting
/// at 1 leaves 0 free to mean "send me everything you still have".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub seq: u64,
    pub stream: LogStream,
    pub text: String,
}

// ─────────────────────────────────────────────────────────────
// Downloads
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadKind {
    ForgeInstaller { version: String },
    Jre { major: u8 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadState {
    Running,
    Done { path: PathBuf },
    Cancelled,
    Failed { message: String },
}

/// Progress for one download. Emitted on a throttle (see `PROGRESS_HZ`), not
/// per chunk — a 100 MB installer at 64 KB chunks is ~1600 events otherwise.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: DownloadId,
    pub what: DownloadKind,
    pub received: u64,
    /// `None` when the server sends no `Content-Length`.
    pub total: Option<u64>,
    pub bytes_per_sec: f64,
    pub state: DownloadState,
}

// ─────────────────────────────────────────────────────────────
// Forge versions
// ─────────────────────────────────────────────────────────────

/// Forge builds for one Minecraft line, e.g. all the `1.20.*` builds.
///
/// Grouped rather than flat because the raw list runs to thousands of entries —
/// the download page's two dropdowns pick a line first, then a build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeVersionGroup {
    /// Minecraft major line, e.g. `"1.20"`.
    pub mc_major: String,
    /// Full Forge versions in this line, newest first, e.g. `"1.20.1-47.2.0"`.
    pub versions: Vec<String>,
}

/// An installer jar sitting in the downloads folder.
///
/// Carries its size because the only reason to list these is to decide which
/// ones to delete, and that decision is about disk space.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
}

/// One jar in a server's `mods/` folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    /// File name, which is also the handle for deleting it — the core resolves
    /// it against the server's own `mods/` and nothing else.
    pub name: String,
    pub bytes: u64,
}

// ─────────────────────────────────────────────────────────────
// Java runtimes
// ─────────────────────────────────────────────────────────────

/// Which Java major a given Minecraft version needs.
///
/// Two numbering schemes have to be read here. Everything up to 1.21.x is the
/// old `1.<minor>.<patch>` shape, where the leading 1 carries no information
/// and the floor moved with the minor: 1.20.5+ wants 21, 1.18–1.20.4 wants 17,
/// 1.17.x wants 16, older runs on 8.
///
/// From 26.1 Mojang switched to year-based numbers — 26.1, 26.1.2, 26.2 — and
/// raised the floor to Java 25. So the leading component stopped being a
/// constant and started being the version, which is exactly the assumption the
/// old reading baked in: `26.2` parsed as "minor 2" and came back as Java 8.
///
/// A number this app has never heard of is treated as the newest floor we know.
/// Versions only move forward, and guessing low means a server that refuses to
/// start with an error about a Java version the user was never offered.
pub fn required_java_major(mc_version: &str) -> u8 {
    let parts: Vec<u32> = mc_version
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    match parts.as_slice() {
        // The old scheme is the only one that starts with 1.
        [1, minor, rest @ ..] => {
            let patch = rest.first().copied().unwrap_or(0);
            match (*minor, patch) {
                (m, _) if m >= 21 => 21,
                (20, p) if p >= 5 => 21,
                (m, _) if m >= 18 => 17,
                (17, _) => 16,
                _ => 8,
            }
        }
        // 26.1 and everything after it.
        [_, ..] => 25,
        [] => 25,
    }
}

// ─────────────────────────────────────────────────────────────
// Events (core → UI)
// ─────────────────────────────────────────────────────────────

/// Everything the core pushes at the UI. One channel, one enum: the UI holds a
/// single subscription and switches on `type`, so a new event kind can never be
/// silently dropped by a listener nobody registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CoreEvent {
    /// A server changed lifecycle state. Drives the home screen status lights.
    ServerState {
        id: ServerId,
        state: ServerState,
        light: StatusLight,
    },
    /// New console output. Batched — see `LogLine::seq` for gap detection.
    ServerLog { id: ServerId, lines: Vec<LogLine> },
    /// A server was created, renamed, or deleted; the card grid should refetch.
    ServersChanged,
    /// Download progress, throttled.
    Download(DownloadProgress),
    /// Somebody connected to this machine's remote-access listener. Shown as a
    /// running list: a service that writes files is one the user should be able
    /// to watch without going looking for a log file.
    AgentActivity { line: String },
    /// Something failed outside a command's return path (a background install,
    /// a crashed reader task). Commands report their own errors as `Err`.
    Error {
        id: Option<ServerId>,
        error: CoreError,
    },
}

/// Where core modules push events. A plain callback rather than a trait: there
/// is one real implementation (Tauri's emitter) and the tests want a collector,
/// which does not justify defining an interface.
pub type EventSink = std::sync::Arc<dyn Fn(CoreEvent) + Send + Sync>;

/// Event channel name. The UI listens on exactly this one.
pub const EVENT_CHANNEL: &str = "core";

/// Progress and log batches are flushed at this rate. 30 Hz matches the frame
/// budget; anything faster is invisible and just burns IPC.
pub const PROGRESS_HZ: u32 = 30;

/// Applies to establishing a connection, not to the whole transfer — a large
/// installer on a slow line must be allowed to take as long as it takes.
pub const HTTP_TIMEOUT_SEC: u64 = 60;

/// Console lines held per server. Older lines are evicted; the UI detects the
/// gap via `LogLine::seq`. A busy modded server emits megabytes per hour, so
/// this must be bounded.
pub const LOG_BUFFER_LINES: usize = 5_000;

// ─────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────

/// Errors the UI is expected to *act* on, not just display.
///
/// The variants are split by recovery action, which is why `Network` and
/// `VersionNotFound` are separate: one means "retry", the other means "pick a
/// different version". A single opaque error string would collapse that.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CoreError {
    /// Transport failed. Retryable.
    #[error("network error: {message}")]
    Network { message: String },

    /// The server answered, but that version does not exist. Not retryable —
    /// the user must choose another.
    #[error("version not found: {version}")]
    VersionNotFound { version: String },

    /// User cancelled. Not a failure; the UI should not show it as one.
    #[error("download cancelled")]
    DownloadAborted,

    /// No usable JRE for the required major, and fetching one failed.
    #[error("java error: {message}")]
    Java { message: String },

    /// No Java on this machine that can run the version being launched.
    ///
    /// Separate from `Java` because the recovery is a button, not a message:
    /// the UI offers to download exactly this major.
    #[error("java {major} is required")]
    JavaMissing { major: u8 },

    /// The Forge installer ran but exited non-zero.
    #[error("forge install failed (exit {code:?}): {message}")]
    Install { code: Option<i32>, message: String },

    /// Spawning, writing to, or killing the server process failed.
    #[error("process error: {message}")]
    Process { message: String },

    /// A precondition the user can fix: EULA not accepted, no server.jar,
    /// server already running.
    #[error("{message}")]
    Precondition { message: String },

    #[error("io error: {message}")]
    Io { message: String },

    #[error("config error: {message}")]
    Config { message: String },
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io {
            message: e.to_string(),
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_major_matches_mojang_floors() {
        assert_eq!(required_java_major("1.12.2"), 8);
        assert_eq!(required_java_major("1.16.5"), 8);
        assert_eq!(required_java_major("1.17.1"), 16);
        assert_eq!(required_java_major("1.18.2"), 17);
        assert_eq!(required_java_major("1.20.1"), 17);
        assert_eq!(required_java_major("1.20.4"), 17);
        assert_eq!(required_java_major("1.20.6"), 21);
        assert_eq!(required_java_major("1.21.4"), 21);
        assert_eq!(required_java_major("1.21"), 21, "a two-part old version");
    }

    /// 26.1 moved to year-based numbers and to Java 25. Read with the old
    /// rules, "26.2" looks like minor 2 and comes back as Java 8 — a server
    /// that then refuses to start, blaming a Java version nobody offered.
    #[test]
    fn the_year_based_versions_need_java_25() {
        assert_eq!(required_java_major("26.1"), 25);
        assert_eq!(required_java_major("26.1.2"), 25);
        assert_eq!(required_java_major("26.2"), 25);
        // Nothing newer exists yet; the newest floor we know beats guessing low.
        assert_eq!(required_java_major("27.1"), 25);
        assert_eq!(required_java_major(""), 25);
    }

    #[test]
    fn state_maps_onto_the_four_lights() {
        assert_eq!(ServerState::Stopped.light(), StatusLight::Offline);
        assert_eq!(ServerState::Starting.light(), StatusLight::Starting);
        assert_eq!(ServerState::Installing.light(), StatusLight::Starting);
        assert_eq!(ServerState::Online.light(), StatusLight::Online);
        assert_eq!(
            ServerState::Crashed { code: Some(1) }.light(),
            StatusLight::Error
        );
    }

    #[test]
    fn ids_are_filesystem_safe() {
        assert_eq!(ServerId::from_name("My Server!").0, "my-server");
        assert_eq!(ServerId::from_name("  ").0, "server");
        assert_eq!(ServerId::from_name("生存伺服器").0, "生存伺服器");
    }

    // ── wire shape ──────────────────────────────────────
    //
    // `src/lib/types.ts` is a hand-written mirror of this file, so the exact
    // JSON matters. These pin the shapes the UI destructures; a serde attribute
    // changed without updating the mirror fails here instead of at runtime.

    #[test]
    fn download_event_flattens_progress_alongside_the_tag() {
        let json = serde_json::to_value(CoreEvent::Download(DownloadProgress {
            id: DownloadId(7),
            what: DownloadKind::ForgeInstaller {
                version: "1.20.1-47.2.0".into(),
            },
            received: 1024,
            total: Some(4096),
            bytes_per_sec: 512.0,
            state: DownloadState::Running,
        }))
        .unwrap();

        // The UI reads `event.id` and `event.state`, not `event.payload.id`.
        assert_eq!(json["type"], "download");
        assert_eq!(json["id"], 7);
        assert_eq!(json["received"], 1024);
        assert_eq!(json["bytesPerSec"], 512.0);
        assert_eq!(json["what"]["kind"], "forgeInstaller");
        assert_eq!(json["what"]["version"], "1.20.1-47.2.0");
        assert_eq!(json["state"]["kind"], "running");
    }

    #[test]
    fn a_missing_total_serialises_as_null_not_zero() {
        let json = serde_json::to_value(DownloadProgress {
            id: DownloadId(1),
            what: DownloadKind::Jre { major: 17 },
            received: 10,
            total: None,
            bytes_per_sec: 0.0,
            state: DownloadState::Done {
                path: "x.jar".into(),
            },
        })
        .unwrap();

        // The UI switches on null to draw an indeterminate bar; 0 would read as
        // a known-zero size and produce NaN%.
        assert!(json["total"].is_null());
        assert_eq!(json["state"]["kind"], "done");
        assert_eq!(json["what"]["major"], 17);
    }

    #[test]
    fn server_state_is_tagged_by_kind() {
        let json = serde_json::to_value(CoreEvent::ServerState {
            id: ServerId("s".into()),
            state: ServerState::Crashed { code: Some(1) },
            light: StatusLight::Error,
        })
        .unwrap();

        assert_eq!(json["type"], "serverState");
        assert_eq!(json["state"]["kind"], "crashed");
        assert_eq!(json["state"]["code"], 1);
        assert_eq!(json["light"], "error");
    }

    #[test]
    fn errors_carry_the_variant_the_ui_switches_on() {
        let json = serde_json::to_value(CoreError::VersionNotFound {
            version: "9.9.9".into(),
        })
        .unwrap();
        assert_eq!(json["kind"], "versionNotFound");
        assert_eq!(json["version"], "9.9.9");

        let json = serde_json::to_value(CoreError::DownloadAborted).unwrap();
        assert_eq!(json["kind"], "downloadAborted");
    }
}
