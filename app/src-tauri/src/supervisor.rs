//! Server process supervision.
//!
//! One tokio task owns each running process. That task is the only thing that
//! touches the `Child`, its pipes, and its lifecycle — commands from the UI
//! arrive over a channel, output leaves over an event sink. Nothing else in the
//! app can hold a handle to a live process, so there is no path where two
//! callers race to kill or write to the same server.
//!
//! Three properties the rest of the app depends on:
//!
//! * **`Online` means playable.** A spawned JVM is not a running server. The
//!   state only advances when the log emits Mojang's "Done (…)! For help" line.
//! * **Stopping is graceful first.** `stop` goes in over stdin so the world
//!   saves; the force-kill is a deadline behind it, not the first resort.
//! * **Output is never lost silently.** Lines land in a bounded ring buffer
//!   with monotonic sequence numbers, so eviction is detectable rather than
//!   invisible.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

use crate::logbuf::LogBuffer;
use crate::types::{
    CoreError, CoreEvent, CoreResult, EventSink, LogLine, LogStream, ServerConfig, ServerId,
    ServerState, LOG_BUFFER_LINES, PROGRESS_HZ,
};

/// How long a graceful `stop` gets before the process is killed outright.
///
/// A large modded world can take a while to save. Too short corrupts saves;
/// too long leaves the user staring at a button that appears to do nothing.
const STOP_GRACE: Duration = Duration::from_secs(90);

/// What the owning task accepts from the outside world.
enum Cmd {
    /// A console line typed by the user. Newline is added here.
    Send(String),
    /// Graceful shutdown: `stop` over stdin, force-kill after `STOP_GRACE`.
    Stop,
}

struct Running {
    state: ServerState,
    tx: mpsc::UnboundedSender<Cmd>,
    /// When this process was spawned. Uptime counts from here, so it survives
    /// a page being opened long after the server started.
    started: Instant,
    /// Who is currently connected, from the server's own join/leave lines.
    /// Nothing else reports this — the query protocol would mean opening a
    /// second connection to a server we are already reading the console of.
    players: HashSet<String>,
}

pub struct Supervisor {
    running: Arc<Mutex<HashMap<ServerId, Running>>>,
    logs: Arc<Mutex<HashMap<ServerId, LogBuffer>>>,
    emit: EventSink,
}

impl Supervisor {
    pub fn new(emit: EventSink) -> Self {
        Self {
            running: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
            emit,
        }
    }

    // ── queries ─────────────────────────────────────────────

    /// Live state, or `Stopped` for a server this supervisor has never run.
    pub fn state(&self, id: &ServerId) -> ServerState {
        self.running
            .lock()
            .expect("supervisor mutex poisoned")
            .get(id)
            .map(|r| r.state.clone())
            .unwrap_or(ServerState::Stopped)
    }

    pub fn is_active(&self, id: &ServerId) -> bool {
        !matches!(
            self.state(id),
            ServerState::Stopped | ServerState::Crashed { .. }
        )
    }

    /// Seconds of uptime and connected players, or `None` when not running.
    ///
    /// The entry outlives the process — it holds the final state so the UI can
    /// see *why* a server stopped — so this filters on the state rather than on
    /// the entry existing, or a stopped server would keep counting up.
    pub fn stats(&self, id: &ServerId) -> Option<(u64, u32)> {
        self.running
            .lock()
            .expect("supervisor mutex poisoned")
            .get(id)
            .filter(|r| !matches!(r.state, ServerState::Stopped | ServerState::Crashed { .. }))
            .map(|r| (r.started.elapsed().as_secs(), r.players.len() as u32))
    }

    pub fn console_since(&self, id: &ServerId, after_seq: u64) -> Vec<LogLine> {
        self.with_log(id, |buf| buf.since(after_seq))
    }

    pub fn forget(&self, id: &ServerId) {
        self.running
            .lock()
            .expect("supervisor mutex poisoned")
            .remove(id);
        self.logs.lock().expect("log mutex poisoned").remove(id);
    }

    fn with_log<T>(&self, id: &ServerId, f: impl FnOnce(&mut LogBuffer) -> T) -> T {
        let mut logs = self.logs.lock().expect("log mutex poisoned");
        f(logs
            .entry(id.clone())
            .or_insert_with(|| LogBuffer::new(LOG_BUFFER_LINES)))
    }

    // ── control ─────────────────────────────────────────────

    /// Launch a server. Fails if one is already active for this id.
    pub async fn start(&self, id: &ServerId, dir: &Path, config: &ServerConfig) -> CoreResult<()> {
        self.reject_if_active(id)?;

        // Forge's generated run script reads its heap settings from this file;
        // passing -Xmx on the command line would be ignored.
        write_jvm_args(dir, config.memory_mb)?;

        let launch = launch_plan(dir, config)?;
        let mut command = Command::new(&launch.program);
        command.args(&launch.args).current_dir(dir);
        // Forge's run.bat calls a bare `java`, so pointing at our runtime means
        // putting its bin directory first on the child's PATH. Passing the
        // executable path alone would only cover the plain-server.jar case.
        if let Some(path) = java_first_on_path(config) {
            command.env("PATH", path);
        }
        // A server runs until stopped; nothing here waits for it to exit.
        self.spawn(id.clone(), command, ServerState::Starting)
            .await?;
        Ok(())
    }

    /// Run the Forge installer against a server folder.
    ///
    /// Deliberately does not touch `eula.txt`: consent belongs to the checkbox
    /// the user ticked, not to a side effect of installing.
    pub async fn install(
        &self,
        id: &ServerId,
        dir: &Path,
        installer: &Path,
        config: &ServerConfig,
    ) -> CoreResult<()> {
        self.reject_if_active(id)?;
        if installer.extension().and_then(|e| e.to_str()) != Some("jar") {
            return Err(CoreError::Precondition {
                message: "請選擇有效的 Forge installer .jar 檔案。".into(),
            });
        }
        std::fs::create_dir_all(dir)?;

        let mut command = Command::new(java_binary(config));
        command
            .arg("-jar")
            .arg(installer)
            .arg("--installServer")
            .current_dir(dir);

        // Unlike `start`, this waits: the caller has follow-up work that is only
        // valid once the installer has actually written the server out.
        let done = self
            .spawn(id.clone(), command, ServerState::Installing)
            .await?;
        match done.await {
            Ok(ServerState::Stopped) => Ok(()),
            Ok(ServerState::Crashed { code }) => Err(CoreError::Install {
                code,
                message: "請查看主控台輸出。".into(),
            }),
            // The task went away without reporting — treat as a failure rather
            // than claiming an install that may not have happened.
            _ => Err(CoreError::Install {
                code: None,
                message: "安裝程序未回報結果。".into(),
            }),
        }
    }

    pub fn stop(&self, id: &ServerId) -> CoreResult<()> {
        self.send_cmd(id, Cmd::Stop)
    }

    /// Send a console command. Only meaningful once the server is `Online`;
    /// a booting server discards stdin it is not reading yet.
    pub fn send(&self, id: &ServerId, line: String) -> CoreResult<()> {
        if !matches!(self.state(id), ServerState::Online) {
            return Err(CoreError::Precondition {
                message: "伺服器尚未啟動完成。".into(),
            });
        }
        self.send_cmd(id, Cmd::Send(line))
    }

    fn reject_if_active(&self, id: &ServerId) -> CoreResult<()> {
        if self.is_active(id) {
            return Err(CoreError::Precondition {
                message: "伺服器已在執行中。".into(),
            });
        }
        Ok(())
    }

    fn send_cmd(&self, id: &ServerId, cmd: Cmd) -> CoreResult<()> {
        let running = self.running.lock().expect("supervisor mutex poisoned");
        let Some(handle) = running.get(id) else {
            return Err(CoreError::Precondition {
                message: "伺服器尚未啟動。".into(),
            });
        };
        handle.tx.send(cmd).map_err(|_| CoreError::Process {
            message: "server process is no longer accepting commands".into(),
        })
    }

    // ── the supervising task ────────────────────────────────

    /// Spawn `command` and hand it to a task that owns it until it exits.
    ///
    /// Returns a receiver that resolves to the process's final state, so a
    /// caller with follow-up work (the installer) can wait while one that just
    /// launches something (the server) can drop it.
    ///
    /// `pub(crate)` so tests can drive the whole pipeline with a stand-in
    /// process instead of a real JVM.
    pub(crate) async fn spawn(
        &self,
        id: ServerId,
        mut command: Command,
        initial: ServerState,
    ) -> CoreResult<tokio::sync::oneshot::Receiver<ServerState>> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            // Without this the JVM flashes a console window on every start.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|e| {
            // The overwhelmingly common cause is "java is not on PATH", and
            // "program not found" would send the user hunting in the wrong place.
            if e.kind() == std::io::ErrorKind::NotFound {
                CoreError::Java {
                    message: "找不到 Java。請在伺服器設定指定 java 路徑。".into(),
                }
            } else {
                CoreError::Process {
                    message: e.to_string(),
                }
            }
        })?;

        let mut stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        // Both pipes feed one channel; when it closes, both are at EOF and the
        // process is gone. That is the exit signal — no polling `try_wait`.
        let (line_tx, mut line_rx) = mpsc::unbounded_channel::<(LogStream, String)>();
        tokio::spawn(pump(stdout, LogStream::Stdout, line_tx.clone()));
        tokio::spawn(pump(stderr, LogStream::Stderr, line_tx));

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Cmd>();
        // Held by the task itself so `cmd_rx.recv()` can never resolve to
        // `None` and spin the select loop after the UI drops its handle.
        let _keepalive = cmd_tx.clone();

        // The handle has to exist before any state update, because `set_state`
        // addresses a server by its entry and drops updates for one it cannot
        // find. Insert first, then announce.
        self.insert_handle(id.clone(), cmd_tx, initial.clone());
        self.set_state(&id, initial.clone());
        self.push_log(&id, LogStream::System, format!("$ {:?}", command.as_std()));

        let running = Arc::clone(&self.running);
        let logs = Arc::clone(&self.logs);
        let emit = Arc::clone(&self.emit);
        let was_installing = matches!(initial, ServerState::Installing);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let _keepalive = _keepalive;
            let mut batch: Vec<LogLine> = Vec::new();
            let mut ticker =
                tokio::time::interval(Duration::from_millis((1000 / PROGRESS_HZ.max(1)) as u64));
            let mut stop_requested = false;
            let mut kill_at: Option<Instant> = None;

            loop {
                tokio::select! {
                    line = line_rx.recv() => {
                        let Some((stream, text)) = line else { break };
                        let is_done = is_server_ready(&text);
                        let roster_changed = update_players(&running, &id, &text);
                        let stored = {
                            let mut logs = logs.lock().expect("log mutex poisoned");
                            logs.entry(id.clone())
                                .or_insert_with(|| LogBuffer::new(LOG_BUFFER_LINES))
                                .push(stream, text)
                        };
                        batch.push(stored);
                        if is_done && !stop_requested {
                            set_state(&running, &emit, &id, ServerState::Online);
                        }
                        // The card shows a player count, and a join is the only
                        // notice we get that it changed. Rare enough (a few an
                        // hour) that a refetch costs nothing.
                        if roster_changed {
                            emit(CoreEvent::ServersChanged);
                        }
                    }

                    Some(cmd) = cmd_rx.recv() => {
                        match cmd {
                            Cmd::Send(text) => {
                                let _ = stdin.write_all(format!("{}\n", text.trim_end()).as_bytes()).await;
                                let _ = stdin.flush().await;
                            }
                            Cmd::Stop => {
                                stop_requested = true;
                                set_state(&running, &emit, &id, ServerState::Stopping);
                                let _ = stdin.write_all(b"stop\n").await;
                                let _ = stdin.flush().await;
                                kill_at = Some(Instant::now() + STOP_GRACE);
                            }
                        }
                    }

                    _ = deadline(kill_at) => {
                        kill_at = None;
                        force_kill(&mut child).await;
                    }

                    _ = ticker.tick() => flush(&emit, &id, &mut batch),
                }
            }

            flush(&emit, &id, &mut batch);
            let code = child.wait().await.ok().and_then(|s| s.code());

            // An installer that exits 0 has finished its job; a server that
            // exits on its own without being asked to has crashed.
            let final_state = if stop_requested || (was_installing && code == Some(0)) {
                ServerState::Stopped
            } else {
                ServerState::Crashed { code }
            };

            if let ServerState::Crashed { code } = &final_state {
                let text = match code {
                    Some(c) => format!("process exited unexpectedly (code {c})"),
                    None => "process was terminated".into(),
                };
                let stored = {
                    let mut logs = logs.lock().expect("log mutex poisoned");
                    logs.entry(id.clone())
                        .or_insert_with(|| LogBuffer::new(LOG_BUFFER_LINES))
                        .push(LogStream::System, text)
                };
                emit(CoreEvent::ServerLog {
                    id: id.clone(),
                    lines: vec![stored],
                });
            }

            set_state(&running, &emit, &id, final_state.clone());
            // After the state is published, so a waiter that reacts to the
            // result never observes a stale state.
            let _ = done_tx.send(final_state);
        });

        Ok(done_rx)
    }

    fn insert_handle(&self, id: ServerId, tx: mpsc::UnboundedSender<Cmd>, state: ServerState) {
        self.running
            .lock()
            .expect("supervisor mutex poisoned")
            .insert(
                id,
                Running {
                    state,
                    tx,
                    started: Instant::now(),
                    players: HashSet::new(),
                },
            );
    }

    fn set_state(&self, id: &ServerId, state: ServerState) {
        set_state(&self.running, &self.emit, id, state);
    }

    fn push_log(&self, id: &ServerId, stream: LogStream, text: String) {
        let stored = self.with_log(id, |buf| buf.push(stream, text));
        (self.emit)(CoreEvent::ServerLog {
            id: id.clone(),
            lines: vec![stored],
        });
    }
}

// ─────────────────────────────────────────────────────────────
// free functions — the testable logic
// ─────────────────────────────────────────────────────────────

/// Mojang's readiness line, e.g. `[12:34:56] [Server thread/INFO]: Done
/// (21.402s)! For help, type "help"`.
///
/// Matched on two stable fragments rather than a regex: the timestamp format,
/// the thread name, and the surrounding brackets have all changed across
/// versions, but this pair has not.
fn is_server_ready(line: &str) -> bool {
    line.contains("Done (") && line.contains("For help")
}

/// What to launch, and with what arguments.
#[derive(Debug)]
struct Launch {
    program: PathBuf,
    args: Vec<String>,
}

/// Modern Forge ships a `run.bat`/`run.sh` that resolves its own classpath;
/// older versions leave a plain `server.jar`. Prefer the script when present —
/// hand-rolling the module path for 1.17+ Forge is a losing game.
fn launch_plan(dir: &Path, config: &ServerConfig) -> CoreResult<Launch> {
    let script = dir.join(if cfg!(windows) { "run.bat" } else { "run.sh" });
    if script.is_file() {
        let text = std::fs::read_to_string(&script)?;
        if let Some(args) = java_args_from_script(&text) {
            return Ok(Launch {
                program: java_binary(config),
                args,
            });
        }
    }

    let jar = dir.join("server.jar");
    if jar.is_file() {
        return Ok(Launch {
            program: java_binary(config),
            args: vec![
                format!("-Xms{}M", config.memory_mb),
                format!("-Xmx{}M", config.memory_mb),
                "-jar".into(),
                jar.to_string_lossy().into_owned(),
                "nogui".into(),
            ],
        });
    }

    Err(CoreError::Precondition {
        message: "找不到 run.bat 或 server.jar；請先安裝 Forge。".into(),
    })
}

/// Lift the real `java` invocation out of Forge's generated run script.
///
/// The script is not run as a script. It ends in `pause`, which with a piped
/// stdin never returns: after the JVM exits, `cmd` sits there holding both
/// pipes open, so the supervisor never sees EOF and the server appears to be
/// stopping until the 90-second force-kill deadline fires. Newer scripts also
/// open with a shim that re-checks Java and would run under whatever `cmd`
/// resolves rather than the runtime we picked.
///
/// What the script is good for is the argument files, whose paths carry a
/// version number we would otherwise have to reconstruct. So: find the line
/// that references the args file, take its arguments, and launch the JVM
/// directly. That makes the server our own direct child — exit detection and
/// stdin both stop going through an intermediary.
fn java_args_from_script(text: &str) -> Option<Vec<String>> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.starts_with("REM") && !l.starts_with('#') && l.contains("_args.txt"))?;

    let mut args: Vec<String> = line
        .split_whitespace()
        .skip(1) // the `java` itself; the program comes from the config
        .filter(|a| a.starts_with('@'))
        .map(str::to_owned)
        .collect();
    if args.is_empty() {
        return None;
    }
    // `%*` / `"$@"` is where the script forwards its own arguments.
    args.push("nogui".into());
    Some(args)
}

/// The child's `PATH` with the chosen runtime's `bin` directory in front, or
/// `None` when no runtime is pinned and the inherited `PATH` should stand.
fn java_first_on_path(config: &ServerConfig) -> Option<std::ffi::OsString> {
    // A bare `java` (what the PATH lookup resolves to) has an empty parent, and
    // an empty PATH entry means the current directory on Windows — which is the
    // server folder, so a java.exe dropped in there would win. Nothing to
    // prepend in that case anyway: it already came from PATH.
    let bin = config
        .java_path
        .as_ref()?
        .parent()
        .filter(|p| !p.as_os_str().is_empty())?;
    Some(prepend_path(bin, std::env::var_os("PATH").as_deref()))
}

fn prepend_path(bin: &Path, current: Option<&std::ffi::OsStr>) -> std::ffi::OsString {
    let mut out = std::ffi::OsString::from(bin);
    if let Some(current) = current.filter(|c| !c.is_empty()) {
        out.push(if cfg!(windows) { ";" } else { ":" });
        out.push(current);
    }
    out
}

/// The runtime to run a jar with. `None` means whatever `java` resolves to,
/// which is correct for a machine that already has a suitable one installed.
fn java_binary(config: &ServerConfig) -> PathBuf {
    config.java_path.clone().unwrap_or_else(|| "java".into())
}

/// Forge's run script sources its JVM flags from `user_jvm_args.txt`, so heap
/// size has to be written there. Existing `-Xm*` lines are replaced; anything
/// else the user added is left alone.
/// Apply one log line to the roster. Returns whether it changed anything.
///
/// Vanilla prints `<name> joined the game` / `left the game`; those two lines
/// are stable across every version this app can install.
fn update_players(
    running: &Arc<Mutex<HashMap<ServerId, Running>>>,
    id: &ServerId,
    text: &str,
) -> bool {
    let Some((name, joined)) = player_event(text) else {
        return false;
    };
    let mut running = running.lock().expect("supervisor mutex poisoned");
    let Some(entry) = running.get_mut(id) else {
        return false;
    };
    if joined {
        entry.players.insert(name)
    } else {
        entry.players.remove(&name)
    }
}

/// `(name, joined)` for a join/leave line, `None` for anything else.
fn player_event(text: &str) -> Option<(String, bool)> {
    // Chat is echoed to the log too, so a player can type "x joined the game"
    // and be counted. Requiring the server's own `]: ` prefix immediately
    // before the name keeps chat out, since chat lines carry `<name>` there.
    let body = text.rsplit_once("]: ")?.1;
    let (name, rest) = body.split_once(' ')?;
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    match rest {
        "joined the game" => Some((name.to_owned(), true)),
        "left the game" => Some((name.to_owned(), false)),
        _ => None,
    }
}

fn write_jvm_args(dir: &Path, memory_mb: u32) -> CoreResult<()> {
    let path = dir.join("user_jvm_args.txt");
    if !dir
        .join(if cfg!(windows) { "run.bat" } else { "run.sh" })
        .is_file()
    {
        return Ok(()); // plain server.jar takes -Xmx on the command line
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, merge_jvm_args(&existing, memory_mb))?;
    Ok(())
}

fn merge_jvm_args(existing: &str, memory_mb: u32) -> String {
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.starts_with("-Xms") && !t.starts_with("-Xmx")
        })
        .map(str::to_owned)
        .collect();
    lines.push(format!("-Xms{memory_mb}M"));
    lines.push(format!("-Xmx{memory_mb}M"));
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Read one pipe line-by-line, tolerating non-UTF-8.
///
/// A server running under a non-UTF-8 console codepage, or a mod logging raw
/// bytes, would abort a strict `lines()` reader and silently truncate the
/// console. Lossy decoding keeps the stream alive.
async fn pump<R>(reader: R, stream: LogStream, tx: mpsc::UnboundedSender<(LogStream, String)>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&raw).trim_end().to_owned();
                if tx.send((stream, text)).is_err() {
                    break;
                }
            }
        }
    }
}

/// A future that resolves at `at`, or never when there is no deadline.
async fn deadline(at: Option<Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

/// Terminate the process and everything it spawned.
///
/// On Windows the launcher is `cmd /c run.bat`, so killing the child only kills
/// `cmd` — the JVM underneath survives, holds the world files, and blocks the
/// next start. `taskkill /T` walks the tree.
async fn force_kill(child: &mut Child) {
    #[cfg(target_os = "windows")]
    if let Some(pid) = child.id() {
        let killed = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        if matches!(killed, Ok(s) if s.success()) {
            return;
        }
    }
    let _ = child.kill().await;
}

fn flush(emit: &EventSink, id: &ServerId, batch: &mut Vec<LogLine>) {
    if batch.is_empty() {
        return;
    }
    emit(CoreEvent::ServerLog {
        id: id.clone(),
        lines: std::mem::take(batch),
    });
}

fn set_state(
    running: &Arc<Mutex<HashMap<ServerId, Running>>>,
    emit: &EventSink,
    id: &ServerId,
    state: ServerState,
) {
    {
        let mut map = running.lock().expect("supervisor mutex poisoned");
        match map.get_mut(id) {
            Some(entry) => entry.state = state.clone(),
            None => return, // handle removed (server deleted) — drop the update
        }
    }
    emit(CoreEvent::ServerState {
        id: id.clone(),
        light: state.light(),
        state,
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_joins_and_leaves_but_not_chat() {
        use super::player_event;
        let line = |t: &str| format!("[12:04:31] [Server thread/INFO]: {t}");

        assert_eq!(
            player_event(&line("Steve joined the game")),
            Some(("Steve".into(), true))
        );
        assert_eq!(
            player_event(&line("Steve left the game")),
            Some(("Steve".into(), false))
        );

        // A player typing the same words in chat must not move the count.
        assert_eq!(player_event(&line("<Alex> Steve joined the game")), None);
        assert_eq!(player_event(&line("Starting minecraft server")), None);
        assert_eq!(player_event(""), None);
    }

    use super::*;

    /// The sink must not block: these tests run on tokio's current-thread
    /// runtime, so a synchronous `recv` here would starve the very tasks the
    /// test is waiting on.
    fn collector() -> (EventSink, mpsc::UnboundedReceiver<CoreEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        (sink, rx)
    }

    /// Drain events until one satisfies `pred`, or time out.
    async fn wait_for(
        rx: &mut mpsc::UnboundedReceiver<CoreEvent>,
        pred: impl Fn(&CoreEvent) -> bool,
    ) -> Option<CoreEvent> {
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(event) = rx.recv().await {
                if pred(&event) {
                    return Some(event);
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
    }

    fn shell(script: &str) -> Command {
        let mut c = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/c", script]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        };
        c.kill_on_drop(true);
        c
    }

    /// A process that stays up long enough to be commanded, but bounds the test
    /// run if a kill ever fails to land.
    fn long_running() -> Command {
        shell(if cfg!(windows) {
            "ping -n 15 127.0.0.1 > nul"
        } else {
            "sleep 15"
        })
    }

    // ── pure logic ──────────────────────────────────────────

    #[test]
    fn readiness_line_is_recognised_across_versions() {
        assert!(is_server_ready(
            r#"[12:34:56] [Server thread/INFO]: Done (21.402s)! For help, type "help""#
        ));
        assert!(is_server_ready(
            r#"[Server thread/INFO] [minecraft/DedicatedServer]: Done (8.1s)! For help, type "help" or "?""#
        ));
        assert!(!is_server_ready(
            "[Server thread/INFO]: Preparing spawn area"
        ));
        assert!(!is_server_ready("Done loading chunks"));
    }

    #[test]
    fn the_pinned_runtime_goes_in_front_of_the_inherited_path() {
        use std::ffi::OsStr;
        let sep = if cfg!(windows) { ";" } else { ":" };

        let joined = prepend_path(Path::new("C:/jre/bin"), Some(OsStr::new("C:/windows")));
        assert_eq!(joined, OsStr::new(&format!("C:/jre/bin{sep}C:/windows")));

        // An empty or absent PATH must not leave a dangling separator.
        assert_eq!(
            prepend_path(Path::new("C:/jre/bin"), Some(OsStr::new(""))),
            OsStr::new("C:/jre/bin")
        );
        assert_eq!(
            prepend_path(Path::new("C:/jre/bin"), None),
            OsStr::new("C:/jre/bin")
        );
    }

    #[test]
    fn a_config_without_a_pinned_runtime_leaves_path_alone() {
        assert!(java_first_on_path(&ServerConfig::default()).is_none());

        // A bare `java` came from PATH already; prepending its empty parent
        // would put the server folder itself on the child's PATH.
        let from_path = ServerConfig {
            java_path: Some("java".into()),
            ..Default::default()
        };
        assert!(java_first_on_path(&from_path).is_none());
    }

    #[test]
    fn jvm_args_replace_heap_flags_and_keep_the_rest() {
        let merged = merge_jvm_args("-XX:+UseG1GC\n-Xmx1024M\n-Xms512M\n", 4096);
        assert!(merged.contains("-XX:+UseG1GC"));
        assert!(merged.contains("-Xms4096M"));
        assert!(merged.contains("-Xmx4096M"));
        assert!(!merged.contains("1024M"));
        assert!(!merged.contains("512M"));
    }

    #[test]
    fn jvm_args_from_empty_file() {
        assert_eq!(merge_jvm_args("", 2048), "-Xms2048M\n-Xmx2048M\n");
    }

    #[test]
    fn the_run_script_yields_a_direct_java_invocation() {
        // Forge 1.20.1's script, verbatim in shape.
        let classic = "@echo off
             REM Add custom JVM arguments to the user_jvm_args.txt
             java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.20.1-47.0.0/win_args.txt %*
             pause
";
        assert_eq!(
            java_args_from_script(classic),
            Some(vec![
                "@user_jvm_args.txt".into(),
                "@libraries/net/minecraftforge/forge/1.20.1-47.0.0/win_args.txt".into(),
                "nogui".into(),
            ])
        );

        // 1.21's script runs a shim check first; that line must not be taken
        // for the launch, and the REM lines mentioning the file must not either.
        let modern = "@echo off
             java -jar forge-1.21.11-61.1.14-shim.jar --onlyCheckJava
             REM Add custom program arguments to the next line before the %*
             java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.11-61.1.14/win_args.txt %*
             :exit
             pause
";
        assert_eq!(
            java_args_from_script(modern),
            Some(vec![
                "@user_jvm_args.txt".into(),
                "@libraries/net/minecraftforge/forge/1.21.11-61.1.14/win_args.txt".into(),
                "nogui".into(),
            ])
        );

        assert_eq!(
            java_args_from_script(
                "@echo off
pause
"
            ),
            None
        );
    }

    #[test]
    fn launch_prefers_the_run_script_over_the_jar() {
        let dir = tempfile::tempdir().unwrap();
        let config = ServerConfig::default();
        let script = dir
            .path()
            .join(if cfg!(windows) { "run.bat" } else { "run.sh" });

        std::fs::write(dir.path().join("server.jar"), b"").unwrap();
        let jar_plan = launch_plan(dir.path(), &config).unwrap();
        assert!(jar_plan.args.iter().any(|a| a.contains("server.jar")));
        assert!(jar_plan.args.iter().any(|a| a == "-Xmx2048M"));

        std::fs::write(
            &script,
            b"java @user_jvm_args.txt @libraries/x/win_args.txt %*
pause
",
        )
        .unwrap();
        let script_plan = launch_plan(dir.path(), &config).unwrap();
        assert_eq!(
            script_plan.args,
            vec![
                "@user_jvm_args.txt".to_owned(),
                "@libraries/x/win_args.txt".to_owned(),
                "nogui".to_owned(),
            ],
            "the script supplies arguments, never the program"
        );

        // A script we cannot read falls through to the jar rather than failing.
        std::fs::write(
            &script,
            b"@echo off
pause
",
        )
        .unwrap();
        assert!(launch_plan(dir.path(), &config)
            .unwrap()
            .args
            .iter()
            .any(|a| a.contains("server.jar")));
    }

    #[test]
    fn launch_without_forge_installed_is_a_precondition_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = launch_plan(dir.path(), &ServerConfig::default()).unwrap_err();
        assert!(matches!(err, CoreError::Precondition { .. }));
    }

    // ── the pipeline ────────────────────────────────────────

    #[tokio::test]
    async fn ready_line_promotes_to_online_and_a_bare_exit_is_a_crash() {
        let (sink, mut rx) = collector();
        let sup = Supervisor::new(sink);
        let id = ServerId("t".into());

        sup.spawn(
            id.clone(),
            shell(r#"echo Done (1.0s)! For help, type "help""#),
            ServerState::Starting,
        )
        .await
        .unwrap();

        assert!(
            wait_for(&mut rx, |e| matches!(
                e,
                CoreEvent::ServerState {
                    state: ServerState::Online,
                    ..
                }
            ))
            .await
            .is_some(),
            "readiness line must promote the server to Online"
        );

        // Nobody asked it to stop, so exiting on its own is a crash.
        assert!(
            wait_for(&mut rx, |e| matches!(
                e,
                CoreEvent::ServerState {
                    state: ServerState::Crashed { .. },
                    ..
                }
            ))
            .await
            .is_some(),
            "an unrequested exit must surface as Crashed"
        );
    }

    #[tokio::test]
    async fn output_reaches_the_ring_buffer_with_sequence_numbers() {
        let (sink, mut rx) = collector();
        let sup = Supervisor::new(sink);
        let id = ServerId("t".into());

        sup.spawn(id.clone(), shell("echo alpha"), ServerState::Starting)
            .await
            .unwrap();

        // Must match the process's own output, not the `$ …` system line that
        // echoes the command — that one also contains "alpha".
        wait_for(&mut rx, |e| {
            matches!(e, CoreEvent::ServerLog { lines, .. }
                if lines.iter().any(|l| l.stream == LogStream::Stdout && l.text.contains("alpha")))
        })
        .await
        .expect("stdout must be forwarded");

        let held = sup.console_since(&id, 0);
        assert!(held
            .iter()
            .any(|l| l.stream == LogStream::Stdout && l.text.contains("alpha")));
        assert!(held.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    #[tokio::test]
    async fn force_kill_takes_down_the_whole_process_tree() {
        // The reason this is not `child.kill()`: killing the shell leaves the
        // process it launched alive, holding the world files. `taskkill /T`
        // walks the tree.
        let mut child = shell(if cfg!(windows) {
            "ping -n 30 127.0.0.1 > nul"
        } else {
            "sleep 30"
        })
        .spawn()
        .unwrap();

        force_kill(&mut child).await;

        let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .expect("a force-killed process must not outlive the call");
        assert!(status.is_ok());
    }

    #[tokio::test]
    async fn starting_twice_is_refused() {
        let (sink, _rx) = collector();
        let sup = Supervisor::new(sink);
        let id = ServerId("t".into());

        sup.spawn(id.clone(), long_running(), ServerState::Starting)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let err = sup.reject_if_active(&id).unwrap_err();
        assert!(matches!(err, CoreError::Precondition { .. }));

        // No cleanup call: the command carries kill_on_drop, and `long_running`
        // is a 15-second ping either way.
    }

    #[tokio::test]
    async fn console_input_is_refused_before_the_server_is_online() {
        let (sink, _rx) = collector();
        let sup = Supervisor::new(sink);
        let id = ServerId("t".into());

        sup.spawn(id.clone(), long_running(), ServerState::Starting)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let err = sup.send(&id, "list".into()).unwrap_err();
        assert!(matches!(err, CoreError::Precondition { .. }));

        // No cleanup call: the command carries kill_on_drop, and `long_running`
        // is a 15-second ping either way.
    }

    /// The installer path waits on this receiver, so a zero exit has to arrive
    /// as `Stopped` and anything else as `Crashed` — that mapping is what turns
    /// into "install succeeded" or "install failed" for the caller.
    #[tokio::test]
    async fn spawn_reports_the_final_state_to_a_waiter() {
        let (sink, _rx) = collector();
        let sup = Supervisor::new(sink);

        let ok = sup
            .spawn(
                ServerId("ok".into()),
                shell("exit 0"),
                ServerState::Installing,
            )
            .await
            .unwrap();
        assert!(
            matches!(ok.await, Ok(ServerState::Stopped)),
            "an installer that exits 0 has done its job"
        );

        let bad = sup
            .spawn(
                ServerId("bad".into()),
                shell("exit 3"),
                ServerState::Installing,
            )
            .await
            .unwrap();
        assert!(
            matches!(bad.await, Ok(ServerState::Crashed { code: Some(3) })),
            "a non-zero exit must surface the code, not be swallowed"
        );
    }

    #[tokio::test]
    async fn install_rejects_anything_that_is_not_a_jar() {
        let (sink, _rx) = collector();
        let sup = Supervisor::new(sink);
        let dir = tempfile::tempdir().unwrap();

        let err = sup
            .install(
                &ServerId("t".into()),
                dir.path(),
                &dir.path().join("installer.zip"),
                &ServerConfig::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Precondition { .. }));
    }

    #[tokio::test]
    async fn a_missing_binary_reports_a_java_error() {
        let (sink, _rx) = collector();
        let sup = Supervisor::new(sink);
        let err = sup
            .spawn(
                ServerId("t".into()),
                Command::new("definitely-not-a-real-binary-xyz"),
                ServerState::Starting,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, CoreError::Java { .. }),
            "a missing launcher must point at Java, not at a generic spawn failure"
        );
    }
}
