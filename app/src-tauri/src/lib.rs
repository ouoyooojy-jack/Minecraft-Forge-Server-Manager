//! Tauri wiring: application state and the IPC command surface.
//!
//! Rule for this file: no business logic. Commands validate, delegate to a core
//! module, and translate the result. Anything worth testing lives in a module
//! that does not import `tauri`, so it can be tested without a window.

mod agent;
mod backup;
mod crash;
mod download;
mod forge;
mod java;
mod logbuf;
mod playit;
mod properties;
mod registry;
mod remote;
mod supervisor;
#[cfg(test)]
mod testutil;
mod types;

use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

use agent::{Agent, AgentSettings};
use download::Downloader;
use forge::Forge;
use java::Java;
use playit::Playit;
use registry::Registry;
use remote::Remotes;
use supervisor::Supervisor;
use types::{
    CoreError, CoreEvent, CoreResult, DownloadId, DownloadKind, ForgeVersionGroup, LogLine,
    ServerConfig, ServerId, ServerState, ServerSummary, EVENT_CHANNEL,
};

pub struct AppState {
    registry: Arc<Registry>,
    /// Owns every running process and the console history that outlives it.
    ///
    /// Shared rather than owned: the auto-restart path has to keep a handle
    /// across an await, and a `State` borrow cannot cross one.
    supervisor: Arc<Supervisor>,
    /// Crashes in a row per server, for the auto-restart give-up rule. Cleared
    /// the moment a server reaches `Online`.
    restart_counts: Mutex<std::collections::HashMap<ServerId, u32>>,
    /// Servers waiting out `RESTART_DELAY` after a crash. Taking one out of
    /// the set — a start or stop by hand, a delete, the cancel button — is how
    /// the restart is called off.
    pending_restarts: Mutex<HashSet<ServerId>>,
    /// Servers whose world is being zipped after a stop. Starting one now
    /// would have Minecraft writing into the files the zip is reading.
    backing_up: Mutex<HashSet<ServerId>>,
    /// Servers the scheduled restart is cycling right now.
    scheduled: Mutex<HashSet<ServerId>>,
    /// Set once "quit" is chosen. Nothing may start after that, or the exit
    /// would wait on a server it is about to orphan.
    quitting: AtomicBool,
    downloader: Downloader,
    forge: Forge,
    /// Locates a JRE, and unpacks one when the machine has none.
    java: Java,
    /// The playit.gg tunnel agent, for servers that cannot be port-forwarded.
    playit: Playit,
    /// Other people's machines.
    remotes: Remotes,
    /// This machine's own listener, when the user has switched it on. `None`
    /// means nothing is bound and nothing is answering.
    running_agent: Mutex<Option<Agent>>,
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
    // The tray is the only thing on screen while the window is hidden, so it
    // has to carry the answer to "is anything still running?" by itself.
    if matches!(
        event,
        CoreEvent::ServerState { .. } | CoreEvent::ServersChanged | CoreEvent::Players { .. }
    ) {
        refresh_tray_tooltip(app);
    }
    if let CoreEvent::ServerState { id, state, .. } = &event {
        match state {
            // A server that got all the way up is not in a crash loop.
            ServerState::Online => {
                app.state::<AppState>()
                    .restart_counts
                    .lock()
                    .expect("restart mutex poisoned")
                    .remove(id);
            }
            ServerState::Crashed { .. } => {
                maybe_restart(app, id.clone());
                after_stop(app, id.clone(), false);
            }
            ServerState::Stopped => after_stop(app, id.clone(), true),
            _ => {}
        }
    }
    if let Err(e) = app.emit(EVENT_CHANNEL, &event) {
        eprintln!("event emit failed: {e}");
    }
}

/// Wait this long before bringing a crashed server back.
///
/// Long enough for the port to be released and for a human watching the
/// console to read what happened before it scrolls away.
const RESTART_DELAY: Duration = Duration::from_secs(10);

/// Give up after this many crashes in a row.
const RESTART_LIMIT: u32 = 3;

/// Start a crashed server again, if that could possibly help.
///
/// Two guards, and both matter more than the feature does. A restart cannot
/// fix a missing mod dependency, the wrong Java, an unaccepted EULA, or a port
/// somebody else is holding — those fail identically forever, so the
/// diagnosis decides whether to bother. And even for a fault that might be
/// transient, a server that has crashed three times running is not going to
/// come up on the fourth; it is going to do this all night.
fn maybe_restart(app: &AppHandle, id: ServerId) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let tries = {
            let state = app.state::<AppState>();
            let Ok(config) = state.registry.load_config(&id) else {
                return;
            };
            if !config.restart_on_crash || state.quitting.load(Ordering::SeqCst) {
                return;
            }

            // Ask the same diagnosis the UI shows. A fault with a fix the user
            // has to apply is not one to retry.
            let log: Vec<String> = state
                .supervisor
                .console_since(&id, 0)
                .into_iter()
                .map(|l| l.text)
                .collect();
            let info = crash::diagnose(
                &state.registry.dir_of(&id),
                &log,
                None,
                state
                    .supervisor
                    .launched_at(&id)
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            );
            if info.fix.is_some() {
                announce(&app, &id, format!("不自動重啟：{}", info.headline));
                return;
            }

            let tries = {
                let mut counts = state
                    .restart_counts
                    .lock()
                    .expect("restart mutex poisoned");
                let n = counts.entry(id.clone()).or_insert(0);
                *n += 1;
                *n
            };
            if tries > RESTART_LIMIT {
                announce(
                    &app,
                    &id,
                    format!("已連續當機 {RESTART_LIMIT} 次，停止自動重啟。"),
                );
                return;
            }
            held(&state.pending_restarts).insert(id.clone());
            tries
        };
        // The page shows a cancel button while one is pending.
        emit(&app, CoreEvent::ServersChanged);

        announce(
            &app,
            &id,
            format!("當機，{} 秒後自動重啟（第 {tries} 次）。", RESTART_DELAY.as_secs()),
        );
        tokio::time::sleep(RESTART_DELAY).await;

        // Called off in the meantime, or already taken care of by hand.
        if !held(&app.state::<AppState>().pending_restarts).remove(&id) {
            return;
        }
        emit(&app, CoreEvent::ServersChanged);
        if let Err(error) = launch(&app, &id).await {
            announce(&app, &id, format!("自動重啟失敗：{}", in_words(&error)));
            emit(
                &app,
                CoreEvent::Error {
                    id: Some(id),
                    error,
                },
            );
        }
    });
}

/// The start every path shares: the button, the restart after a crash, and
/// the scheduled one. One order of checks, so none of them can skip one.
async fn launch(app: &AppHandle, id: &ServerId) -> CoreResult<()> {
    let state = app.state::<AppState>();
    if state.quitting.load(Ordering::SeqCst) {
        return Err(CoreError::Precondition {
            message: "程式正在關閉。".into(),
        });
    }
    if held(&state.backing_up).contains(id) {
        return Err(CoreError::Precondition {
            message: "正在備份世界，備份完成後再啟動。".into(),
        });
    }
    if !state.registry.eula_accepted(id) {
        return Err(CoreError::Precondition {
            message: "請先同意 Minecraft EULA。".into(),
        });
    }
    let config = state.registry.load_config(id)?;
    port_is_free(&state, id)?;
    let config = resolve_java(&state, &config, config.mc_version.as_deref())?;
    state
        .supervisor
        .start(id, &state.registry.dir_of(id), &config)
        .await?;
    open_tunnel_if_wanted(app, id.clone());
    Ok(())
}

/// What follows a server going down, whichever way it went.
///
/// Runs synchronously up to the point of deciding, so that anything checking
/// `backing_up` straight after the stop event already sees the backup.
fn after_stop(app: &AppHandle, id: ServerId, clean: bool) {
    let state = app.state::<AppState>();

    // The public address follows the servers: with none left to point at,
    // the agent goes down too. Not while one is about to come back.
    if state.playit.auto()
        && state.supervisor.active().is_empty()
        && held(&state.pending_restarts).is_empty()
        && held(&state.scheduled).is_empty()
    {
        state.playit.stop();
    }

    // Only after a clean stop: after a crash the world may be mid-write, and
    // a copy of that is the thing a backup is supposed to protect against.
    if !clean {
        return;
    }
    let Ok(config) = state.registry.load_config(&id) else {
        return;
    };
    if !config.backup_on_stop {
        return;
    }
    let Ok(world) = world_dir(&state, &id) else {
        return;
    };
    if !world.is_dir() {
        return;
    }
    let store = backup_store(&state, &id);
    held(&state.backing_up).insert(id.clone());
    announce(app, &id, "正在備份世界…".into());

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            backup::create(&world, &store, KEEP_STOP_BACKUPS, backup::STOP_PREFIX)
        })
        .await;
        held(&app.state::<AppState>().backing_up).remove(&id);
        announce(
            &app,
            &id,
            match result {
                Ok(Ok(name)) => format!("已備份世界：{name}"),
                Ok(Err(e)) => format!("備份失敗：{}", in_words(&e)),
                Err(e) => format!("備份失敗：{e}"),
            },
        );
        emit(&app, CoreEvent::ServersChanged);
    });
}

/// How many of the copies taken on stop to keep. Fewer than the hand-made
/// ones: they arrive every time the server stops, so they turn over fast.
const KEEP_STOP_BACKUPS: usize = 5;

/// A set of servers, locked.
fn held(set: &Mutex<HashSet<ServerId>>) -> MutexGuard<'_, HashSet<ServerId>> {
    set.lock().expect("server set mutex poisoned")
}

/// An error as a sentence for the console, where the UI's own wording for
/// each kind of error is not available.
fn in_words(error: &CoreError) -> String {
    match error {
        CoreError::JavaMissing { major } => {
            format!("找不到 Java {major}。到伺服器頁按「啟動」，會提示下載。")
        }
        other => other.to_string(),
    }
}

/// Put a line in the server's own console, so the decision is visible where
/// the crash is rather than in a toast nobody was looking at.
fn announce(app: &AppHandle, id: &ServerId, text: String) {
    app.state::<AppState>().supervisor.note(id, text);
}

/// Name what is running, in the tray tooltip.
fn refresh_tray_tooltip(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let state = app.state::<AppState>();

    let running: Vec<String> = state
        .registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| state.supervisor.is_active(&s.id))
        .map(|s| s.name)
        .collect();

    let _ = tray.set_tooltip(Some(match running.len() {
        0 => "Mc Server Manager".to_owned(),
        // Naming them is the point; past a handful the list stops fitting and
        // stops being read, so it collapses to a count.
        1..=3 => format!("執行中：{}", running.join("、")),
        n => format!("{n} 座伺服器執行中"),
    }));
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
            server.players = Some(players);
        }
        server.restart_pending = held(&state.pending_restarts).contains(&server.id);
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
    install_new_server(&app, &state, &name, &installer).await
}

/// The same, from a Forge version rather than a jar: fetch the installer if it
/// is not on disk yet, then install. One step for the person creating a
/// server, who should not have to visit the downloads page first.
#[tauri::command]
async fn create_server(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    version: String,
) -> CoreResult<ServerId> {
    require_name(&name)?;
    let installer = state
        .downloads_dir
        .join(forge::installer_filename(&version)?);
    if !installer.is_file() {
        state
            .downloader
            .fetch(
                DownloadKind::ForgeInstaller {
                    version: version.clone(),
                },
                forge::installer_url(&version)?,
                installer.clone(),
            )
            .await?;
    }
    install_new_server(&app, &state, &name, &installer).await
}

fn require_name(name: &str) -> CoreResult<()> {
    if name.trim().is_empty() {
        return Err(CoreError::Precondition {
            message: "伺服器名稱不能是空的。".into(),
        });
    }
    Ok(())
}

async fn install_new_server(
    app: &AppHandle,
    state: &AppState,
    name: &str,
    installer: &Path,
) -> CoreResult<ServerId> {
    require_name(name)?;
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
    let mc_version = installer_mc_version(installer);
    resolve_java(state, &ServerConfig::default(), mc_version.as_deref())?;

    let id = state.registry.create(name)?;
    emit(app, CoreEvent::ServersChanged);

    let config = state.registry.load_config(&id)?;
    let config = resolve_java(state, &config, mc_version.as_deref())?;
    let dir = state.registry.dir_of(&id);
    // A failure leaves the server in place rather than deleting it: the console
    // output is the only clue to what went wrong, and silently removing what
    // the user just made is worse than an entry they can delete themselves.
    state
        .supervisor
        .install(&id, &dir, installer, &config)
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

    emit(app, CoreEvent::ServersChanged);
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
    if held(&state.backing_up).contains(&id) {
        return Err(CoreError::Precondition {
            message: "正在備份世界，備份完成後再刪除。".into(),
        });
    }
    held(&state.pending_restarts).remove(&id);
    let tunnel = state
        .registry
        .load_config(&id)
        .ok()
        .and_then(|c| c.tunnel_id);
    state.registry.delete(&id)?;
    state.supervisor.forget(&id);
    emit(&app, CoreEvent::ServersChanged);

    // Its public address goes with it — the confirm dialog says so. Best
    // effort: the server is already gone, and a tunnel left behind is one
    // the user can still remove on playit's site.
    if let Some(tunnel) = tunnel {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = app.state::<AppState>().playit.delete_tunnel(&tunnel).await {
                playit_log(&app)(format!("刪除公開位址失敗：{e}"));
            }
        });
    }
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
    require_name(&config.name)?;
    let before = state.registry.load_config(&id)?;
    // Not the page's to change: where an imported server lives, and which
    // tunnel carries this one. A page that loaded before the tunnel was made
    // would otherwise save it away.
    let config = ServerConfig {
        external_path: before.external_path.clone(),
        tunnel_id: before.tunnel_id.clone(),
        ..config
    };
    state.registry.save_config(&id, &config)?;
    // The display name may have changed; the card grid must refetch.
    emit(&app, CoreEvent::ServersChanged);

    // Keep the name on playit's dashboard in step, for whoever reads it there.
    if let (true, Some(tunnel)) = (config.name != before.name, config.tunnel_id) {
        let app = app.clone();
        let name = config.name;
        tauri::async_runtime::spawn(async move {
            let _ = app
                .state::<AppState>()
                .playit
                .rename_tunnel(&tunnel, &name)
                .await;
        });
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Commands — server.properties and EULA
// ─────────────────────────────────────────────────────────────

/// Every editable line of `server.properties`, with the control each needs.
#[tauri::command]
fn get_properties(
    state: State<'_, AppState>,
    id: ServerId,
) -> CoreResult<Vec<properties::PropertyField>> {
    state.registry.property_fields(&id)
}

/// Write back only the keys that changed.
///
/// Refused while the server is running for the same reason the raw editor is:
/// Minecraft holds the file in memory and rewrites it on shutdown, so an edit
/// made now would vanish when the server stops.
#[tauri::command]
fn save_properties(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ServerId,
    changes: std::collections::BTreeMap<String, String>,
) -> CoreResult<()> {
    if state.supervisor.is_active(&id) {
        return Err(CoreError::Precondition {
            message: "伺服器執行中，設定會在關閉時被覆寫。請先停止伺服器再修改。".into(),
        });
    }
    state.registry.write_property_fields(&id, &changes)?;
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
    let supervisor = Arc::clone(&state.supervisor);
    Agent::start(
        Arc::clone(&state.registry),
        Arc::new(move |id| supervisor.is_active(id)),
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
    if config.java_path.as_deref().is_some_and(Path::is_file) {
        return Ok(config.clone());
    }
    // A stored path that no longer exists — a JRE folder deleted by hand — is
    // treated as none, rather than failing the spawn with "file not found".
    let config = &ServerConfig {
        java_path: None,
        ..config.clone()
    };
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
// Commands — playit.gg tunnels
// ─────────────────────────────────────────────────────────────

/// How long the user gets to sign in and approve the link in their browser.
/// Generous on purpose: this includes registering an account for the first time.
const PLAYIT_CLAIM_TIMEOUT: Duration = Duration::from_secs(300);

#[tauri::command]
async fn playit_status(state: State<'_, AppState>) -> CoreResult<playit::PlayitStatus> {
    Ok(state.playit.status().await)
}

/// Fetch the agent from playit's own release. Progress rides the normal
/// download events, so it appears wherever a Forge or JRE download would.
#[tauri::command]
async fn playit_install(state: State<'_, AppState>) -> CoreResult<()> {
    let dest = state.playit.exe_path();
    state
        .downloader
        .fetch(DownloadKind::PlayitAgent, state.playit.url(), dest)
        .await?;
    Ok(())
}

/// Step one of linking: open the claim page in the user's own browser.
///
/// The sign-in happens on playit's site, in a real browser, so this app never
/// sees an account password. The URL is returned alongside the code because
/// `explorer` reports nothing back — if no browser opened, the UI can still
/// show the address to type by hand. The code goes to the UI rather than into
/// core state so that an abandoned claim leaves nothing behind to clean up.
#[tauri::command]
fn playit_claim_url(state: State<'_, AppState>) -> CoreResult<(String, String)> {
    let (code, url) = state.playit.claim_url();
    std::process::Command::new("explorer").arg(&url).spawn()?;
    Ok((code, url))
}

/// Step two: block until the user approves in the browser, then store the key.
#[tauri::command]
async fn playit_finish_claim(state: State<'_, AppState>, code: String) -> CoreResult<()> {
    state
        .playit
        .finish_claim(&code, PLAYIT_CLAIM_TIMEOUT)
        .await
}

/// Stop waiting on a claim the user walked away from.
#[tauri::command]
fn playit_cancel_claim(state: State<'_, AppState>) {
    state.playit.cancel_claim();
}

/// Start the tunnel agent. Its log goes to the same activity feed as the remote
/// listener's, which is where someone looks when a tunnel is not working.
#[tauri::command]
fn playit_start(app: AppHandle, state: State<'_, AppState>) -> CoreResult<()> {
    state.playit.start(playit_log(&app))
}

/// Where the agent's own output, and anything said about it, goes.
fn playit_log(app: &AppHandle) -> impl Fn(String) + Send + Sync + 'static {
    let handle = app.clone();
    move |line| {
        emit(
            &handle,
            CoreEvent::AgentActivity {
                line: format!("playit: {line}"),
            },
        );
    }
}

#[tauri::command]
fn playit_stop(state: State<'_, AppState>) {
    state.playit.stop();
}

/// Give one server a public address, or re-point the one it has.
///
/// The name and port come from the registry rather than from the UI — a
/// caller that could choose them could point a tunnel at a server it does
/// not name.
#[tauri::command]
async fn playit_create_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ServerId,
) -> CoreResult<()> {
    let config = state.registry.load_config(&id)?;
    // server.properties does not exist until the first run, and the default
    // is the one Minecraft will write into it.
    let port = server_port(&state, &id).unwrap_or(DEFAULT_MC_PORT);
    let tunnel = state
        .playit
        .ensure(config.tunnel_id.as_deref(), &config.name, port, playit_log(&app))
        .await?;
    remember_tunnel(&app, &id, tunnel)
}

/// Store which tunnel a server's address lives on, if that is news.
fn remember_tunnel(app: &AppHandle, id: &ServerId, tunnel: String) -> CoreResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.registry.load_config(id)?;
    if config.tunnel_id.as_deref() != Some(tunnel.as_str()) {
        config.tunnel_id = Some(tunnel);
        state.registry.save_config(id, &config)?;
        emit(app, CoreEvent::ServersChanged);
    }
    Ok(())
}

#[tauri::command]
async fn playit_delete_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CoreResult<()> {
    state.playit.delete_tunnel(&id).await?;
    // Whichever server pointed at it no longer has one.
    for server in state.registry.list().unwrap_or_default() {
        if let Ok(mut config) = state.registry.load_config(&server.id) {
            if config.tunnel_id.as_deref() == Some(id.as_str()) {
                config.tunnel_id = None;
                let _ = state.registry.save_config(&server.id, &config);
            }
        }
    }
    emit(&app, CoreEvent::ServersChanged);
    Ok(())
}

/// Switch "public address follows the server" on or off.
#[tauri::command]
fn playit_set_auto(state: State<'_, AppState>, on: bool) -> CoreResult<()> {
    state.playit.set_auto(on)
}

/// Forget this machine's key. The agent stays on the user's playit account
/// until they remove it there; this app does not delete other people's account
/// resources on its own.
#[tauri::command]
fn playit_unlink(state: State<'_, AppState>) -> CoreResult<()> {
    state.playit.unlink()
}

// ─────────────────────────────────────────────────────────────
// Commands — process control
// ─────────────────────────────────────────────────────────────

#[tauri::command]
async fn start_server(app: AppHandle, state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    // Starting by hand supersedes a restart that was still counting down.
    if held(&state.pending_restarts).remove(&id) {
        emit(&app, CoreEvent::ServersChanged);
    }
    launch(&app, &id).await
}

/// Bring this server's public address up alongside it, if the user asked for
/// that in settings.
///
/// In the background and infallible from the caller's point of view: playit
/// being slow or unreachable is not a reason to hold up, or refuse, a server
/// that the people on the same network can still join. The failure goes to
/// the activity feed, which is where a missing public address is looked into.
fn open_tunnel_if_wanted(app: &AppHandle, id: ServerId) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if !state.playit.auto() || !state.playit.linked() {
            return;
        }
        let Ok(config) = state.registry.load_config(&id) else {
            return;
        };
        let port = server_port(&state, &id).unwrap_or(DEFAULT_MC_PORT);
        let result = state
            .playit
            .ensure(config.tunnel_id.as_deref(), &config.name, port, playit_log(&app))
            .await
            .and_then(|tunnel| remember_tunnel(&app, &id, tunnel));
        if let Err(e) = result {
            playit_log(&app)(format!("公開位址沒開起來：{e}"));
        }
    });
}

/// What Minecraft listens on when `server.properties` does not say otherwise.
const DEFAULT_MC_PORT: u16 = 25565;

/// The port this server answers on, as the file has it.
///
/// `None` means the file has no usable value — it does not exist yet, or the
/// line is empty or zero. Callers decide whether that is a refusal or the
/// default; a tunnel wants the default, a collision check wants to stay quiet.
fn server_port(state: &AppState, id: &ServerId) -> Option<u16> {
    state
        .registry
        .read_raw_properties(id)
        .ok()?
        .get("server-port")
        .and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
}

/// Refuse a start that is going to fail on the port.
///
/// Minecraft's own failure here is a `BindException` two hundred lines into
/// the log, after which the process exits and the server reads as "crashed" —
/// which is a terrible way to learn that the other server is already using
/// 25565. Binding it ourselves for a moment answers the question up front.
///
/// A server with no `server-port` line yet gets the benefit of the doubt: the
/// file is written on first run, and refusing to start it would be worse than
/// letting Minecraft report the collision itself.
fn port_is_free(state: &AppState, id: &ServerId) -> CoreResult<()> {
    let Some(port) = server_port(state, id) else {
        return Ok(());
    };

    match std::net::TcpListener::bind(("0.0.0.0", port)) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(CoreError::Precondition {
            message: format!(
                "連接埠 {port} 已被占用。可能是另一座伺服器正在執行，或有別的程式在用這個埠。"
            ),
        }),
        // Anything else — no permission, a strange adapter — is not something
        // to block a start over. Minecraft will say so if it matters.
        Err(_) => Ok(()),
    }
}

/// How many hand-made backups to keep per server before the oldest is dropped.
///
/// A world runs to hundreds of megabytes and this button is one click, so an
/// unbounded store fills a disk without ever saying so. Copies taken
/// automatically are counted separately.
const KEEP_BACKUPS: usize = 10;

/// Where a server's world folder is. `level-name` names it; `world` is the
/// default Minecraft writes, and the only name most servers ever have.
fn world_dir(state: &AppState, id: &ServerId) -> CoreResult<std::path::PathBuf> {
    let level = state
        .registry
        .read_raw_properties(id)?
        .get("level-name")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && !v.contains(['/', '\\', ':']) && !v.contains(".."))
        .unwrap_or_else(|| "world".to_owned());
    Ok(state.registry.dir_of(id).join(level))
}

/// Backups live under the app's own metadata for this server, never inside the
/// server folder — see `backup.rs`.
fn backup_store(state: &AppState, id: &ServerId) -> std::path::PathBuf {
    state.registry.meta_dir(id).join("backups")
}

#[tauri::command]
fn list_backups(state: State<'_, AppState>, id: ServerId) -> CoreResult<Vec<backup::Backup>> {
    backup::list(&backup_store(&state, &id))
}

/// Zip the world. Refused while the server is running: Minecraft holds region
/// files open and writes them on its own schedule, so a copy taken now would
/// be a snapshot of a save in progress — which is exactly the corruption this
/// is here to recover from.
#[tauri::command]
async fn create_backup(state: State<'_, AppState>, id: ServerId) -> CoreResult<String> {
    let (world, store) = {
        if state.supervisor.is_active(&id) {
            return Err(CoreError::Precondition {
                message: "伺服器執行中，現在備份會拷到寫到一半的世界。請先停止伺服器。".into(),
            });
        }
        if held(&state.backing_up).contains(&id) {
            return Err(CoreError::Precondition {
                message: "正在自動備份，請稍候。".into(),
            });
        }
        (world_dir(&state, &id)?, backup_store(&state, &id))
    };

    // A world is big enough that zipping it on the UI thread would freeze the
    // window for a minute.
    tauri::async_runtime::spawn_blocking(move || {
        backup::create(&world, &store, KEEP_BACKUPS, "world-")
    })
    .await
    .map_err(|e| CoreError::Io {
        message: format!("備份失敗：{e}"),
    })?
}

/// Replace the world with a backup. The world being replaced is saved first.
#[tauri::command]
async fn restore_backup(state: State<'_, AppState>, id: ServerId, name: String) -> CoreResult<()> {
    let (world, store) = {
        if state.supervisor.is_active(&id) {
            return Err(CoreError::Precondition {
                message: "伺服器執行中，無法還原世界。請先停止伺服器。".into(),
            });
        }
        if held(&state.backing_up).contains(&id) {
            return Err(CoreError::Precondition {
                message: "正在自動備份，請稍候。".into(),
            });
        }
        (world_dir(&state, &id)?, backup_store(&state, &id))
    };

    tauri::async_runtime::spawn_blocking(move || backup::restore(&world, &store, &name))
        .await
        .map_err(|e| CoreError::Io {
            message: format!("還原失敗：{e}"),
        })?
}

#[tauri::command]
fn delete_backup(state: State<'_, AppState>, id: ServerId, name: String) -> CoreResult<()> {
    backup::delete(&backup_store(&state, &id), &name)
}

/// Turn a mod off without deleting it, or back on.
#[tauri::command]
fn set_mod_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ServerId,
    name: String,
    enabled: bool,
) -> CoreResult<()> {
    if state.supervisor.is_active(&id) {
        return Err(CoreError::Precondition {
            message: "伺服器執行中。模組只在啟動時載入，要停止後才能變更。".into(),
        });
    }
    state.registry.set_mod_enabled(&id, &name, enabled)?;
    emit(&app, CoreEvent::ServersChanged);
    Ok(())
}

/// Everything someone would otherwise be asked to go and find, in one zip.
///
/// The alternative is a conversation: what does the log say, which mods, what
/// version, can you paste the crash report. This collects all of it in one
/// click so that asking for help is one file rather than six questions.
///
/// The log contains player names and, on a public server, addresses. It goes
/// to a file the user then chooses to share or not — nothing is uploaded.
#[tauri::command]
fn export_diagnostics(state: State<'_, AppState>, id: ServerId) -> CoreResult<String> {
    let dir = state.registry.dir_of(&id);
    let config = state.registry.load_config(&id)?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    let mut summary = format!(
        "server: {}\nid: {}\nminecraft: {}\nforge: {}\nmemory_mb: {}\njava: {}\nimported: {}\napp: {}\n",
        config.name,
        id.0,
        config.mc_version.as_deref().unwrap_or("?"),
        config.forge_version.as_deref().unwrap_or("?"),
        config.memory_mb,
        config
            .java_path
            .as_deref()
            .map_or("(PATH)".into(), |p| p.to_string_lossy().into_owned()),
        config.external_path.is_some(),
        env!("CARGO_PKG_VERSION"),
    );

    if let Ok(mods) = state.registry.list_mods(&id) {
        summary.push_str(&format!("\nmods ({}):\n", mods.len()));
        for m in mods {
            summary.push_str(&format!(
                "  {} {} ({} bytes)\n",
                if m.enabled { "[on ]" } else { "[off]" },
                m.name,
                m.bytes
            ));
        }
    }
    files.push(("summary.txt".into(), summary.into_bytes()));

    // The three editable files, whichever of them exist.
    for file in [
        types::ServerFile::Properties,
        types::ServerFile::JvmArgs,
        types::ServerFile::RunScript,
    ] {
        if let Ok(text) = state.registry.read_file(&id, file) {
            files.push((file.filename().to_owned(), text.into_bytes()));
        }
    }

    if let Ok(bytes) = std::fs::read(dir.join("logs/latest.log")) {
        files.push(("latest.log".into(), bytes));
    }
    // Plus the console this app itself captured, which covers a failure that
    // happened before Minecraft opened its own log.
    files.push((
        "console.txt".into(),
        state
            .supervisor
            .console_since(&id, 0)
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes(),
    ));

    if let Some(report) = newest_crash_report(&dir).and_then(|p| std::fs::read(p).ok()) {
        files.push(("crash-report.txt".into(), report));
    }

    let out = state
        .downloads_dir
        .join(format!("診斷-{}-{}.zip", id.0, now_stamp()));
    backup::zip_blobs(&files, &out)?;
    // Shown in Explorer, selected, ready to drag into a chat or an issue —
    // a path in a toast is something nobody goes and finds.
    reveal(&out)?;
    Ok(out.to_string_lossy().into_owned())
}

/// Open Explorer with one file selected.
fn reveal(path: &Path) -> CoreResult<()> {
    use std::os::windows::process::CommandExt as _;
    // Raw, because Explorer does not understand `"/select,C:\a b\c"` — the
    // quoting std would apply to a path with a space in it. Windows paths
    // cannot contain a double quote, so this cannot be broken out of.
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()?;
    Ok(())
}

/// Open the newest crash report in whatever the machine reads text with.
#[tauri::command]
fn open_crash_report(state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    let report = newest_crash_report(&state.registry.dir_of(&id)).ok_or_else(|| {
        CoreError::Precondition {
            message: "這座伺服器沒有當機報告。".into(),
        }
    })?;
    std::process::Command::new("explorer").arg(report).spawn()?;
    Ok(())
}

/// The most recent crash report, whenever it was written. Unlike the
/// diagnosis, this bundle is not about one run — an old report is still the
/// most useful thing in the folder.
fn newest_crash_report(dir: &std::path::Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir.join("crash-reports")).ok()?.flatten() {
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().map_or(true, |(t, _)| modified > *t) {
            best = Some((modified, entry.path()));
        }
    }
    Some(best?.1)
}

fn now_stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ─────────────────────────────────────────────────────────────
// Commands — feedback
// ─────────────────────────────────────────────────────────────

/// Where bugs and ideas go.
const REPO: &str = "https://github.com/ouoyooojy-jack/Minecraft-Forge-Server-Manager";

/// How much text the issue body may carry.
///
/// The whole report travels in a URL, and percent-encoded Chinese costs nine
/// bytes a character. Browsers and GitHub both give up somewhere past 8 KB, and
/// they give up by silently truncating or showing an error page — so this is
/// capped here, where the UI can say so, rather than discovered by someone
/// whose long bug report vanished.
const REPORT_MAX_CHARS: usize = 1200;

/// Open a GitHub issue in the browser, pre-filled with what the user wrote.
///
/// The words are theirs. This adds the version and the environment underneath —
/// the two things every report needs and nobody remembers to include — and
/// nothing else.
///
/// Nothing is sent from here: this opens a page the user then reads, edits and
/// submits themselves. Their log is not attached and never leaves the machine
/// unless they attach the diagnostics bundle by hand.
#[tauri::command]
fn report_issue(title: String, body: String) -> CoreResult<()> {
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() || body.is_empty() {
        return Err(CoreError::Precondition {
            message: "標題和內容都要填。".into(),
        });
    }
    if title.chars().count() + body.chars().count() > REPORT_MAX_CHARS {
        return Err(CoreError::Precondition {
            message: format!("內容太長，請縮到 {REPORT_MAX_CHARS} 字以內再送出。"),
        });
    }

    let full = format!(
        "{body}

---
程式版本：{}
系統：Windows
",
        env!("CARGO_PKG_VERSION")
    );
    let url = format!(
        "{REPO}/issues/new?title={}&body={}",
        percent_encode(title),
        percent_encode(&full)
    );
    // Explorer hands an http(s) URL to the default browser, the same way it
    // hands a path to a file manager.
    std::process::Command::new("explorer").arg(&url).spawn()?;
    Ok(())
}

/// Percent-encode everything that is not unreserved.
///
/// Deliberately aggressive: `&`, `#` and `=` inside a title would otherwise
/// become query structure, and the whole point of building the URL in here is
/// that its shape is not up for negotiation.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Who is connected. Empty unless the server is fully up.
///
/// The roster is also pushed as a `Players` event on every change; this is the
/// first read, for a page that opened mid-session.
#[tauri::command]
fn list_players(state: State<'_, AppState>, id: ServerId) -> Vec<String> {
    state.supervisor.players(&id)
}

/// Why the server stopped, when it stopped on its own.
///
/// Read on demand rather than pushed with the state change: the diagnosis
/// touches the disk, and a server that crashes while nobody is looking at its
/// page should not pay for a crash report nobody reads.
#[tauri::command]
fn crash_report(state: State<'_, AppState>, id: ServerId) -> Option<crash::CrashInfo> {
    let code = match state.supervisor.state(&id) {
        types::ServerState::Crashed { code } => code,
        _ => return None,
    };
    let log: Vec<String> = state
        .supervisor
        .console_since(&id, 0)
        .into_iter()
        .map(|l| l.text)
        .collect();

    Some(crash::diagnose(
        &state.registry.dir_of(&id),
        &log,
        code,
        state
            .supervisor
            .launched_at(&id)
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
    ))
}

/// Graceful shutdown. The world saves; a force-kill follows only if the server
/// ignores `stop` past the grace period.
#[tauri::command]
fn stop_server(app: AppHandle, state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    // Stop on a crashed server means "leave it down".
    if held(&state.pending_restarts).remove(&id) {
        emit(&app, CoreEvent::ServersChanged);
        if !state.supervisor.is_active(&id) {
            return Ok(());
        }
    }
    state.supervisor.stop(&id)
}

/// End a server that is ignoring `stop`, without waiting out the grace
/// period. Whatever the world had not saved is lost.
#[tauri::command]
fn kill_server(state: State<'_, AppState>, id: ServerId) -> CoreResult<()> {
    state.supervisor.kill(&id)
}

/// Call off a restart that is still counting down.
#[tauri::command]
fn cancel_restart(app: AppHandle, state: State<'_, AppState>, id: ServerId) {
    if held(&state.pending_restarts).remove(&id) {
        announce(&app, &id, "已取消自動重啟。".into());
        emit(&app, CoreEvent::ServersChanged);
    }
}

/// The whitelist and the operators, as the server's own files have them.
#[tauri::command]
fn player_access(state: State<'_, AppState>, id: ServerId) -> CoreResult<types::PlayerAccess> {
    state.registry.player_access(&id)
}

/// Add or remove one name from the whitelist or the operators.
///
/// Through the server's own console, so only while it is up. Minecraft is the
/// one that looks the name up, writes the file, and applies it at once;
/// writing the JSON by hand would need a UUID lookup and would be overwritten
/// by the running server's copy anyway.
#[tauri::command]
fn set_player_access(
    state: State<'_, AppState>,
    id: ServerId,
    list: types::AccessList,
    name: String,
    allowed: bool,
) -> CoreResult<()> {
    // The name becomes part of a console line: nothing but what Minecraft
    // allows in a player name may reach it.
    if name.is_empty()
        || name.len() > 16
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(CoreError::Precondition {
            message: "玩家名稱只能有英文字母、數字和底線，最多 16 個字。".into(),
        });
    }
    let command = match (list, allowed) {
        (types::AccessList::Whitelist, true) => format!("whitelist add {name}"),
        (types::AccessList::Whitelist, false) => format!("whitelist remove {name}"),
        (types::AccessList::Ops, true) => format!("op {name}"),
        (types::AccessList::Ops, false) => format!("deop {name}"),
    };
    state.supervisor.send(&id, command)
}

/// Turn the whitelist on or off on a running server, where it applies at once.
#[tauri::command]
fn set_whitelist(state: State<'_, AppState>, id: ServerId, on: bool) -> CoreResult<()> {
    state.supervisor.send(
        &id,
        if on { "whitelist on" } else { "whitelist off" }.to_owned(),
    )
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
        return Err(CoreError::Precondition {
            message: "只能刪除下載資料夾裡的檔案。".into(),
        });
    }
    std::fs::remove_file(path)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Scheduled restarts
// ─────────────────────────────────────────────────────────────

/// How often uptimes are checked against their schedule.
const SCHEDULE_TICK: Duration = Duration::from_secs(30);

/// Warnings in chat before a scheduled restart, as seconds before it.
const SCHEDULE_WARNINGS: [u64; 2] = [60, 10];

/// Restart servers that have been up longer than their config asks for.
///
/// Modded servers leak: a week of uptime is a slow server, and a nightly
/// restart is the usual cure. Measured by uptime rather than a clock time, so
/// there is nothing to configure beyond "every N hours" and a server started
/// late is not restarted five minutes in.
fn spawn_scheduled_restarts(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(SCHEDULE_TICK);
        loop {
            tick.tick().await;
            let state = app.state::<AppState>();
            for id in state.supervisor.active() {
                let Ok(config) = state.registry.load_config(&id) else {
                    continue;
                };
                let due = u64::from(config.restart_every_hours) * 3600;
                let Some((uptime, _)) = state.supervisor.stats(&id) else {
                    continue;
                };
                if due == 0
                    || uptime < due
                    || !matches!(state.supervisor.state(&id), ServerState::Online)
                    || !held(&state.scheduled).insert(id.clone())
                {
                    continue;
                }
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    scheduled_restart(&app, &id).await;
                    held(&app.state::<AppState>().scheduled).remove(&id);
                });
            }
        }
    });
}

async fn scheduled_restart(app: &AppHandle, id: &ServerId) {
    let state = app.state::<AppState>();
    // The run this was scheduled for. If the user restarts it by hand during
    // the countdown, that is a fresh run with its own clock.
    let run = state.supervisor.launched_at(id);

    let mut left = SCHEDULE_WARNINGS[0];
    for warn_at in SCHEDULE_WARNINGS {
        tokio::time::sleep(Duration::from_secs(left - warn_at)).await;
        left = warn_at;
        let _ = state
            .supervisor
            .send(id, format!("say 伺服器將在 {warn_at} 秒後定時重啟。"));
    }
    tokio::time::sleep(Duration::from_secs(left)).await;

    if state.supervisor.launched_at(id) != run
        || !matches!(state.supervisor.state(id), ServerState::Online)
    {
        return;
    }
    announce(app, id, "定時重啟：正在關閉…".into());
    if state.supervisor.stop(id).is_err() {
        return;
    }

    // Down, then any backup-on-stop done, then up again.
    let deadline = Instant::now() + SHUTDOWN_GRACE + Duration::from_secs(600);
    while Instant::now() < deadline
        && (state.supervisor.is_active(id) || held(&state.backing_up).contains(id))
    {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // A stop that ended in a crash belongs to the crash path; one that never
    // ended is not something to start a second copy on top of.
    if !matches!(state.supervisor.state(id), ServerState::Stopped) {
        return;
    }
    if let Err(error) = launch(app, id).await {
        announce(app, id, format!("定時重啟失敗：{}", in_words(&error)));
    }
}

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────

/// How long to wait for servers to shut down before giving up and exiting.
///
/// Longer than the supervisor's own 90-second grace period, because that clock
/// starts when `stop` is sent and this one starts a moment earlier. A modded
/// world can take a minute to save, and exiting out from under that is the
/// exact data loss this whole path exists to prevent.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(100);

/// Bring the window back from the tray and focus it.
fn show_window(app: &AppHandle) {
    use tauri::Manager as _;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// The tray icon, which is what the app becomes while it is holding servers up.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "顯示視窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        "安全關閉並離開",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&show, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Mc Server Manager")
        .menu(&menu)
        // Left click raises the window; the menu is the right-click gesture,
        // which is what every other tray icon on Windows does.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "quit" => stop_everything_then_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Closing the window with servers running hides the app instead of exiting.
///
/// Killing this process would leave every JVM it started orphaned: on Windows
/// a child outlives its parent, so the servers would go on running with no
/// console, no controls, and no way to stop them short of Task Manager - while
/// still holding their ports. Hiding keeps the supervisor, the console
/// buffers, and the stop button alive behind the tray icon.
fn close_or_hide(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    use tauri::Manager as _;

    let active = window.state::<AppState>().supervisor.active().len();
    if active == 0 {
        return; // nothing to protect; let the app exit normally
    }

    api.prevent_close();
    let _ = window.hide();
    warn_once_about_the_tray(window.app_handle(), active);
}

/// Say where the app went - once per run.
///
/// A window that vanishes reads as "quit", and someone who believes the app
/// quit will not go looking for a tray icon to stop the server with.
fn warn_once_about_the_tray(app: &AppHandle, active: usize) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri_plugin_dialog::DialogExt as _;

    static TOLD: AtomicBool = AtomicBool::new(false);
    if TOLD.swap(true, Ordering::Relaxed) {
        return;
    }

    app.dialog()
        .message(format!(
            "還有 {active} 座伺服器在執行，程式縮到系統匣繼續看著它們。\n\n要完全關閉，請在系統匣圖示按右鍵選「安全關閉並離開」——那會先安全關閉伺服器再結束。"
        ))
        .title("仍在背景執行")
        .blocking_show();
}

/// The only exit that does not risk a world: stop everything, wait, then quit.
fn stop_everything_then_exit(app: &AppHandle) {
    use tauri::Manager as _;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        {
            let state = app.state::<AppState>();
            state.quitting.store(true, Ordering::SeqCst);
            held(&state.pending_restarts).clear();
        }
        // Each lookup is scoped so that no state guard is held across an await.
        for id in app.state::<AppState>().supervisor.active() {
            let _ = app.state::<AppState>().supervisor.stop(&id);
        }

        // ponytail: polling, because the supervisor has no "all stopped"
        // signal and one 200ms tick during shutdown does not justify a
        // broadcast channel. Swap it for one if anything else waits here.
        // The tunnel goes down first. A public address that still resolves
        // to a server being shut down is worse than one that stops answering.
        app.state::<AppState>().playit.stop();

        // A backup-on-stop that starts as the last server goes down is
        // waited for too: exiting mid-zip would throw the copy away.
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline {
            let state = app.state::<AppState>();
            if state.supervisor.active().is_empty() && held(&state.backing_up).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        app.exit(0);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be registered first: it decides whether this process is the one
        // that goes on to build a window at all.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_window(app);
        }))
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
            let supervisor = Arc::new(Supervisor::new(Arc::clone(&sink)));
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
                playit: Playit::new(playit::root(&data_dir))?,
                remotes: Remotes::new(&data_dir)?,
                running_agent: Mutex::new(None),
                restart_counts: Mutex::new(std::collections::HashMap::new()),
                pending_restarts: Mutex::new(HashSet::new()),
                backing_up: Mutex::new(HashSet::new()),
                scheduled: Mutex::new(HashSet::new()),
                quitting: AtomicBool::new(false),
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

            spawn_scheduled_restarts(app.handle().clone());
            build_tray(app.handle())?;

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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                close_or_hide(window, api);
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_servers,
            create_server,
            create_server_from_installer,
            import_server,
            delete_server,
            get_server_config,
            save_server_config,
            get_properties,
            save_properties,
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
            kill_server,
            cancel_restart,
            player_access,
            set_player_access,
            set_whitelist,
            open_crash_report,
            crash_report,
            list_players,
            list_backups,
            create_backup,
            restore_backup,
            delete_backup,
            set_mod_enabled,
            export_diagnostics,
            report_issue,
            send_console_command,
            cancel_download,
            list_downloads,
            delete_download,
            list_forge_versions,
            download_forge,
            playit_status,
            playit_install,
            playit_claim_url,
            playit_finish_claim,
            playit_cancel_claim,
            playit_start,
            playit_stop,
            playit_create_tunnel,
            playit_delete_tunnel,
            playit_set_auto,
            playit_unlink,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
