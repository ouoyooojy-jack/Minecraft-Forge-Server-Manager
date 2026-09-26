//! Letting a trusted friend edit this machine's server files.
//!
//! The other half of `remote`. That module reaches out to someone else's
//! machine; this one answers when they reach here. Both sides run the same app,
//! so the listener rides along with it and there is no second program for
//! anyone to install, start, or forget to update.
//!
//! ## What this is, plainly
//!
//! A network service that writes files on the user's computer. That is worth
//! saying in the module that implements it, because every decision below exists
//! to bound it:
//!
//! * **Off unless switched on.** No default port, no "just this once".
//! * **Three files, inside registered server folders.** The protocol has no way
//!   to name a path. It names a server this app already manages and one of the
//!   files the local editor also offers, and the path is built here.
//! * **Private networks only.** A connection from a routable address is closed
//!   before it can say anything. This is for a VPN or a LAN, and a listener that
//!   answers the open internet is a different, much worse thing.
//! * **The pairing code never crosses the wire.** The listener sends a nonce;
//!   the client proves it knows the code by returning an HMAC of that nonce.
//!   Someone watching the traffic learns nothing they can replay.
//!
//! What it deliberately is not: an authenticated remote shell, a file browser,
//! or a way to start and stop someone else's server. Those are all things the
//! person sitting at that machine should be doing.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::registry::Registry;
use crate::types::{CoreError, CoreResult, ServerFile, ServerId};

/// Whether a server is running right now. A closure rather than the
/// supervisor itself, so this module stays testable without one.
pub type IsRunning = Arc<dyn Fn(&ServerId) -> bool + Send + Sync>;

/// Not 25565: that is Minecraft's, and a manager that squats on it would stop
/// the very server it manages from starting.
pub const DEFAULT_PORT: u16 = 47285;

/// How long a connection has to finish the handshake and its one request.
const DEADLINE_SEC: u64 = 20;

/// Refuse a body larger than this. `server.properties` is a couple of KB;
/// anything at this size is not a config file.
const MAX_REQUEST_BYTES: u64 = 1 << 20;

const SETTINGS_FILE: &str = "agent.json";

type HmacSha256 = Hmac<Sha256>;

/// Persisted state of the listener.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettings {
    pub enabled: bool,
    pub port: u16,
    /// The pairing code, shown in the UI and typed into the other person's
    /// client. Regenerating it locks out everyone who had the old one.
    pub token: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: new_token(),
        }
    }
}

/// A readable pairing code: four groups of four, no vowels and no characters
/// that get misread out loud over voice chat.
pub fn new_token() -> String {
    const ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRSTVWXYZ";
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness unavailable");
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn settings_path(dir: &Path) -> PathBuf {
    dir.join(SETTINGS_FILE)
}

pub fn load_settings(dir: &Path) -> AgentSettings {
    std::fs::read_to_string(settings_path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(dir: &Path, settings: &AgentSettings) -> CoreResult<()> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| CoreError::Config {
        message: e.to_string(),
    })?;
    std::fs::write(settings_path(dir), text)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Wire protocol
// ─────────────────────────────────────────────────────────────

/// One newline-terminated JSON object each way, in this order:
///
/// ```text
/// server → { "nonce": "…" }
/// client → { "auth": "…", "op": { … } }
/// server → { "ok": true, … } | { "ok": false, "error": "…" }
/// ```
///
/// Hand-rolled rather than HTTP: there is one client, it ships with the server,
/// and a framework would be a dependency and an attack surface for a protocol
/// that fits on a screen.
#[derive(Debug, Serialize, Deserialize)]
pub struct Hello {
    pub nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    /// The servers this machine manages, so the client can offer a list.
    List,
    Read {
        server: String,
        file: ServerFile,
    },
    Write {
        server: String,
        file: ServerFile,
        text: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Authenticated {
    /// Hex HMAC-SHA256 of the server's nonce, keyed by the pairing code.
    pub auth: String,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteServer {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<Vec<RemoteServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Response {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            servers: None,
            text: None,
        }
    }
}

/// Prove knowledge of `token` without sending it.
pub fn sign(token: &str, nonce: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(token.as_bytes()).expect("hmac takes any key length");
    mac.update(nonce.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compare in constant time. A byte-at-a-time comparison leaks how much of a
/// guess was right, which is enough to find the rest one byte per attempt.
fn signature_matches(expected: &str, given: &str) -> bool {
    if expected.len() != given.len() {
        return false;
    }
    expected
        .bytes()
        .zip(given.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn hex_nonce() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Addresses this listener will talk to: loopback, RFC1918, link-local, and
/// Hamachi's 25.0.0.0/8. Everything else is hung up on before the handshake.
fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                // Hamachi hands out 25.x addresses. The block belongs to the UK
                // MoD and is not routed on the public internet, which is why
                // Hamachi picked it and why trusting it here is safe.
                || v4.octets()[0] == 25
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.segments()[0] & 0xfe00 == 0xfc00,
    }
}

// ─────────────────────────────────────────────────────────────
// Listener
// ─────────────────────────────────────────────────────────────

/// Runs until the returned handle is dropped or `stop` is called.
pub struct Agent {
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Agent {
    /// Bind and start answering. Fails if the port is taken, which the UI shows
    /// rather than silently leaving the switch on with nothing behind it.
    pub async fn start(
        registry: Arc<Registry>,
        running: IsRunning,
        settings: AgentSettings,
        on_event: Arc<dyn Fn(String) + Send + Sync>,
    ) -> CoreResult<Self> {
        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], settings.port)))
            .await
            .map_err(|e| CoreError::Precondition {
                message: format!("無法在連接埠 {} 監聽：{e}", settings.port),
            })?;

        let (shutdown, mut rx) = tokio::sync::watch::channel(false);
        let token = Arc::new(settings.token);

        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = rx.changed() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((stream, peer)) = accepted else {
                    continue;
                };

                if !is_private(peer.ip()) {
                    on_event(format!("拒絕來自 {} 的連線（不是私有網路）", peer.ip()));
                    continue;
                }

                let registry = Arc::clone(&registry);
                let running = Arc::clone(&running);
                let token = Arc::clone(&token);
                let on_event = Arc::clone(&on_event);
                tokio::spawn(async move {
                    let result = tokio::time::timeout(
                        tokio::time::Duration::from_secs(DEADLINE_SEC),
                        serve(stream, &registry, &running, &token),
                    )
                    .await;
                    match result {
                        Ok(Ok(note)) => on_event(format!("{} {note}", peer.ip())),
                        Ok(Err(e)) => on_event(format!("{} 失敗：{e}", peer.ip())),
                        Err(_) => on_event(format!("{} 逾時", peer.ip())),
                    }
                });
            }
        });

        Ok(Self { shutdown })
    }

    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One connection: greet, authenticate, do exactly one thing, reply.
///
/// Returns a line describing what happened, for the activity list — a listener
/// the user cannot watch is one they have no reason to trust.
async fn serve(
    stream: TcpStream,
    registry: &Registry,
    running: &IsRunning,
    token: &str,
) -> CoreResult<String> {
    let (read_half, mut write) = stream.into_split();
    // Capped before buffering: an endless line would otherwise be read into
    // memory until there is none left.
    let mut reader = BufReader::new(read_half.take(MAX_REQUEST_BYTES));

    let nonce = hex_nonce();
    let hello = serde_json::to_string(&Hello {
        nonce: nonce.clone(),
    })
    .map_err(|e| CoreError::Config {
        message: e.to_string(),
    })?;
    write.write_all(format!("{hello}\n").as_bytes()).await?;

    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let response = match serde_json::from_str::<Authenticated>(&line) {
        Err(_) => Response::failed("看不懂的請求"),
        Ok(message) if !signature_matches(&sign(token, &nonce), &message.auth) => {
            // Same wording for a wrong code and a malformed one: distinguishing
            // them tells an attacker which half to keep working on.
            Response::failed("配對碼不正確")
        }
        Ok(message) => handle(registry, running, message.request),
    };

    let note = match (&response.ok, &response.error) {
        (true, _) => "完成".to_owned(),
        (false, Some(e)) => e.clone(),
        (false, None) => "失敗".to_owned(),
    };

    let body = serde_json::to_string(&response).map_err(|e| CoreError::Config {
        message: e.to_string(),
    })?;
    write.write_all(format!("{body}\n").as_bytes()).await?;
    write.flush().await?;
    Ok(note)
}

fn handle(registry: &Registry, running: &IsRunning, request: Request) -> Response {
    match request {
        Request::List => match registry.list() {
            Ok(servers) => Response {
                ok: true,
                error: None,
                servers: Some(
                    servers
                        .into_iter()
                        .map(|s| RemoteServer {
                            id: s.id.0,
                            name: s.name,
                        })
                        .collect(),
                ),
                text: None,
            },
            Err(e) => Response::failed(e.to_string()),
        },

        // The path is built from a server this machine already manages plus a
        // fixed filename. Nothing in the request reaches the filesystem as text.
        Request::Read { server, file } => {
            match registry.read_file(&crate::types::ServerId(server), file) {
                Ok(text) => Response {
                    ok: true,
                    error: None,
                    servers: None,
                    text: Some(text),
                },
                Err(e) => Response::failed(e.to_string()),
            }
        }

        // Same rule as the local editor: Minecraft rewrites these files when it
        // shuts down, so an edit made while it runs would silently vanish.
        Request::Write { server, .. } if running(&ServerId(server.clone())) => {
            Response::failed("伺服器執行中，關閉時會覆寫這個檔案。請先停止伺服器再編輯。")
        }
        Request::Write { server, file, text } => {
            match registry.write_file(&crate::types::ServerId(server), file, &text) {
                Ok(()) => Response {
                    ok: true,
                    error: None,
                    servers: None,
                    text: None,
                },
                Err(e) => Response::failed(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never_running() -> IsRunning {
        Arc::new(|_| false)
    }

    #[test]
    fn a_running_server_refuses_remote_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = Registry::new(tmp.path().join("servers")).unwrap();
        let id = registry.create("Test").unwrap();
        let running: IsRunning = Arc::new(|_| true);

        let reply = handle(
            &registry,
            &running,
            Request::Write {
                server: id.0,
                file: ServerFile::Properties,
                text: "motd=x\n".into(),
            },
        );
        assert!(!reply.ok);
    }

    #[test]
    fn a_pairing_code_is_readable_and_not_reused() {
        let a = new_token();
        assert_eq!(a.len(), 19, "four groups of four, three dashes");
        assert!(a
            .chars()
            .all(|c| c == '-' || c.is_ascii_uppercase() || c.is_ascii_digit()));
        // Characters that get misheard: no O/0 confusion, no I/1, no vowels to
        // accidentally spell something.
        assert!(!a.contains(['O', 'I', 'L', 'U', 'A', 'E', '0', '1']));
        assert_ne!(a, new_token());
    }

    #[test]
    fn the_code_itself_never_has_to_be_sent() {
        let nonce = hex_nonce();
        let signature = sign("ABCD-EFGH", &nonce);
        assert!(!signature.contains("ABCD"));
        assert_eq!(signature.len(), 64);

        // A signature is only good for the nonce it answered.
        assert_ne!(signature, sign("ABCD-EFGH", &hex_nonce()));
        assert_ne!(signature, sign("WRONG-CODE", &nonce));
    }

    #[test]
    fn signature_comparison_is_length_safe() {
        let good = sign("code", "nonce");
        assert!(signature_matches(&good, &good));
        assert!(!signature_matches(&good, &good[..10]));
        assert!(!signature_matches(&good, ""));

        let mut nearly = good.clone();
        nearly.pop();
        nearly.push(if good.ends_with('a') { 'b' } else { 'a' });
        assert!(!signature_matches(&good, &nearly));
    }

    #[test]
    fn only_private_addresses_are_answered() {
        for allowed in [
            "127.0.0.1",
            "192.168.1.20",
            "10.0.0.5",
            "172.16.4.1",
            "25.63.1.2",
        ] {
            assert!(
                is_private(allowed.parse().unwrap()),
                "{allowed} is reachable only on a private network"
            );
        }
        // A listener that answers these is exposed to the internet.
        for refused in ["8.8.8.8", "203.0.113.7", "1.1.1.1"] {
            assert!(
                !is_private(refused.parse().unwrap()),
                "{refused} must be refused"
            );
        }
    }

    #[test]
    fn settings_start_switched_off() {
        let settings = AgentSettings::default();
        assert!(!settings.enabled, "a file-writing listener must be opt-in");
        assert_eq!(settings.port, DEFAULT_PORT);
        assert_ne!(settings.port, 25565, "that port belongs to the server");
    }

    #[tokio::test]
    async fn a_wrong_code_gets_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new(tmp.path().join("servers")).unwrap());
        registry.create("Test").unwrap();

        let settings = AgentSettings {
            enabled: true,
            // Port 0 asks the OS for a free one, so the test cannot collide
            // with anything else on the machine.
            port: 0,
            token: "RIGHT-CODE".into(),
        };
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let registry_for_task = Arc::clone(&registry);
        let token = settings.token.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = serve(stream, &registry_for_task, &never_running(), &token).await;
        });

        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (read_half, mut write) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();
        let hello: Hello = serde_json::from_str(&hello).unwrap();

        let forged = serde_json::json!({
            "auth": sign("WRONG-CODE", &hello.nonce),
            "op": "list",
        });
        write
            .write_all(format!("{forged}\n").as_bytes())
            .await
            .unwrap();

        let mut reply = String::new();
        reader.read_line(&mut reply).await.unwrap();
        let reply: Response = serde_json::from_str(&reply).unwrap();

        assert!(!reply.ok);
        assert!(reply.servers.is_none(), "a refused caller learns nothing");
    }

    #[tokio::test]
    async fn the_right_code_reads_and_writes_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::new(tmp.path().join("servers")).unwrap());
        let id = registry.create("Test").unwrap();

        let token = "RIGHT-CODE".to_owned();
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();

        let registry_for_task = Arc::clone(&registry);
        let token_for_task = token.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let registry = Arc::clone(&registry_for_task);
                let token = token_for_task.clone();
                tokio::spawn(async move {
                    let _ = serve(stream, &registry, &never_running(), &token).await;
                });
            }
        });

        async fn call(port: u16, token: &str, op: serde_json::Value) -> Response {
            let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let (read_half, mut write) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            let hello: Hello = serde_json::from_str(&hello).unwrap();

            let mut body = op;
            body["auth"] = sign(token, &hello.nonce).into();
            write
                .write_all(format!("{body}\n").as_bytes())
                .await
                .unwrap();

            let mut reply = String::new();
            reader.read_line(&mut reply).await.unwrap();
            serde_json::from_str(&reply).unwrap()
        }

        let listed = call(port, &token, serde_json::json!({ "op": "list" })).await;
        assert!(listed.ok);
        assert_eq!(listed.servers.unwrap()[0].name, "Test");

        let written = call(
            port,
            &token,
            serde_json::json!({
                "op": "write",
                "server": id.0,
                "file": "properties",
                "text": "motd=hello from afar\n",
            }),
        )
        .await;
        assert!(written.ok, "{:?}", written.error);

        let read = call(
            port,
            &token,
            serde_json::json!({ "op": "read", "server": id.0, "file": "properties" }),
        )
        .await;
        assert!(read.text.unwrap().contains("hello from afar"));
    }
}
