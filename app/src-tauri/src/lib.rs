//! Tauri wiring: application state and the IPC command surface.
//!
//! Rule for this file: no business logic. Commands validate, delegate to a core
//! module, and translate the result. Anything worth testing lives in a module
//! that does not import `tauri`, so it can be tested without a window.

mod agent;
mod download;
mod forge;
mod java;
mod logbuf;
mod registry;
mod remote;
mod supervisor;
#[cfg(test)]
mod testutil;
mod types;

use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use agent::{Agent, AgentSettings};
use download::Downloader;
use forge::Forge;
use java::Java;
use registry::Registry;
use remote::Remotes;
use supervisor::Supervisor;
use types::{
    CoreError, CoreEvent, CoreResult, DownloadId, DownloadKind, ForgeVersionGroup, LogLine,
    ServerConfig, ServerId, ServerProperties, ServerSummary, EVENT_CHANNEL,
};

pub struct AppState {
    registry: Arc<Registry>,
    /// Owns every running process and the console history that outlives it.
    supervisor: Supervisor,
    downloader: Downloader,
    forge: Forge,
    /// Locates a JRE, and unpacks one when the machine has none.
    java: Java,
    /// Other people's machines.
    remotes: Remotes,
    /// This machine's own listener, when the user has switched it on. `None`
    /// means nothing is bound and nothing is answering.
    running_agent: std::sync::Mutex<Option<Agent>>,
    /// Where `agent.json` and `remotes.json` live.
    data_dir: PathBuf,
    /// Where installer jars and JREs land. Shared across servers, so the same
    /// Forge version is fetched once no matter how many servers use it.
    downloads_dir: PathBuf,
}

/// Push an event to the UI. Failures are logged, never propagated: an event
/// that cannot be delivered (window closing) must not fail the operation that
/// produced it.
fn emit(app: &AppHandle, event: CoreEvent) {
    if let Err(e) = app.emit(EVENT_CHANNEL, &event) {
        eprintln!("event emit failed: {e}");
    }
}

// ─────────────────────────────────────────────────────────────
// Commands — servers
// ─────────────────────────────────────────────────────────────

/// Folders on disk, each carrying its live process state.
///
/// The registry only knows what is stored; the supervisor only knows what is
/// running. Merging happens here so the UI gets one list with the status light
/// already resolved.
#[tauri::command]
fn list_servers(state: State<'_, AppState>) -> CoreResult<Vec<ServerSummary>> {
    let mut servers = state.registry.list()?;
    for server in &mut servers {
        server.state = state.supervisor.state(&server.id);
        server.light = server.state.light();
        if let Some((uptime, players)) = state.supervisor.stats(&server.id) {
            server.uptime_secs = Some(uptime);
            server.players_online = Some(players);
        }
    }
    Ok(servers)
}

/// Create a server and install Forge into it from a jar already on disk.
///
/// One command rather than a sequence the UI drives, because the steps are not
/// independently meaningful: a folder with no Forge in it cannot start, and a
/// half-finished sequence would leave one behind if the window closed.
/// Installer output streams out as `CoreEvent::ServerLog` while this runs.
///
/// Accepts the Minecraft EULA on the user's behalf. That is a deliberate
/// product decision, not an oversight: the create dialog states it, and the
/// alternative is a server that installs successfully and then refuses to
/// start for a reason the user has already agreed to elsewhere.
#[tauri::command]
async fn create_server_from_installer(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    installer: PathBuf,
) -> CoreResult<ServerId> {
    if name.trim().is_empty() {
        return Err(CoreError::Precondition {
            message: "Server name cannot be empty".into(),
        });
    }
    // The UI picks from a list this app produced, but the argument still
    // crosses a trust boundary — only jars in the downloads folder may run.
    if installer.parent() != Some(state.downloads_dir.as_path()) || !installer.is_file() {
        return Err(CoreError::Precondition {
            message: "找不到安裝檔，請重新選擇。".into(),
        });
    }

    // Before the folder exists: the installer is itself a jar that needs the
    // right Java, and failing after `create` would leave an empty server behind
    // for the user to clean up.
    let mc_version = installer_mc_version(&installer);
    resolve_java(&state, &ServerConfig::default(), mc_version.as_deref())?;

    let id = state.registry.create(&name)?;
    emit(&app, CoreEvent::ServersChanged);

    let config = state.registry.load_config(&id)?;
    let config = resolve_java(&state, &config, mc_version.as_deref())?;
    let dir = state.registry.dir_of(&id);
    // A failure leaves the server in place rather than deleting it: the console
    // output is the only clue to what went wrong, and silently removing what
    // the user just made is worse than an entry they can delete themselves.
    state
        .supervisor
        .install(&id, &dir, &installer, &config)
        .await?;

    state.registry.set_eula(&id, true)?;

    let mut config = state.registry.load_config(&id)?;
    if let Some(version) = installer
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(forge::version_from_filename)
    {
        let (mc, build) = forge::split_version(&version);
        config.mc_version = Some(mc);
        config.forge_version = Some(build);
    }
    state.registry.save_config(&id, &config)?;

    emit(&app, CoreEvent::ServersChanged);
    Ok(id)
}

/// Adopt an installation that already exists on disk.
///
/// Registers the path; nothing is copied or moved, and deleting the server
/// later only unlinks it.
#[tauri::command]
fn import_server(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> CoreResult<ServerId> {
    let id = state.registry.import(&path)?;
    // Same policy as creating one: the app accepts the EULA on the user's
    // behalf. Without this an imported folder whose eula.txt says false can
    // never be started, and there is no other control that sets it.
    state.registry.set_eula(&id, true)?;
    emit(&app, CoreEvent::ServersChanged);
    Ok(id)
}

#[tauri::command]
fn delete_server(app: AppHandle, state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    // Removing the working directory of a live JVM corrupts the world save, so
    // this refuses rather than stopping the server on the user's behalf.
    if state.supervisor.is_active(&id) {
        return Err(CoreError::Precondition {
            message: "請先停止伺服器再刪除。".into(),
        });
    }
    state.registry.delete(&id)?;
    state.supervisor.forget(&id);
    emit(&app, CoreEvent::ServersChanged);
    Ok(())
}

#[tauri::command]
fn get_server_config(state: State<'_, AppState>, id: ServerId) -> CoreResult<ServerConfig> {
    state.registry.load_config(&id)
}

#[tauri::command]
fn save_server_config(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ServerId,
    config: ServerConfig,
) -> CoreResult<()> {
    state.registry.save_config(&id, &config)?;
    // The display name may have changed; the card grid must refetch.
    emit(&app, CoreEvent::ServersChanged);
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Commands — server.properties and EULA
// ─────────────────────────────────────────────────────────────

#[tauri::command]
fn get_server_properties(state: State<'_, AppState>, id: ServerId) -> CoreResult<ServerProperties> {
    state.registry.read_properties(&id)
}

#[tauri::command]
fn save_server_properties(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ServerId,
    properties: ServerProperties,
) -> CoreResult<()> {
    // Same reason `write_server_file` refuses: Minecraft holds the file in
    // memory and rewrites it on shutdown, so a save now would vanish when the
    // server stops. Refusing beats accepting an edit that silently disappears.
    if state.supervisor.is_active(&id) {
        return Err(CoreError::Precondition {
            message: "伺服器執行中，設定會在關閉時被覆寫。請先停止伺服器再修改。".into(),
        });
    }
    state.registry.write_properties(&id, &properties)?;
    // The card reads port and max players from this file.
    emit(&app, CoreEvent::ServersChanged);
    Ok(())
}

/// One of the two hand-editable files, verbatim. Missing reads as empty.
#[tauri::command]
fn read_server_file(
    state: State<'_, AppState>,
    id: ServerId,
    file: types::ServerFile,
) -> CoreResult<String> {
    state.registry.read_file(&id, file)
}

/// Write it back verbatim. Refused while the server is running: Minecraft holds
/// `server.properties` in memory and rewrites it on shutdown, so an edit made
/// now would be silently thrown away.
#[tauri::command]
fn write_server_file(
    state: State<'_, AppState>,
    id: ServerId,
    file: types::ServerFile,
    text: String,
) -> CoreResult<()> {
    if state.supervisor.is_active(&id) {
        return Err(CoreError::Precondition {
            message: "伺服器執行中，關閉時會覆寫這個檔案。請先停止伺服器再編輯。".into(),
        });
    }
    state.registry.write_file(&id, file, &text)
}

/// Reveal the server folder in Explorer. Not a plugin: one process spawn.
#[tauri::command]
fn open_server_folder(state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    let dir = state.registry.dir_of(&id);
    std::process::Command::new("explorer").arg(&dir).spawn()?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Commands — console
// ─────────────────────────────────────────────────────────────

/// Lines newer than `after_seq`. The console modal passes the last seq it holds
/// so reopening it fetches only the gap, not the whole buffer.
#[tauri::command]
fn console_since(state: State<'_, AppState>, id: ServerId, after_seq: u64) -> Vec<LogLine> {
    state.supervisor.console_since(&id, after_seq)
}

// ─────────────────────────────────────────────────────────────
// Commands — mods
// ─────────────────────────────────────────────────────────────

#[tauri::command]
fn list_mods(state: State<'_, AppState>, id: ServerId) -> CoreResult<Vec<types::ModFile>> {
    state.registry.list_mods(&id)
}

/// Copy jars into the server's `mods/`.
///
/// Refused while the server is running: Forge reads the folder once at startup,
/// so nothing added now would load, and Windows holds the jars it has open —
/// a delete would fail outright.
#[tauri::command]
fn add_mods(state: State<'_, AppState>, id: ServerId, paths: Vec<PathBuf>) -> CoreResult<()> {
    reject_while_running(&state, &id)?;
    for path in &paths {
        state.registry.add_mod(&id, path)?;
    }
    Ok(())
}

#[tauri::command]
fn delete_mod(state: State<'_, AppState>, id: ServerId, name: String) -> CoreResult<()> {
    reject_while_running(&state, &id)?;
    state.registry.delete_mod(&id, &name)
}

fn reject_while_running(state: &AppState, id: &ServerId) -> CoreResult<()> {
    if state.supervisor.is_active(id) {
        return Err(CoreError::Precondition {
            message: "伺服器執行中，模組要在停止時才能變更。".into(),
        });
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Commands — remote hosts
// ─────────────────────────────────────────────────────────────

#[tauri::command]
fn list_remotes(state: State<'_, AppState>) -> Vec<types::RemoteHost> {
    state.remotes.list()
}

/// Add or update one. An empty id means new; the assigned entry comes back.
#[tauri::command]
fn save_remote(
    state: State<'_, AppState>,
    host: types::RemoteHost,
) -> CoreResult<types::RemoteHost> {
    state.remotes.save(host)
}

#[tauri::command]
fn delete_remote(state: State<'_, AppState>, id: String) -> CoreResult<()> {
    state.remotes.delete(&id)
}

/// Connect and report what is on the other side.
///
/// Not a bare "ok": a login that succeeds against the wrong folder, or a
/// pairing code that reaches somebody else's machine, both look like success
/// until an edit goes somewhere unexpected.
#[tauri::command]
async fn test_remote(state: State<'_, AppState>, id: String) -> CoreResult<String> {
    let host = state.remotes.get(&id)?;
    match host.transport {
        types::Transport::Ssh => remote::test(&host).await,
        types::Transport::App => {
            let servers = remote::agent_servers(&host).await?;
            Ok(match servers.len() {
                0 => "連上了，但對方還沒有任何伺服器。".into(),
                n => format!(
                    "連上了，對方有 {n} 座伺服器：{}",
                    servers
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            })
        }
    }
}

/// The servers on that machine. `App` transport only — over SSH this app is
/// pointed at one folder and has no list to ask for.
#[tauri::command]
async fn remote_servers(
    state: State<'_, AppState>,
    id: String,
) -> CoreResult<Vec<agent::RemoteServer>> {
    remote::agent_servers(&state.remotes.get(&id)?).await
}

/// The host key fingerprint, for the user to read before trusting it. SSH only.
#[tauri::command]
async fn remote_fingerprint(state: State<'_, AppState>, id: String) -> CoreResult<String> {
    remote::fingerprint(&state.remotes.get(&id)?).await
}

/// Record the host key. Only after the user has seen the fingerprint.
#[tauri::command]
async fn trust_remote(state: State<'_, AppState>, id: String) -> CoreResult<()> {
    remote::trust(&state.remotes.get(&id)?).await
}

/// `server` names which server on the far side; it is required for `App` and
/// ignored for `Ssh`, where the host entry already points at one folder.
#[tauri::command]
async fn read_remote_file(
    state: State<'_, AppState>,
    id: String,
    server: Option<String>,
    file: types::ServerFile,
) -> CoreResult<String> {
    let host = state.remotes.get(&id)?;
    match host.transport {
        types::Transport::Ssh => remote::read(&host, file).await,
        types::Transport::App => remote::agent_read(&host, require_server(server)?, file).await,
    }
}

#[tauri::command]
async fn write_remote_file(
    state: State<'_, AppState>,
    id: String,
    server: Option<String>,
    file: types::ServerFile,
    text: String,
) -> CoreResult<()> {
    let host = state.remotes.get(&id)?;
    match host.transport {
        types::Transport::Ssh => remote::write_file(&host, file, &text).await,
        types::Transport::App => {
            remote::agent_write(&host, require_server(server)?, file, text).await
        }
    }
}

fn require_server(server: Option<String>) -> CoreResult<String> {
    server
        .filter(|s| !s.is_empty())
        .ok_or(CoreError::Precondition {
            message: "請先選擇對方的伺服器。".into(),
        })
}

// ─────────────────────────────────────────────────────────────
// Commands — this machine's listener
// ─────────────────────────────────────────────────────────────

#[tauri::command]
fn get_agent_settings(state: State<'_, AppState>) -> AgentSettings {
    agent::load_settings(&state.data_dir)
}

/// Switch the listener on or off, or move it to another port.
///
/// Binding is done here rather than on the next launch so the switch means what
/// it says, and a port already in use fails visibly instead of leaving the UI
/// claiming to be listening.
#[tauri::command]
async fn set_agent_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
    port: u16,
) -> CoreResult<AgentSettings> {
    let mut settings = agent::load_settings(&state.data_dir);
    settings.enabled = enabled;
    settings.port = port;

    // Always stop first: a port change is a stop and a start.
    if let Some(previous) = state
        .running_agent
        .lock()
        .expect("agent mutex poisoned")
        .take()
    {
        previous.stop();
    }
    if enabled {
        let started = start_agent(&app, &state, &settings).await?;
        *state.running_agent.lock().expect("agent mutex poisoned") = Some(started);
    }

    agent::save_settings(&state.data_dir, &settings)?;
    Ok(settings)
}

/// New pairing code. Everyone holding the old one stops being able to connect,
/// which is the point — it is the only way to take access back.
#[tauri::command]
async fn regenerate_agent_token(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CoreResult<AgentSettings> {
    let mut settings = agent::load_settings(&state.data_dir);
    settings.token = agent::new_token();
    agent::save_settings(&state.data_dir, &settings)?;

    // The running listener holds the old code; restart it on the new one.
    if settings.enabled {
        if let Some(previous) = state
            .running_agent
            .lock()
            .expect("agent mutex poisoned")
            .take()
        {
            previous.stop();
        }
        let started = start_agent(&app, &state, &settings).await?;
        *state.running_agent.lock().expect("agent mutex poisoned") = Some(started);
    }
    Ok(settings)
}

async fn start_agent(
    app: &AppHandle,
    state: &AppState,
    settings: &AgentSettings,
) -> CoreResult<Agent> {
    let handle = app.clone();
    Agent::start(
        Arc::clone(&state.registry),
        settings.clone(),
        Arc::new(move |line| emit(&handle, CoreEvent::AgentActivity { line })),
    )
    .await
}

// ─────────────────────────────────────────────────────────────
// Commands — java
// ─────────────────────────────────────────────────────────────

/// A config with `java_path` filled in, or `JavaMissing` naming the major to
/// fetch.
///
/// Resolved here rather than inside the supervisor so the failure is a typed
/// error the UI can put a download button on, instead of a spawn that fails
/// with "program not found" once the process is already half set up.
///
/// `mc_version` is `None` for a server whose version we never recorded — an
/// imported folder, usually. Nothing is checked in that case: guessing a major
/// and refusing to start would be worse than letting the launch try.
fn resolve_java(
    state: &AppState,
    config: &ServerConfig,
    mc_version: Option<&str>,
) -> CoreResult<ServerConfig> {
    if config.java_path.is_some() {
        return Ok(config.clone());
    }
    let Some(version) = mc_version else {
        return Ok(config.clone());
    };
    let major = types::required_java_major(version);
    let Some(path) = state.java.find(major) else {
        return Err(CoreError::JavaMissing { major });
    };
    Ok(ServerConfig {
        java_path: Some(path),
        ..config.clone()
    })
}

/// The Minecraft version an installer jar is for, read from its filename.
fn installer_mc_version(installer: &Path) -> Option<String> {
    let version = installer
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(forge::version_from_filename)?;
    Some(forge::split_version(&version).0)
}

/// Fetch and unpack the JRE for `major`. Progress arrives as
/// `CoreEvent::Download` with a `jre` kind, same as any other transfer.
#[tauri::command]
async fn install_java(state: State<'_, AppState>, major: u8) -> CoreResult<()> {
    let archive = state
        .downloader
        .fetch(
            DownloadKind::Jre { major },
            state.java.url(major),
            state.java.archive_path(major),
        )
        .await?;
    state.java.install(&archive, major)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Commands — process control
// ─────────────────────────────────────────────────────────────

#[tauri::command]
async fn start_server(state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    let config = state.registry.load_config(&id)?;
    let dir = state.registry.dir_of(&id);
    if !state.registry.eula_accepted(&id) {
        return Err(CoreError::Precondition {
            message: "請先同意 Minecraft EULA。".into(),
        });
    }
    let config = resolve_java(&state, &config, config.mc_version.as_deref())?;
    state.supervisor.start(&id, &dir, &config).await
}

/// Graceful shutdown. The world saves; a force-kill follows only if the server
/// ignores `stop` past the grace period.
#[tauri::command]
fn stop_server(state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    state.supervisor.stop(&id)
}

#[tauri::command]
fn send_console_command(
    state: State<'_, AppState>,
    id: ServerId,
    command: String,
) -> CoreResult<()> {
    state.supervisor.send(&id, command)
}

// ─────────────────────────────────────────────────────────────
// Commands — downloads
// ─────────────────────────────────────────────────────────────

#[tauri::command]
fn cancel_download(state: State<'_, AppState>, id: DownloadId) {
    state.downloader.cancel(id);
}

// ─────────────────────────────────────────────────────────────
// Commands — Forge
// ─────────────────────────────────────────────────────────────

/// Published Forge builds, grouped by Minecraft line, newest first.
/// Cached for the life of the process unless `refresh` is set.
#[tauri::command]
async fn list_forge_versions(
    state: State<'_, AppState>,
    refresh: bool,
) -> CoreResult<Vec<ForgeVersionGroup>> {
    state.forge.versions(refresh).await
}

/// Fetch the installer for one Forge version into the shared downloads folder.
///
/// The URL and filename are derived here rather than in the UI so a version
/// string can only ever produce the one file Forge itself would publish.
#[tauri::command]
fn download_forge(state: State<'_, AppState>, version: String) -> CoreResult<DownloadId> {
    let url = forge::installer_url(&version)?;
    let filename = forge::installer_filename(&version)?;
    let dest = state.downloads_dir.join(filename);
    state
        .downloader
        .start(DownloadKind::ForgeInstaller { version }, url, dest)
}

/// Installer jars already on disk, newest first.
///
/// Sorted by modification time because the one you just fetched is the one you
/// are looking for; alphabetical order buries it among older versions.
#[tauri::command]
fn list_downloads(state: State<'_, AppState>) -> CoreResult<Vec<types::DownloadedFile>> {
    let mut jars: Vec<(std::time::SystemTime, types::DownloadedFile)> =
        std::fs::read_dir(&state.downloads_dir)?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jar"))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((
                    meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    types::DownloadedFile {
                        name: e.file_name().to_string_lossy().into_owned(),
                        bytes: meta.len(),
                        path: e.path(),
                    },
                ))
            })
            .collect();
    jars.sort_by_key(|(modified, _)| Reverse(*modified));
    Ok(jars.into_iter().map(|(_, f)| f).collect())
}

#[tauri::command]
fn delete_download(state: State<'_, AppState>, path: PathBuf) -> CoreResult<()> {
    // Only files inside the downloads directory, whatever the UI sends.
    if path.parent() != Some(state.downloads_dir.as_path()) {
        return Err(CoreError::Config {
            message: "refusing to delete a file outside the downloads folder".into(),
        });
    }
    std::fs::remove_file(path)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // The update flow is driven entirely from the UI: check on launch,
        // then a modal the user cannot dismiss until they take the update.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Servers live under the app data dir, not next to the executable:
            // an app installed into Program Files cannot write beside itself.
            let data_dir = app.path().app_data_dir()?;
            let registry = Arc::new(Registry::new(data_dir.join("servers"))?);

            // The supervisor pushes events from background tasks, so it gets an
            // owned handle rather than borrowing the one `setup` was given.
            let handle = app.handle().clone();
            let sink: types::EventSink = Arc::new(move |event| emit(&handle, event));
            let supervisor = Supervisor::new(Arc::clone(&sink));
            // Downloads are started from synchronous commands, which run off the
            // async runtime — the handle has to be handed over explicitly.
            let downloader = Downloader::new(sink, tauri::async_runtime::handle().inner().clone())?;

            let downloads_dir = data_dir.join("downloads");
            std::fs::create_dir_all(&downloads_dir)?;

            app.manage(AppState {
                registry,
                supervisor,
                downloader,
                forge: Forge::new()?,
                java: Java::new(data_dir.join("java"))?,
                remotes: Remotes::new(&data_dir)?,
                running_agent: std::sync::Mutex::new(None),
                data_dir: data_dir.clone(),
                downloads_dir,
            });

            // Remote access, if the user left it switched on. Started here so
            // it rides along with the app: the machine running the server is
            // the machine that has to be reachable, and it is already open.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let settings = agent::load_settings(&state.data_dir);
                if !settings.enabled {
                    return;
                }
                match start_agent(&handle, &state, &settings).await {
                    Ok(started) => {
                        *state.running_agent.lock().expect("agent mutex poisoned") = Some(started);
                        emit(
                            &handle,
                            CoreEvent::AgentActivity {
                                line: format!("遠端存取已啟動，連接埠 {}", settings.port),
                            },
                        );
                    }
                    Err(e) => emit(&handle, CoreEvent::Error { id: None, error: e }),
                }
            });

            #[cfg(target_os = "windows")]
            {
                use tauri::Manager as _;
                if let Some(window) = app.get_webview_window("main") {
                    // Mica is Windows 11 only; on 10 this fails and the window
                    // just falls back to its solid background.
                    let _ = window_vibrancy::apply_mica(&window, Some(true));
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_servers,
            create_server_from_installer,
            import_server,
            delete_server,
            get_server_config,
            save_server_config,
            get_server_properties,
            save_server_properties,
            list_mods,
            add_mods,
            delete_mod,
            list_remotes,
            save_remote,
            delete_remote,
            test_remote,
            remote_servers,
            get_agent_settings,
            set_agent_settings,
            regenerate_agent_token,
            remote_fingerprint,
            trust_remote,
            read_remote_file,
            write_remote_file,
            install_java,
            console_since,
            read_server_file,
            write_server_file,
            open_server_folder,
            start_server,
            stop_server,
            send_console_command,
            cancel_download,
            list_downloads,
            delete_download,
            list_forge_versions,
            download_forge,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
