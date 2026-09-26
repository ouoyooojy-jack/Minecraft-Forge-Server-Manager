//! playit.gg tunnels: one public address per server, no router configuration.
//!
//! The hard part of running a Minecraft server for friends is not the server,
//! it is the network. Port forwarding needs router access most people renting a
//! flat do not have, CGNAT makes it impossible regardless, and every VPN answer
//! ends with "now everyone installs this too". playit.gg solves it from the
//! other side: an agent on this machine holds an outbound connection to their
//! network, and players connect to a hostname that never touches the router.
//!
//! ## What this module deliberately does not do
//!
//! **It does not ship the playit binary.** Their terms forbid making the
//! service available "as part of a hosted, managed, bundled, white-labeled, or
//! multi-tenant service", and their own README asks that the program only be
//! used when downloaded from an official source. So the agent is fetched from
//! playit's own GitHub release on first use, exactly the way a JRE is, and the
//! app is a launcher for it rather than a distributor of it.
//!
//! **It does not hold an account.** The claim flow below opens playit.gg in the
//! user's browser; they sign in as themselves, the secret that comes back is
//! theirs, and it is stored on this machine only. Nothing this app operates
//! sits between a player and playit. That is the line between a client and a
//! reseller, and it is the whole reason the design looks like this.
//!
//! **It runs one agent, for this machine.** Their terms separately prohibit
//! "services that remotely control, manage, instruct, or automate multiple
//! devices or agents". The remote-access listener in `agent.rs` must therefore
//! never be given a way to reach these functions on someone else's machine.
//!
//! "One" also means not minting a second one. A claim always creates a new
//! agent on the account, so when playit's own installer has already linked
//! this machine, its key is used instead of asking to link again.
//!
//! ## Why the claim flow is reimplemented here
//!
//! The agent has `claim generate`/`url`/`exchange` subcommands, but they print
//! through a terminal UI built for a human. Screen-scraping a TUI is a
//! dependency on its layout. The flow underneath is three unauthenticated JSON
//! posts and five random bytes, so it is done directly — fewer moving parts
//! than parsing someone's console output, and it cannot break when they
//! redesign the display.

use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::types::{CoreError, CoreResult, HTTP_TIMEOUT_SEC};

const API_BASE: &str = "https://api.playit.gg";

/// Every call to playit's API is a small JSON exchange. One that has not come
/// back in this long is not coming back, and the caller must not hang on it.
const API_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a `sc query` answer is reused. The status poll runs every few
/// seconds, and spawning a process each time to ask the same question is waste.
const SERVICE_CHECK_TTL: Duration = Duration::from_secs(30);

/// Pinned rather than "latest": a release whose command-line interface changed
/// would break this integration silently, and the user finds out when their
/// friends cannot connect. Bump it deliberately after checking.
///
/// 0.17.1 rather than 1.0.x on purpose — 1.0 split into a `playitd` Windows
/// service plus a client that talks to it over a named pipe, which means an
/// installer and administrator rights. 0.17.1 is one self-contained process
/// that takes a secret on the command line, which is all this needs. Both are
/// listed as supported on their download page.
const AGENT_VERSION: &str = "0.17.1";

/// The signed build, so SmartScreen and antivirus have something to verify.
fn agent_url() -> String {
    format!(
        "https://github.com/playit-cloud/playit-agent/releases/download/v{AGENT_VERSION}/playit-windows-x86_64-signed.exe"
    )
}

/// What the UI needs to draw the whole feature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayitStatus {
    /// The agent binary is on disk.
    pub installed: bool,
    /// A secret key is available — this app's own, or the one an official
    /// playit install left — so this machine is attached to a playit account.
    pub linked: bool,
    /// The agent process is up. Tunnels only carry traffic while it is.
    pub running: bool,
    /// Starting a server brings its public address up with it.
    pub auto: bool,
    /// Every tunnel on the account, whether this app made it or not.
    pub tunnels: Vec<PlayitTunnel>,
    /// Tunnels playit is still allocating. They appear with an address shortly.
    pub pending: Vec<String>,
    /// Notices from playit worth passing on verbatim — quota warnings, an
    /// unverified email blocking allocation. Not interpreted here.
    pub notices: Vec<String>,
    /// Set when the last status refresh could not reach playit. The rest of the
    /// fields are then local knowledge only.
    pub offline: Option<String>,
}

/// One tunnel, reduced to what a person needs to read out loud.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayitTunnel {
    /// What a server's config stores to find its tunnel again.
    pub id: String,
    /// The tunnel's name on playit's side. This app names tunnels after the
    /// server they were made for, but only for the person reading the list —
    /// matching is by id, since names change and can repeat.
    pub name: String,
    /// The port on this machine it forwards to, read from the same
    /// `agent_config.fields` entry the agent itself uses. `None` if playit
    /// stops sending it, in which case nothing is compared against it.
    pub local_port: Option<u16>,
    /// What a player types. Already includes a port when there is one to type.
    pub address: String,
    /// `null` while playit is still assigning; the UI shows it as pending.
    pub disabled_reason: Option<String>,
}

pub struct Playit {
    /// `<app data>/playit/playit.exe`.
    exe: PathBuf,
    /// `<app data>/playit/secret.txt`, holding the agent secret and nothing
    /// else. Separate from `agent.json` because this one is a credential for
    /// somebody else's account, and a file that holds one key is a file whose
    /// permissions and deletion are unambiguous.
    secret_path: PathBuf,
    /// `<app data>/playit/auto`. Its existence is the flag: "bring a public
    /// address up whenever a server starts". A file rather than a field in a
    /// settings struct because there is exactly one bit to store, and a bit
    /// that is either there or not cannot be half-parsed.
    auto_path: PathBuf,
    /// The agent, while it is up. Killed on drop of the app, and on request.
    child: Mutex<Option<Child>>,
    /// Where an official playit install keeps its key: the 1.0 service's
    /// machine-wide config first, then 0.17's per-user one. Read, never
    /// written — that file belongs to playit.
    machine_secrets: Vec<PathBuf>,
    client: reqwest::Client,
    /// Held across a whole `ensure`. Two servers starting together would
    /// otherwise both see "no tunnel yet" and both create one.
    ensure_lock: tokio::sync::Mutex<()>,
    /// Bumped to abandon a claim that is waiting on the browser.
    claim_generation: AtomicU64,
    /// Last `sc query` answer and when it was taken.
    service_seen: Mutex<Option<(Instant, bool)>>,
}

impl Playit {
    pub fn new(root: impl Into<PathBuf>) -> CoreResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(HTTP_TIMEOUT_SEC))
            .timeout(API_TIMEOUT)
            .user_agent(concat!("McServerManager/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| CoreError::Network {
                message: e.to_string(),
            })?;
        Ok(Self {
            exe: root.join("playit.exe"),
            secret_path: root.join("secret.txt"),
            auto_path: root.join("auto"),
            child: Mutex::new(None),
            machine_secrets: ["ProgramData", "LOCALAPPDATA"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(|dir| PathBuf::from(dir).join("playit_gg").join("playit.toml"))
                .collect(),
            client,
            ensure_lock: tokio::sync::Mutex::new(()),
            claim_generation: AtomicU64::new(0),
            service_seen: Mutex::new(None),
        })
    }

    pub fn exe_path(&self) -> PathBuf {
        self.exe.clone()
    }

    pub fn url(&self) -> String {
        agent_url()
    }

    pub fn installed(&self) -> bool {
        self.exe.is_file()
    }

    /// The key to run with: this app's own if it linked one, otherwise the one
    /// an official playit install on this machine already holds.
    fn secret(&self) -> Option<String> {
        self.own_secret().or_else(|| self.machine_secret())
    }

    fn own_secret(&self) -> Option<String> {
        let text = std::fs::read_to_string(&self.secret_path).ok()?;
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    fn machine_secret(&self) -> Option<String> {
        self.machine_secrets
            .iter()
            .find_map(|path| secret_from_toml(&std::fs::read_to_string(path).ok()?))
    }

    /// playit's own Windows service is up and running the machine's agent.
    /// It is already carrying the tunnels then, and a second process on the
    /// same key would only fight it for the connection.
    fn service_carries_it(&self) -> bool {
        if self.own_secret().is_some() || self.machine_secret().is_none() {
            return false;
        }
        let mut seen = self.service_seen.lock().expect("playit mutex poisoned");
        if let Some((at, answer)) = *seen {
            if at.elapsed() < SERVICE_CHECK_TTL {
                return answer;
            }
        }
        let mut command = Command::new("sc");
        command
            .args(["query", "playitd"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        no_window(&mut command);
        let answer = command
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("RUNNING"));
        *seen = Some((Instant::now(), answer));
        answer
    }

    pub fn linked(&self) -> bool {
        self.secret().is_some()
    }

    /// Whether starting a server should bring its public address up with it.
    pub fn auto(&self) -> bool {
        self.auto_path.exists()
    }

    pub fn set_auto(&self, on: bool) -> CoreResult<()> {
        if on {
            std::fs::write(&self.auto_path, b"")?;
        } else if self.auto_path.exists() {
            std::fs::remove_file(&self.auto_path)?;
        }
        Ok(())
    }

    pub fn running(&self) -> bool {
        let mut guard = self.child.lock().expect("playit mutex poisoned");
        match guard.as_mut() {
            // `try_wait` is the only honest answer: the process can die on its
            // own (network gone, secret revoked from the dashboard) and a
            // handle we are still holding says nothing about that.
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                _ => {
                    *guard = None;
                    false
                }
            },
            None => {
                drop(guard);
                self.service_carries_it()
            }
        }
    }

    // ─────────────────────────────────────────────────────────
    // Linking this machine to the user's playit account
    // ─────────────────────────────────────────────────────────

    /// Step one: a fresh claim code and the page the user has to visit.
    ///
    /// The code is five random bytes as hex, which is what the agent itself
    /// generates. It is not a secret — it is a one-time handle that only
    /// becomes a secret after the user approves it while signed in.
    pub fn claim_url(&self) -> (String, String) {
        let mut bytes = [0u8; 5];
        getrandom::fill(&mut bytes).expect("system randomness unavailable");
        let code: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let url = format!("https://playit.gg/claim/{code}");
        (code, url)
    }

    /// Step two: wait for the user to approve, then store the secret.
    ///
    /// Polls until approved, rejected, or `timeout` elapses. Long-lived on
    /// purpose: the user is signing in, possibly registering, in a browser —
    /// a minute is normal and a short timeout would fail the common case.
    pub async fn finish_claim(&self, code: &str, timeout: Duration) -> CoreResult<()> {
        let deadline = Instant::now() + timeout;
        let generation = self.claim_generation.load(Ordering::SeqCst);
        let cancelled = || self.claim_generation.load(Ordering::SeqCst) != generation;

        loop {
            if cancelled() {
                return Err(CoreError::Precondition {
                    message: "已取消連結 playit.gg。".into(),
                });
            }
            let setup: String = self
                .post_str(
                    "/claim/setup",
                    &serde_json::json!({
                        "code": code,
                        "agent_type": "self-managed",
                        "version": concat!("McServerManager/", env!("CARGO_PKG_VERSION")),
                    }),
                    None,
                )
                .await?;

            match setup.as_str() {
                "UserAccepted" => break,
                "UserRejected" => {
                    return Err(CoreError::Precondition {
                        message: "你在 playit.gg 上拒絕了這個連結。".into(),
                    });
                }
                // WaitingForUserVisit / WaitingForUser
                _ => {}
            }

            if Instant::now() > deadline {
                return Err(CoreError::Precondition {
                    message: "等太久了。請重新按一次連結 playit.gg。".into(),
                });
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        // Approved, but the key is minted a moment later; their own agent
        // retries this for the same reason.
        loop {
            let value = self
                .post_value(
                    "/claim/exchange",
                    &serde_json::json!({ "code": code }),
                    None,
                )
                .await;
            match value {
                Ok(v) => {
                    let key = v
                        .get("secret_key")
                        .and_then(|k| k.as_str())
                        .ok_or_else(|| CoreError::Network {
                            message: "playit 回傳的內容少了 secret_key。".into(),
                        })?;
                    std::fs::write(&self.secret_path, key)?;
                    return Ok(());
                }
                Err(e) if Instant::now() > deadline || cancelled() => return Err(e),
                Err(_) => tokio::time::sleep(Duration::from_secs(2)).await,
            }
        }
    }

    /// Abandon a `finish_claim` that is still waiting. The code it was
    /// polling is simply never used again; there is nothing to undo on
    /// playit's side until the user approves it.
    pub fn cancel_claim(&self) {
        self.claim_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Forget the secret and stop the agent.
    ///
    /// Only removes this machine's copy of the key. The agent stays on the
    /// user's playit account until they remove it there, which is theirs to do
    /// — deleting someone's account resource from a third-party app is not this
    /// app's call to make. A key borrowed from an official playit install is
    /// left alone for the same reason, so the machine stays linked through it.
    pub fn unlink(&self) -> CoreResult<()> {
        self.stop();
        if self.secret_path.exists() {
            std::fs::remove_file(&self.secret_path)?;
        }
        Ok(())
    }

    // ─────────────────────────────────────────────────────────
    // The agent process
    // ─────────────────────────────────────────────────────────

    /// Start the agent. Idempotent: already running is success, not an error.
    ///
    /// `on_line` receives the agent's own log output so it can go somewhere the
    /// user can see. When a tunnel does not work the reason is in here, and a
    /// hidden log is the difference between "it says the address is wrong" and
    /// a support thread.
    pub fn start(&self, on_line: impl Fn(String) + Send + Sync + 'static) -> CoreResult<()> {
        if self.running() {
            return Ok(());
        }
        if !self.installed() {
            return Err(CoreError::Precondition {
                message: "playit 代理程式還沒下載。".into(),
            });
        }
        let secret = self.secret().ok_or_else(|| CoreError::Precondition {
            message: "還沒連結 playit.gg 帳號。".into(),
        })?;

        let mut command = Command::new(&self.exe);
        // `-s` keeps it out of terminal-UI mode, so stdout is plain lines
        // instead of cursor escapes. `--secret` on the command line rather than
        // a config file because the config file is theirs to own, not ours.
        command
            .arg("--secret")
            .arg(&secret)
            .arg("-s")
            .arg("start")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        no_window(&mut command);

        let mut child = command.spawn().map_err(|e| CoreError::Process {
            message: format!("啟動 playit 失敗：{e}"),
        })?;
        die_with_us(&child);

        // Both streams, one reader each: the agent reports tunnel status on
        // stdout and failures on stderr, and a pipe nobody drains fills up and
        // blocks the process that is writing to it.
        let sink = std::sync::Arc::new(on_line);
        for stream in [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let sink = sink.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    sink(line);
                }
            });
        }

        *self.child.lock().expect("playit mutex poisoned") = Some(child);
        Ok(())
    }

    /// Stop the agent. Idempotent.
    ///
    /// Killed rather than signalled: Windows has no polite stop for a console
    /// child, and the agent holds no state worth flushing — the tunnels live on
    /// playit's side and reconnect when it comes back.
    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().expect("playit mutex poisoned").take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    // ─────────────────────────────────────────────────────────
    // Tunnels, via playit's API
    // ─────────────────────────────────────────────────────────

    /// Everything the UI shows, in one round trip.
    ///
    /// A failure to reach playit is reported in `offline` rather than returned
    /// as an error: the local half of this (installed, linked, running) is
    /// still true and still worth drawing.
    pub async fn status(&self) -> PlayitStatus {
        let mut status = PlayitStatus {
            installed: self.installed(),
            linked: self.linked(),
            running: self.running(),
            auto: self.auto(),
            ..Default::default()
        };
        let Some(secret) = self.secret() else {
            return status;
        };

        match self.rundata(&secret).await {
            Ok(data) => {
                status.tunnels = data
                    .get("tunnels")
                    .and_then(|t| t.as_array())
                    .map(|list| list.iter().filter_map(tunnel_from_json).collect())
                    .unwrap_or_default();
                status.pending = strings_at(&data, "pending", "name");
                status.notices = strings_at(&data, "notices", "message");
            }
            Err(e) => status.offline = Some(e.to_string()),
        }
        status
    }

    /// Create a Minecraft tunnel pointing at a local port.
    ///
    /// `minecraft-java` rather than a raw TCP tunnel because of what players
    /// have to type: a Minecraft tunnel gets an SRV record, so the address is
    /// a bare hostname. A generic tunnel gives `hostname:45231`, and a port
    /// number is the thing that gets mistyped.
    ///
    /// Returns the new tunnel's id immediately; playit allocates the address
    /// asynchronously, so that shows up in a later `status()` call.
    async fn create_tunnel(
        &self,
        secret: &str,
        agent_id: &str,
        name: &str,
        local_port: u16,
    ) -> CoreResult<String> {
        let body = serde_json::json!({
            "name": name,
            "tunnel_type": "minecraft-java",
            "port_type": "tcp",
            "port_count": 1,
            "origin": {
                "type": "agent",
                "data": {
                    "agent_id": agent_id,
                    "local_ip": IpAddr::V4(Ipv4Addr::LOCALHOST),
                    "local_port": local_port,
                }
            },
            "enabled": true,
            "alloc": null,
            "firewall_id": null,
            "proxy_protocol": null,
        });
        self.post_value("/tunnels/create", &body, Some(secret))
            .await?
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| CoreError::Network {
                message: "playit 沒有回傳新隧道的 id。".into(),
            })
    }

    /// Make sure a server has a public address, bringing up whatever is
    /// missing, and return the id of the tunnel that carries it.
    ///
    /// Idempotent, and the whole point of it: called on every server start, so
    /// it has to be safe to call when the agent is already up and the tunnel
    /// already exists. A second tunnel for one server would be a second
    /// address for it, which is worse than none.
    ///
    /// `known` is the id the server's config stored last time. A tunnel that
    /// still exists is kept and re-pointed at `local_port` if the server's port
    /// moved — same address, so nobody's saved server list goes stale. Without
    /// a stored id (a server from before ids were kept), a tunnel carrying the
    /// server's name is adopted once, and from then on the id is what matches.
    ///
    /// The address is not waited for. playit allocates it after the request
    /// returns; it appears in a later `status()`.
    pub async fn ensure(
        &self,
        known: Option<&str>,
        name: &str,
        local_port: u16,
        on_line: impl Fn(String) + Send + Sync + 'static,
    ) -> CoreResult<String> {
        let _one_at_a_time = self.ensure_lock.lock().await;
        let secret = self.require_secret()?;
        if !self.running() {
            self.start(on_line)?;
        }

        let data = self.rundata(&secret).await?;
        let agent_id = data
            .get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Network {
                message: "playit 沒有回傳 agent_id。".into(),
            })?;
        let tunnels: Vec<PlayitTunnel> = data
            .get("tunnels")
            .and_then(|t| t.as_array())
            .map(|list| list.iter().filter_map(tunnel_from_json).collect())
            .unwrap_or_default();

        let found = match known {
            Some(id) => tunnels.iter().find(|t| t.id == id),
            None => tunnels.iter().find(|t| t.name == name),
        };
        if let Some(tunnel) = found {
            if tunnel.local_port.is_some_and(|p| p != local_port) {
                self.post_value(
                    "/tunnels/update",
                    &serde_json::json!({
                        "tunnel_id": tunnel.id,
                        "local_ip": IpAddr::V4(Ipv4Addr::LOCALHOST),
                        "local_port": local_port,
                        "agent_id": agent_id,
                        "enabled": true,
                    }),
                    Some(&secret),
                )
                .await?;
            }
            return Ok(tunnel.id.clone());
        }

        // Created, but the address is still being allocated.
        let pending_ids = strings_at(&data, "pending", "id");
        let pending_names = strings_at(&data, "pending", "name");
        let pending = match known {
            Some(id) => pending_ids.iter().find(|p| *p == id),
            None => pending_names
                .iter()
                .position(|p| p == name)
                .and_then(|i| pending_ids.get(i)),
        };
        if let Some(id) = pending {
            return Ok(id.clone());
        }

        self.create_tunnel(&secret, agent_id, name, local_port).await
    }

    /// Keep the name on playit's dashboard in step with the server's.
    /// Cosmetic, so a failure is the caller's to ignore.
    pub async fn rename_tunnel(&self, id: &str, name: &str) -> CoreResult<()> {
        let secret = self.require_secret()?;
        self.post_value(
            "/tunnels/rename",
            &serde_json::json!({ "tunnel_id": id, "name": name }),
            Some(&secret),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_tunnel(&self, id: &str) -> CoreResult<()> {
        let secret = self.require_secret()?;
        self.post_value(
            "/tunnels/delete",
            &serde_json::json!({ "tunnel_id": id }),
            Some(&secret),
        )
        .await?;
        Ok(())
    }

    fn require_secret(&self) -> CoreResult<String> {
        self.secret().ok_or_else(|| CoreError::Precondition {
            message: "還沒連結 playit.gg 帳號。".into(),
        })
    }

    async fn rundata(&self, secret: &str) -> CoreResult<serde_json::Value> {
        self.post_value("/v1/agents/rundata", &serde_json::json!({}), Some(secret))
            .await
    }

    // ─────────────────────────────────────────────────────────
    // Their envelope
    // ─────────────────────────────────────────────────────────

    /// Every playit response is `{"status": "...", "data": ...}`, where a
    /// `status` other than `success` carries the reason in `data`. One place
    /// unwraps it so the callers above read as if it were plain JSON.
    async fn post_value(
        &self,
        path: &str,
        body: &serde_json::Value,
        secret: Option<&str>,
    ) -> CoreResult<serde_json::Value> {
        let mut request = self
            .client
            .post(format!("{API_BASE}{path}"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string());
        if let Some(secret) = secret {
            request = request.header(reqwest::header::AUTHORIZATION, format!("Agent-Key {secret}"));
        }

        let response = request.send().await.map_err(|e| CoreError::Network {
            message: e.to_string(),
        })?;
        let code = response.status();
        let text = response.text().await.map_err(|e| CoreError::Network {
            message: e.to_string(),
        })?;

        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| CoreError::Network {
                message: format!("playit 回了不是 JSON 的內容（HTTP {code}）。"),
            })?;

        let data = value.get("data").cloned().unwrap_or(serde_json::Value::Null);
        match value.get("status").and_then(|s| s.as_str()) {
            Some("success") => Ok(data),
            // 401 here means the stored secret no longer works — revoked from
            // the dashboard, or the agent deleted. Worth its own sentence,
            // because the fix is to link again rather than to retry.
            _ if code == reqwest::StatusCode::UNAUTHORIZED => Err(CoreError::Precondition {
                message: "playit.gg 不接受這台電腦的金鑰了，請重新連結。".into(),
            }),
            _ => Err(CoreError::Network {
                message: format!("playit 回報失敗：{data}"),
            }),
        }
    }

    /// The claim endpoints answer with a bare string in `data`.
    async fn post_str(
        &self,
        path: &str,
        body: &serde_json::Value,
        secret: Option<&str>,
    ) -> CoreResult<String> {
        let value = self.post_value(path, body, secret).await?;
        Ok(value.as_str().unwrap_or_default().to_owned())
    }
}

impl Drop for Playit {
    /// The agent is a child process on Windows, which means it outlives its
    /// parent unless something kills it. Leaving one behind would keep the
    /// tunnel up after the app is gone — a server nobody is watching, still
    /// reachable from the internet. `die_with_us` covers the exits that never
    /// get this far.
    fn drop(&mut self) {
        self.stop();
    }
}

fn no_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
}

/// Tie the agent's life to this process's, however this process ends.
///
/// `Drop` never runs when the app is ended from Task Manager or crashes, and
/// that used to leave an agent running per such exit, still serving the tunnel.
/// A job with KILL_ON_JOB_CLOSE fixes it at the OS level: its one handle is
/// never closed here, so Windows closes it when this process goes, and takes
/// every agent in the job along. One job for the app's lifetime, not one per
/// start. Best effort — if it fails, `Drop` and `stop` still work as before.
#[cfg(target_os = "windows")]
fn die_with_us(child: &Child) -> bool {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // Stored as an address because a raw HANDLE is neither Send nor Sync.
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set == 0 {
            return 0;
        }
        job as usize
    });
    job != 0 && unsafe { AssignProcessToJobObject(job as _, child.as_raw_handle() as _) != 0 }
}

#[cfg(not(target_os = "windows"))]
fn die_with_us(_child: &Child) -> bool {
    false
}

/// `secret_key = "..."` out of playit's own config. A line scan rather than a
/// TOML parser: it is one flat key in a file that is theirs to extend.
fn secret_from_toml(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "secret_key").then(|| value.trim().trim_matches('"').trim().to_owned())
        })
        .filter(|key| !key.is_empty())
}

fn tunnel_from_json(value: &serde_json::Value) -> Option<PlayitTunnel> {
    Some(PlayitTunnel {
        id: value.get("id")?.as_str()?.to_owned(),
        name: value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        address: value.get("display_address")?.as_str()?.to_owned(),
        local_port: value
            .pointer("/agent_config/fields")
            .and_then(|f| f.as_array())
            .and_then(|fields| {
                fields.iter().find_map(|f| {
                    if f.get("name")?.as_str()? != "local_port" {
                        return None;
                    }
                    f.get("value")?.as_str()?.parse().ok()
                })
            }),
        disabled_reason: value
            .get("disabled_reason")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    })
}

/// Pull one field out of every object in an array, skipping anything shaped
/// unexpectedly. Their payload gains fields between releases; a missing one
/// must not empty the list.
fn strings_at(data: &serde_json::Value, array: &str, field: &str) -> Vec<String> {
    data.get(array)
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| item.get(field).and_then(|v| v.as_str()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Where the agent and the secret live, under the app's data directory.
pub fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("playit")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blind to whatever playit install the machine running the tests has.
    fn playit() -> (tempfile::TempDir, Playit) {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = Playit::new(root(tmp.path())).unwrap();
        p.machine_secrets.clear();
        (tmp, p)
    }

    #[test]
    fn an_official_install_links_the_machine_but_a_key_of_our_own_wins() {
        let (tmp, mut p) = playit();
        let toml = tmp.path().join("playit.toml");
        p.machine_secrets = vec![tmp.path().join("missing.toml"), toml.clone()];
        assert!(!p.linked());

        std::fs::write(&toml, "# theirs\nsecret_key = \"machine-key\"\n").unwrap();
        assert_eq!(p.secret().as_deref(), Some("machine-key"));

        // Relinking after their key stopped working has to take effect.
        std::fs::write(&p.secret_path, "own-key").unwrap();
        assert_eq!(p.secret().as_deref(), Some("own-key"));

        // Unlinking forgets our key and falls back; theirs is never touched.
        p.unlink().unwrap();
        assert_eq!(p.secret().as_deref(), Some("machine-key"));
        assert!(toml.exists());
    }

    #[test]
    fn the_secret_is_read_out_of_their_config_and_nothing_else_is() {
        assert_eq!(secret_from_toml("secret_key = \"abc\"").as_deref(), Some("abc"));
        assert_eq!(secret_from_toml("  secret_key=\"abc\"  \r\n").as_deref(), Some("abc"));
        assert_eq!(secret_from_toml("other_secret_key = \"no\""), None);
        assert_eq!(secret_from_toml("secret_key = \"\""), None);
        assert_eq!(secret_from_toml(""), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_spawned_agent_joins_the_job_that_dies_with_the_app() {
        let mut child = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let joined = die_with_us(&child);
        let _ = child.kill();
        let _ = child.wait();
        assert!(joined, "without the job, a killed app leaves the agent running");
    }

    #[test]
    fn a_fresh_install_is_neither_installed_nor_linked() {
        let (_tmp, p) = playit();
        assert!(!p.installed());
        assert!(!p.linked());
        assert!(!p.running());
    }

    #[test]
    fn a_claim_code_is_ten_hex_characters_and_never_repeats() {
        let (_tmp, p) = playit();
        let (code, url) = p.claim_url();
        assert_eq!(code.len(), 10, "five bytes, hex encoded");
        assert!(code.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(url, format!("https://playit.gg/claim/{code}"));

        let (other, _) = p.claim_url();
        assert_ne!(code, other, "a reused claim code would be a reused identity");
    }

    #[test]
    fn a_whitespace_only_secret_file_does_not_count_as_linked() {
        let (_tmp, p) = playit();
        // A half-written file is the realistic failure, and treating it as a
        // link would mean starting the agent with an empty --secret.
        std::fs::write(&p.secret_path, "   \n").unwrap();
        assert!(!p.linked());

        std::fs::write(&p.secret_path, "  real-key-here\n").unwrap();
        assert_eq!(p.secret().as_deref(), Some("real-key-here"));
    }

    #[test]
    fn the_auto_flag_persists_and_clears_and_clearing_twice_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let playit = Playit::new(dir.path()).unwrap();
        assert!(!playit.auto(), "off until asked for");

        playit.set_auto(true).unwrap();
        assert!(playit.auto());
        // A second Playit over the same folder reads the same answer — this is
        // the bit that has to survive a restart.
        assert!(Playit::new(dir.path()).unwrap().auto());

        playit.set_auto(false).unwrap();
        assert!(!playit.auto());
        playit.set_auto(false).expect("clearing twice is not an error");
    }

    #[test]
    fn unlinking_removes_the_key_and_is_safe_to_repeat() {
        let (_tmp, p) = playit();
        std::fs::write(&p.secret_path, "key").unwrap();
        p.unlink().unwrap();
        assert!(!p.linked());
        p.unlink().unwrap();
    }

    #[test]
    fn starting_without_a_link_or_a_binary_says_which_is_missing() {
        let (_tmp, p) = playit();
        let err = p.start(|_| {}).unwrap_err().to_string();
        assert!(err.contains("下載"), "{err}");

        std::fs::write(p.exe_path(), b"not really an exe").unwrap();
        let err = p.start(|_| {}).unwrap_err().to_string();
        assert!(err.contains("連結"), "{err}");
    }

    #[test]
    fn tunnels_are_read_out_of_their_payload_shape() {
        let data = serde_json::json!({
            "agent_id": "a-b-c",
            "tunnels": [
                {
                    "id": "t1",
                    "name": "farmserver",
                    "display_address": "cheese.gl.joinmc.link",
                    "agent_config": { "fields": [
                        { "name": "local_ip", "value": "127.0.0.1" },
                        { "name": "local_port", "value": "25566" }
                    ] },
                    "disabled_reason": null
                },
                { "id": "t2", "name": "old", "display_address": "x.y:25566",
                  "disabled_reason": "over quota" },
                // Missing display_address: still allocating, must not abort the
                // whole list.
                { "id": "t3", "name": "half" }
            ],
            "pending": [{ "name": "waiting" }],
            "notices": [{ "message": "verify your email" }]
        });

        let tunnels: Vec<PlayitTunnel> = data["tunnels"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(tunnel_from_json)
            .collect();
        assert_eq!(tunnels.len(), 2);
        assert_eq!(tunnels[0].address, "cheese.gl.joinmc.link");
        assert_eq!(tunnels[0].local_port, Some(25566));
        assert_eq!(tunnels[1].local_port, None, "absent is unknown, not port 0");
        assert_eq!(tunnels[1].disabled_reason.as_deref(), Some("over quota"));

        assert_eq!(strings_at(&data, "pending", "name"), ["waiting"]);
        assert_eq!(strings_at(&data, "notices", "message"), ["verify your email"]);
        assert!(strings_at(&data, "nothing", "name").is_empty());
    }

    #[test]
    fn the_pinned_download_is_a_playit_release_url() {
        let url = agent_url();
        assert!(url.starts_with("https://github.com/playit-cloud/playit-agent/releases/"));
        assert!(url.contains(AGENT_VERSION));
        assert!(url.ends_with("-signed.exe"), "unsigned builds trip SmartScreen");
    }
}
