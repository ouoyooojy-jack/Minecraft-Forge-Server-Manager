//! Editing a server's files on someone else's machine, over SSH.
//!
//! The case this exists for: a friend runs the server, you are the one who
//! knows what `view-distance` does, and walking them through Notepad over voice
//! chat is worse for both of you. Their machine is reachable over the VPN the
//! group already uses; this reads and writes the same three files the local
//! editor does.
//!
//! ## Why the system `ssh`, and not an SSH library
//!
//! Windows 10 and 11 ship OpenSSH. Using it means no crypto in this binary, no
//! second TLS-shaped dependency to keep patched, and — the part that actually
//! matters day to day — the keys, `known_hosts`, and `~/.ssh/config` the user
//! already has all work untouched. A library would mean re-implementing key
//! discovery and host verification, badly.
//!
//! The cost is that authentication has to be non-interactive: `BatchMode=yes`
//! fails rather than prompting for a password on a terminal nobody can see. So
//! this is key-authentication only, which is the right answer for a machine you
//! connect to regularly anyway.
//!
//! ## Trust
//!
//! Host verification is left to `known_hosts`. An unknown host fails the
//! connection instead of being accepted silently — `trust()` exists so the user
//! can look at a fingerprint and say yes deliberately, which is the one moment
//! where the decision is theirs to make.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::agent::{self, Hello, RemoteServer, Request, Response};
use crate::types::{CoreError, CoreResult, RemoteHost, ServerFile, Transport};

/// Long enough for a VPN hop to a home connection, short enough that a machine
/// that is simply off does not hang the UI.
const CONNECT_TIMEOUT_SEC: u32 = 8;

/// Everything this app runs remotely finishes in well under this. A hang past
/// it means the far side stopped answering mid-transfer.
const COMMAND_TIMEOUT_SEC: u64 = 30;

const REMOTES_FILE: &str = "remotes.json";

pub struct Remotes {
    path: PathBuf,
    /// The saved list. Kept in memory so the UI's reads do not hit the disk,
    /// and written out whole on every change — it is a handful of entries.
    hosts: Mutex<Vec<RemoteHost>>,
}

impl Remotes {
    pub fn new(dir: &Path) -> CoreResult<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(REMOTES_FILE);
        let hosts = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Ok(Self {
            path,
            hosts: Mutex::new(hosts),
        })
    }

    pub fn list(&self) -> Vec<RemoteHost> {
        self.hosts.lock().expect("remotes mutex poisoned").clone()
    }

    pub fn get(&self, id: &str) -> CoreResult<RemoteHost> {
        self.hosts
            .lock()
            .expect("remotes mutex poisoned")
            .iter()
            .find(|h| h.id == id)
            .cloned()
            .ok_or_else(|| CoreError::Precondition {
                message: "找不到這台遠端主機。".into(),
            })
    }

    /// Add or update. An empty id means "new", and one is assigned here so the
    /// UI never has to invent one.
    pub fn save(&self, mut host: RemoteHost) -> CoreResult<RemoteHost> {
        validate(&host)?;
        let mut hosts = self.hosts.lock().expect("remotes mutex poisoned");
        if host.id.is_empty() {
            host.id = next_id(&hosts);
            hosts.push(host.clone());
        } else {
            match hosts.iter_mut().find(|h| h.id == host.id) {
                Some(existing) => *existing = host.clone(),
                None => hosts.push(host.clone()),
            }
        }
        write(&self.path, &hosts)?;
        Ok(host)
    }

    pub fn delete(&self, id: &str) -> CoreResult<()> {
        let mut hosts = self.hosts.lock().expect("remotes mutex poisoned");
        hosts.retain(|h| h.id != id);
        write(&self.path, &hosts)
    }
}

fn write(path: &Path, hosts: &[RemoteHost]) -> CoreResult<()> {
    let text = serde_json::to_string_pretty(hosts).map_err(|e| CoreError::Config {
        message: e.to_string(),
    })?;
    std::fs::write(path, text)?;
    Ok(())
}

fn next_id(hosts: &[RemoteHost]) -> String {
    let highest = hosts
        .iter()
        .filter_map(|h| h.id.strip_prefix("r")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("r{}", highest + 1)
}

/// Reject a host entry that could turn into something other than a host entry.
///
/// None of these strings ever reach a shell — arguments are passed as argv —
/// but a value starting with `-` would be read by `ssh` as an option, which is
/// its own way of turning a hostname into a command.
fn validate(host: &RemoteHost) -> CoreResult<()> {
    let bad_word = |s: &str| {
        s.trim().is_empty()
            || s.starts_with('-')
            || s.contains(char::is_whitespace)
            || s.contains(['@', ':'])
    };
    if bad_word(&host.host) {
        return Err(CoreError::Precondition {
            message: "主機位址無效。".into(),
        });
    }
    if host.port == 0 {
        return Err(CoreError::Precondition {
            message: "連接埠無效。".into(),
        });
    }
    // The two transports need different things, and demanding an ssh username
    // for a pairing-code connection is how a form teaches people to type
    // nonsense into fields that are not used.
    match host.transport {
        Transport::App => {
            if host.token.as_deref().unwrap_or("").trim().is_empty() {
                return Err(CoreError::Precondition {
                    message: "請填入對方顯示的配對碼。".into(),
                });
            }
        }
        Transport::Ssh => {
            if bad_word(&host.user) {
                return Err(CoreError::Precondition {
                    message: "使用者名稱無效。".into(),
                });
            }
            if host.dir.trim().is_empty() {
                return Err(CoreError::Precondition {
                    message: "請填入伺服器資料夾路徑。".into(),
                });
            }
        }
    }
    Ok(())
}

/// Options shared by every `ssh` and `scp` invocation.
///
/// `BatchMode` is the important one: without it a host that wants a password
/// blocks forever on a prompt written to a console this app does not have.
fn common_args(host: &RemoteHost) -> Vec<String> {
    let mut args = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_TIMEOUT_SEC}"),
    ];
    if let Some(key) = &host.key_path {
        args.push("-o".into());
        args.push("IdentitiesOnly=yes".into());
        args.push("-i".into());
        args.push(key.to_string_lossy().into_owned());
    }
    args
}

/// `user@host` — the destination, in the one shape both tools accept.
fn destination(host: &RemoteHost) -> String {
    format!("{}@{}", host.user, host.host)
}

/// The remote path of one server file, always forward-slashed.
///
/// OpenSSH on Windows accepts forward slashes, and mixing separators inside a
/// quoted path is where remote paths usually break.
fn remote_path(host: &RemoteHost, file: ServerFile) -> String {
    let dir = host.dir.trim_end_matches(['/', '\\']).replace('\\', "/");
    format!("{dir}/{}", file.filename())
}

/// Run `ssh`/`scp` and turn a non-zero exit into an error carrying stderr.
///
/// stderr is what the user needs: "Permission denied (publickey)" and "Host key
/// verification failed" are both actionable, and both are invisible if the
/// error says only that the command failed.
async fn run(program: &str, args: Vec<String>) -> CoreResult<String> {
    let mut command = Command::new(program);
    command.args(&args).stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = tokio::time::timeout(
        tokio::time::Duration::from_secs(COMMAND_TIMEOUT_SEC),
        command.output(),
    )
    .await
    .map_err(|_| CoreError::Network {
        message: "連線逾時，對方電腦可能沒開機或不在 VPN 上。".into(),
    })?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CoreError::Precondition {
                message: format!(
                    "找不到 {program}。Windows 10/11 內建 OpenSSH 用戶端，請在\
                     「設定 → 應用程式 → 選用功能」確認已安裝。"
                ),
            }
        } else {
            CoreError::Process {
                message: e.to_string(),
            }
        }
    })?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(CoreError::Network {
        message: if stderr.is_empty() {
            format!("{program} 失敗（結束碼 {:?}）", output.status.code())
        } else {
            stderr
        },
    })
}

/// Prove the connection works and the server folder is really there.
///
/// Reports what it found rather than a bare "ok": a login that succeeds against
/// the wrong folder looks identical to a working setup until the first edit
/// silently goes nowhere.
pub async fn test(host: &RemoteHost) -> CoreResult<String> {
    let mut args = common_args(host);
    args.push("-p".into());
    args.push(host.port.to_string());
    args.push(destination(host));
    // `dir /b` on Windows, `ls` on anything else; whichever runs, the output is
    // the folder listing that proves the path exists.
    args.push(format!(
        "cd \"{}\" && (dir /b || ls)",
        host.dir.replace('"', "")
    ));

    let listing = run("ssh", args).await?;
    let files: Vec<&str> = listing
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let has_server = files
        .iter()
        .any(|f| f.eq_ignore_ascii_case("run.bat") || f.eq_ignore_ascii_case("server.properties"));

    Ok(if has_server {
        format!("連線成功，找到 {} 個檔案。", files.len())
    } else {
        format!(
            "連線成功，但這個資料夾裡沒有 run.bat 或 server.properties（{} 個檔案）。",
            files.len()
        )
    })
}

/// Fetch one file's contents.
///
/// Goes through `scp` and a temporary file rather than `ssh cat`: `cat` returns
/// the bytes through a shell, which on a Windows host means whatever the console
/// code page decides, and `server.properties` with a Chinese MOTD in it comes
/// back mangled.
pub async fn read(host: &RemoteHost, file: ServerFile) -> CoreResult<String> {
    let temp = tempfile();
    let mut args = common_args(host);
    args.push("-P".into());
    args.push(host.port.to_string());
    args.push(format!("{}:{}", destination(host), remote_path(host, file)));
    args.push(temp.to_string_lossy().into_owned());

    let result = run("scp", args).await;
    let text = match result {
        Ok(_) => std::fs::read_to_string(&temp)?,
        // A file that is not there yet is not a failure — the editor opens
        // empty, exactly as it does locally.
        Err(CoreError::Network { message }) if message.contains("No such file") => String::new(),
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
    };
    let _ = std::fs::remove_file(&temp);
    Ok(text)
}

/// Replace one file's contents.
pub async fn write_file(host: &RemoteHost, file: ServerFile, text: &str) -> CoreResult<()> {
    let temp = tempfile();
    std::fs::write(&temp, text)?;

    let mut args = common_args(host);
    args.push("-P".into());
    args.push(host.port.to_string());
    args.push(temp.to_string_lossy().into_owned());
    args.push(format!("{}:{}", destination(host), remote_path(host, file)));

    let result = run("scp", args).await;
    let _ = std::fs::remove_file(&temp);
    result.map(|_| ())
}

/// The host's public key fingerprint, for the user to compare before trusting.
pub async fn fingerprint(host: &RemoteHost) -> CoreResult<String> {
    let scan = run(
        "ssh-keyscan",
        vec!["-p".into(), host.port.to_string(), host.host.clone()],
    )
    .await?;
    if scan.trim().is_empty() {
        return Err(CoreError::Network {
            message: "拿不到主機金鑰，對方的 SSH 服務可能沒開。".into(),
        });
    }

    let temp = tempfile();
    std::fs::write(&temp, &scan)?;
    let listed = run(
        "ssh-keygen",
        vec!["-lf".into(), temp.to_string_lossy().into_owned()],
    )
    .await;
    let _ = std::fs::remove_file(&temp);
    listed
}

/// Record the host in `known_hosts` so later connections verify against it.
///
/// Separate from `fingerprint` on purpose: the user is meant to look at the
/// fingerprint first. Trusting a key nobody read is the same as not checking.
pub async fn trust(host: &RemoteHost) -> CoreResult<()> {
    let scan = run(
        "ssh-keyscan",
        vec!["-p".into(), host.port.to_string(), host.host.clone()],
    )
    .await?;

    let known = dirs_known_hosts()?;
    if let Some(parent) = known.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(&known).unwrap_or_default();
    let mut out = existing.clone();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    // Skip keys already recorded, so trusting twice does not grow the file.
    for line in scan.lines().filter(|l| !l.trim().is_empty()) {
        if !existing.lines().any(|e| e.trim() == line.trim()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    std::fs::write(&known, out)?;
    Ok(())
}

fn dirs_known_hosts() -> CoreResult<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or_else(|| CoreError::Precondition {
            message: "找不到使用者資料夾。".into(),
        })?;
    Ok(PathBuf::from(home).join(".ssh").join("known_hosts"))
}

/// A scratch path in the OS temp directory. Named with the process id and a
/// counter so two transfers cannot pick the same one.
fn tempfile() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("mcsm-remote-{}-{n}.tmp", std::process::id()))
}

// ─────────────────────────────────────────────────────────────
// Talking to another copy of this app
// ─────────────────────────────────────────────────────────────

/// One request, one connection.
///
/// No pooling and no keep-alive: these are a handful of clicks a session, and a
/// held-open socket to someone's home PC is a thing to go stale, not a saving.
async fn call(host: &RemoteHost, request: Request) -> CoreResult<Response> {
    let token = host
        .token
        .clone()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| CoreError::Precondition {
            message: "這台主機還沒填配對碼。".into(),
        })?;

    let connect = tokio::net::TcpStream::connect((host.host.as_str(), host.port));
    let stream = tokio::time::timeout(
        tokio::time::Duration::from_secs(CONNECT_TIMEOUT_SEC as u64),
        connect,
    )
    .await
    .map_err(|_| CoreError::Network {
        message: "連線逾時，對方電腦可能沒開機、不在 VPN 上，或還沒開啟遠端存取。".into(),
    })?
    .map_err(|e| CoreError::Network {
        message: format!("連不上 {}:{}：{e}", host.host, host.port),
    })?;

    let (read_half, mut write) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read_half);

    // The listener speaks first, with the nonce this request has to answer.
    let mut hello = String::new();
    reader.read_line(&mut hello).await?;
    let hello: Hello = serde_json::from_str(&hello).map_err(|_| CoreError::Network {
        message: "對方回應的內容看不懂，可能不是這個 app 在監聽。".into(),
    })?;

    let mut body = serde_json::to_value(&request).map_err(|e| CoreError::Config {
        message: e.to_string(),
    })?;
    body["auth"] = agent::sign(&token, &hello.nonce).into();
    write
        .write_all(
            format!(
                "{body}
"
            )
            .as_bytes(),
        )
        .await?;
    write.flush().await?;

    let mut reply = String::new();
    reader.read_line(&mut reply).await?;
    let reply: Response = serde_json::from_str(&reply).map_err(|_| CoreError::Network {
        message: "對方的回應無法解析。".into(),
    })?;

    if reply.ok {
        Ok(reply)
    } else {
        Err(CoreError::Precondition {
            message: reply.error.unwrap_or_else(|| "遠端拒絕了這個要求。".into()),
        })
    }
}

/// The servers that machine manages.
pub async fn agent_servers(host: &RemoteHost) -> CoreResult<Vec<RemoteServer>> {
    Ok(call(host, Request::List).await?.servers.unwrap_or_default())
}

pub async fn agent_read(host: &RemoteHost, server: String, file: ServerFile) -> CoreResult<String> {
    Ok(call(host, Request::Read { server, file })
        .await?
        .text
        .unwrap_or_default())
}

pub async fn agent_write(
    host: &RemoteHost,
    server: String,
    file: ServerFile,
    text: String,
) -> CoreResult<()> {
    call(host, Request::Write { server, file, text })
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> RemoteHost {
        RemoteHost {
            id: "r1".into(),
            label: "小明".into(),
            host: "25.1.2.3".into(),
            // The ssh half is what these tests exercise; the app transport has
            // its own end-to-end tests over a real socket in `agent`.
            transport: Transport::Ssh,
            token: None,
            user: "minecraft".into(),
            port: 22,
            key_path: None,
            dir: "C:\\servers\\smp\\".into(),
        }
    }

    #[test]
    fn remote_paths_are_forward_slashed_and_not_doubled() {
        assert_eq!(
            remote_path(&host(), ServerFile::Properties),
            "C:/servers/smp/server.properties"
        );
        let unix = RemoteHost {
            dir: "/home/mc/smp".into(),
            ..host()
        };
        assert_eq!(
            remote_path(&unix, ServerFile::RunScript),
            "/home/mc/smp/run.bat"
        );
    }

    #[test]
    fn every_call_is_non_interactive() {
        let args = common_args(&host());
        assert!(
            args.windows(2).any(|w| w[1] == "BatchMode=yes"),
            "a password prompt would block on a console that does not exist"
        );
    }

    #[test]
    fn a_key_path_pins_the_identity() {
        let with_key = RemoteHost {
            key_path: Some("C:/keys/id_ed25519".into()),
            ..host()
        };
        let args = common_args(&with_key);
        assert!(args.iter().any(|a| a == "C:/keys/id_ed25519"));
        // Without this, ssh still offers every other key it can find first and
        // can exhaust the server's auth attempts before reaching this one.
        assert!(args.windows(2).any(|w| w[1] == "IdentitiesOnly=yes"));
    }

    #[test]
    fn a_hostname_cannot_smuggle_an_option() {
        // `-oProxyCommand=...` as a hostname is the classic way to turn a
        // connection into arbitrary local execution.
        for bad in ["-oProxyCommand=calc", "a b", "user@host", "host:22", " "] {
            let h = RemoteHost {
                host: bad.into(),
                ..host()
            };
            assert!(validate(&h).is_err(), "must refuse host {bad:?}");

            let u = RemoteHost {
                user: bad.into(),
                ..host()
            };
            assert!(validate(&u).is_err(), "must refuse user {bad:?}");
        }
        assert!(validate(&host()).is_ok());
    }

    #[test]
    fn saving_assigns_ids_and_updates_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let remotes = Remotes::new(tmp.path()).unwrap();

        let first = remotes
            .save(RemoteHost {
                id: String::new(),
                ..host()
            })
            .unwrap();
        assert_eq!(first.id, "r1");

        let second = remotes
            .save(RemoteHost {
                id: String::new(),
                label: "小華".into(),
                ..host()
            })
            .unwrap();
        assert_eq!(second.id, "r2");

        remotes
            .save(RemoteHost {
                label: "改名".into(),
                ..first.clone()
            })
            .unwrap();
        assert_eq!(remotes.list().len(), 2, "an edit must not add a row");
        assert_eq!(remotes.get("r1").unwrap().label, "改名");

        // Survives a restart.
        let reopened = Remotes::new(tmp.path()).unwrap();
        assert_eq!(reopened.list().len(), 2);

        remotes.delete("r1").unwrap();
        assert!(remotes.get("r1").is_err());
        assert_eq!(Remotes::new(tmp.path()).unwrap().list().len(), 1);
    }
}
