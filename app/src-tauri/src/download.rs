//! Streaming downloads with progress, cancellation, and atomic completion.
//!
//! Serves both Forge installers and JREs — same bytes, same progress bar, same
//! cancel button, so there is one implementation rather than two that drift.
//!
//! Three properties the UI depends on:
//!
//! * **A partial file is never mistaken for a finished one.** Bytes land in
//!   `<name>.part` and are renamed into place only after the last chunk. A
//!   crash, a cancel, or a dropped connection leaves no plausible-looking
//!   truncated jar for the installer to choke on later.
//! * **Progress is throttled, not per-chunk.** A 100 MB file at 64 KB chunks is
//!   ~1600 updates; at 30 Hz it is a few dozen. The extra ones are invisible
//!   and cost IPC.
//! * **Cancellation is cooperative and prompt.** The flag is checked every
//!   chunk, so a cancel lands within one chunk rather than at the end of the
//!   transfer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;

use crate::types::{
    CoreError, CoreEvent, CoreResult, DownloadId, DownloadKind, DownloadProgress, DownloadState,
    EventSink, HTTP_TIMEOUT_SEC, PROGRESS_HZ,
};

/// Weight given to the newest sample when smoothing the speed readout. Raw
/// per-interval rates jitter badly enough to make the number unreadable.
const SPEED_SMOOTHING: f64 = 0.3;

/// Cancellation flags for in-flight downloads. Shared with each transfer task,
/// which removes its own entry on the way out.
type CancelMap = Arc<Mutex<HashMap<DownloadId, Arc<AtomicBool>>>>;

pub struct Downloader {
    client: reqwest::Client,
    emit: EventSink,
    next_id: AtomicU64,
    cancels: CancelMap,
    /// Where transfers run.
    ///
    /// Held explicitly rather than relying on `tokio::spawn` finding an ambient
    /// runtime: `start` is called from synchronous Tauri commands, which run on
    /// threads with no reactor attached, and `tokio::spawn` aborts the process
    /// there. Taking the handle as a parameter makes that requirement a
    /// compile-time one instead of a crash.
    runtime: tokio::runtime::Handle,
}

impl Downloader {
    pub fn new(emit: EventSink, runtime: tokio::runtime::Handle) -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            // Applies to establishing the connection, not to the whole
            // transfer: a large installer on a slow line must not time out.
            .connect_timeout(Duration::from_secs(HTTP_TIMEOUT_SEC))
            .user_agent(concat!("McServerManager/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| CoreError::Network {
                message: e.to_string(),
            })?;
        Ok(Self {
            client,
            emit,
            next_id: AtomicU64::new(1),
            cancels: Arc::new(Mutex::new(HashMap::new())),
            runtime,
        })
    }

    /// Begin a download and return its id immediately.
    ///
    /// The transfer runs in the background and reports through `CoreEvent`.
    /// Returning the id up front is what lets the UI wire a cancel button to a
    /// download that has not finished starting yet.
    pub fn start(&self, what: DownloadKind, url: String, dest: PathBuf) -> CoreResult<DownloadId> {
        let (id, job) = self.prepare(what, url, dest)?;
        self.runtime.spawn(async move {
            let _ = job.await;
        });
        Ok(id)
    }

    /// Fetch and wait for the result.
    ///
    /// Same events, same cancellation, but the caller gets the finished path.
    ///
    /// Used by the JRE install, which has to unpack the archive it just
    /// fetched; the Forge page fires and forgets through `start` instead.
    pub async fn fetch(
        &self,
        what: DownloadKind,
        url: String,
        dest: PathBuf,
    ) -> CoreResult<PathBuf> {
        self.prepare(what, url, dest)?.1.await
    }

    /// Register a download and return the future that performs it.
    ///
    /// Split out so `start` and `fetch` cannot drift: both allocate the id the
    /// same way, both emit the same events, both clean up the same flag.
    ///
    /// One transfer at a time. Two downloads of the same version write to the
    /// same `.part` file and truncate each other, and the first one to finish
    /// renames it out from under the rest — which surfaced as a spray of
    /// half-complete bars and an "os error 2". Serialising here rather than in
    /// the UI means a double-click cannot outrun the guard: the check and the
    /// registration happen under one lock.
    fn prepare(
        &self,
        what: DownloadKind,
        url: String,
        dest: PathBuf,
    ) -> CoreResult<(
        DownloadId,
        impl std::future::Future<Output = CoreResult<PathBuf>>,
    )> {
        let id = DownloadId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut cancels = self.cancels.lock().expect("download mutex poisoned");
            if !cancels.is_empty() {
                return Err(CoreError::Precondition {
                    message: "一次只能下載一個檔案，請等目前的下載完成。".into(),
                });
            }
            cancels.insert(id, Arc::clone(&cancel));
        }

        // Announced before returning, so the UI shows the transfer — and
        // disables its button — without waiting for the first progress tick.
        (self.emit)(CoreEvent::Download(DownloadProgress {
            id,
            what: what.clone(),
            received: 0,
            total: None,
            bytes_per_sec: 0.0,
            state: DownloadState::Running,
        }));

        let client = self.client.clone();
        let emit = Arc::clone(&self.emit);
        let cancels = Arc::clone(&self.cancels);

        let job = async move {
            let outcome = {
                let emit = Arc::clone(&emit);
                transfer(
                    &client,
                    &url,
                    &dest,
                    &cancel,
                    move |progress| emit(CoreEvent::Download(progress)),
                    id,
                    what.clone(),
                )
                .await
            };

            // Dropped before the terminal event fires, so a cancel racing the
            // last chunk is a no-op rather than a write to a dead flag.
            cancels.lock().expect("download mutex poisoned").remove(&id);

            let (state, result) = match outcome {
                Ok(Some(path)) => (DownloadState::Done { path: path.clone() }, Ok(path)),
                Ok(None) => (DownloadState::Cancelled, Err(CoreError::DownloadAborted)),
                Err(error) => {
                    emit(CoreEvent::Error {
                        id: None,
                        error: error.clone(),
                    });
                    (
                        DownloadState::Failed {
                            message: error.to_string(),
                        },
                        Err(error),
                    )
                }
            };
            emit(CoreEvent::Download(DownloadProgress {
                id,
                what,
                received: 0,
                total: None,
                bytes_per_sec: 0.0,
                state,
            }));
            result
        };

        Ok((id, job))
    }

    /// Ask a running download to stop. Unknown or finished ids are ignored —
    /// a cancel that arrives one chunk too late is not an error.
    pub fn cancel(&self, id: DownloadId) {
        if let Some(flag) = self
            .cancels
            .lock()
            .expect("download mutex poisoned")
            .get(&id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

// ─────────────────────────────────────────────────────────────
// the transfer itself
// ─────────────────────────────────────────────────────────────

/// Stream `url` into `dest`.
///
/// `Ok(Some(path))` completed, `Ok(None)` cancelled, `Err` failed. Progress is
/// reported through `report` at no more than `PROGRESS_HZ`.
async fn transfer(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    report: impl Fn(DownloadProgress),
    id: DownloadId,
    what: DownloadKind,
) -> CoreResult<Option<PathBuf>> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let response = client.get(url).send().await.map_err(network_error)?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // Distinct from a transport failure: retrying will never help, the user
        // has to pick a different version.
        return Err(CoreError::VersionNotFound {
            version: what.label(),
        });
    }
    if !response.status().is_success() {
        return Err(CoreError::Network {
            message: format!("HTTP {}", response.status()),
        });
    }

    let total = response.content_length();

    // Write beside the destination so the rename below stays on one volume and
    // is therefore atomic.
    let part = part_path(dest);
    let mut file = tokio::fs::File::create(&part).await?;

    let mut received: u64 = 0;
    let mut speed = 0.0_f64;
    let mut last_emit = Instant::now();
    let mut last_bytes: u64 = 0;
    let interval = Duration::from_millis((1000 / PROGRESS_HZ.max(1)) as u64);

    let mut response = response;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Ok(None);
        }

        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(e) => {
                // Leave nothing that looks like a usable jar behind.
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(network_error(e));
            }
        };

        file.write_all(&chunk).await?;
        received += chunk.len() as u64;

        let elapsed = last_emit.elapsed();
        if elapsed >= interval {
            speed = smooth_speed(speed, received - last_bytes, elapsed);
            last_emit = Instant::now();
            last_bytes = received;
            report(DownloadProgress {
                id,
                what: what.clone(),
                received,
                total,
                bytes_per_sec: speed,
                state: DownloadState::Running,
            });
        }
    }

    file.flush().await?;
    drop(file);

    // Windows rename refuses to clobber; clear the target first.
    if dest.exists() {
        tokio::fs::remove_file(dest).await?;
    }
    tokio::fs::rename(&part, dest).await?;

    Ok(Some(dest.to_path_buf()))
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Exponentially smoothed bytes per second. The first sample seeds the average
/// directly, otherwise the readout crawls up from zero for the first second.
fn smooth_speed(previous: f64, bytes: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return previous;
    }
    let instant = bytes as f64 / secs;
    if previous == 0.0 {
        instant
    } else {
        previous * (1.0 - SPEED_SMOOTHING) + instant * SPEED_SMOOTHING
    }
}

fn network_error(e: reqwest::Error) -> CoreError {
    CoreError::Network {
        message: e.to_string(),
    }
}

impl DownloadKind {
    /// What to name in an error message.
    pub fn label(&self) -> String {
        match self {
            DownloadKind::ForgeInstaller { version } => version.clone(),
            DownloadKind::Jre { major } => format!("JRE {major}"),
            DownloadKind::PlayitAgent => "playit 代理程式".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::serve;
    use std::net::SocketAddr;

    fn kind() -> DownloadKind {
        DownloadKind::ForgeInstaller {
            version: "1.20.1-47.2.0".into(),
        }
    }

    async fn run(
        addr: SocketAddr,
        dest: &Path,
        cancel: &AtomicBool,
    ) -> (CoreResult<Option<PathBuf>>, Vec<DownloadProgress>) {
        let progress = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&progress);
        let result = transfer(
            &reqwest::Client::new(),
            &format!("http://{addr}/file.jar"),
            dest,
            cancel,
            move |p| sink.lock().unwrap().push(p),
            DownloadId(1),
            kind(),
        )
        .await;
        let collected = progress.lock().unwrap().clone();
        (result, collected)
    }

    #[tokio::test]
    async fn writes_the_body_and_reports_the_total() {
        let body = vec![b'x'; 40 * 1024];
        let addr = serve("200 OK", body.clone(), 2).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let (result, progress) = run(addr, &dest, &AtomicBool::new(false)).await;

        assert_eq!(result.unwrap(), Some(dest.clone()));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(
            progress.iter().all(|p| p.total == Some(body.len() as u64)),
            "Content-Length must reach the progress bar"
        );
    }

    #[tokio::test]
    async fn no_part_file_survives_a_completed_download() {
        let addr = serve("200 OK", vec![b'x'; 2048], 0).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        run(addr, &dest, &AtomicBool::new(false)).await.0.unwrap();

        assert!(dest.exists());
        assert!(
            !part_path(&dest).exists(),
            "the .part file must be renamed, not left beside the result"
        );
    }

    #[tokio::test]
    async fn cancelling_leaves_no_partial_file_behind() {
        // Slow enough that cancel lands mid-transfer.
        let addr = serve("200 OK", vec![b'x'; 200 * 1024], 30).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            flag.store(true, Ordering::Relaxed);
        });

        let (result, _) = run(addr, &dest, &cancel).await;

        assert_eq!(result.unwrap(), None, "cancel is not a failure");
        assert!(
            !dest.exists(),
            "a cancelled download must not appear finished"
        );
        assert!(
            !part_path(&dest).exists(),
            "the .part file must be cleaned up"
        );
    }

    #[tokio::test]
    async fn a_404_names_the_version_instead_of_blaming_the_network() {
        let addr = serve("404 Not Found", Vec::new(), 0).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let err = run(addr, &dest, &AtomicBool::new(false))
            .await
            .0
            .unwrap_err();

        match err {
            CoreError::VersionNotFound { version } => assert_eq!(version, "1.20.1-47.2.0"),
            other => panic!("404 must map to VersionNotFound, got {other:?}"),
        }
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn a_server_error_is_a_network_error() {
        let addr = serve("500 Internal Server Error", Vec::new(), 0).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let err = run(addr, &dest, &AtomicBool::new(false))
            .await
            .0
            .unwrap_err();
        assert!(matches!(err, CoreError::Network { .. }));
    }

    #[tokio::test]
    async fn an_existing_file_is_replaced() {
        let addr = serve("200 OK", b"new contents".to_vec(), 0).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");
        std::fs::write(&dest, b"old contents").unwrap();

        run(addr, &dest, &AtomicBool::new(false)).await.0.unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"new contents");
    }

    /// Exercises `start` rather than `transfer`.
    ///
    /// The unit tests above all called `transfer` directly, so the spawn inside
    /// `start` was never run — and it aborted the process on first use, because
    /// synchronous Tauri commands have no ambient Tokio runtime. Anything that
    /// only the real entry point does needs a test that goes through it.
    #[tokio::test]
    async fn start_runs_the_transfer_and_reports_a_terminal_state() {
        let body = vec![b'x'; 8 * 1024];
        let addr = serve("200 OK", body.clone(), 0).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CoreEvent>();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });

        let downloader = Downloader::new(sink, tokio::runtime::Handle::current()).unwrap();
        let id = downloader
            .start(kind(), format!("http://{addr}/f.jar"), dest.clone())
            .unwrap();
        assert_eq!(id, DownloadId(1));

        let done = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(CoreEvent::Download(p)) = rx.recv().await {
                if !matches!(p.state, DownloadState::Running) {
                    return Some(p);
                }
            }
            None
        })
        .await
        .expect("a started download must reach a terminal state")
        .unwrap();

        assert!(matches!(done.state, DownloadState::Done { .. }));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    /// `fetch` is what the create-a-server flow uses: it has to hand back the
    /// path, because the very next step runs that file.
    #[tokio::test]
    async fn fetch_awaits_and_returns_the_finished_path() {
        let body = vec![b'x'; 4096];
        let addr = serve("200 OK", body.clone(), 0).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<CoreEvent>();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let downloader = Downloader::new(sink, tokio::runtime::Handle::current()).unwrap();

        let path = downloader
            .fetch(kind(), format!("http://{addr}/f.jar"), dest.clone())
            .await
            .unwrap();

        assert_eq!(path, dest);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn fetch_surfaces_a_404_as_version_not_found() {
        let addr = serve("404 Not Found", Vec::new(), 0).await;
        let tmp = tempfile::tempdir().unwrap();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<CoreEvent>();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let downloader = Downloader::new(sink, tokio::runtime::Handle::current()).unwrap();

        let err = downloader
            .fetch(
                kind(),
                format!("http://{addr}/f.jar"),
                tmp.path().join("forge.jar"),
            )
            .await
            .unwrap_err();

        // The awaiting caller needs the actionable variant, not just "failed".
        assert!(matches!(err, CoreError::VersionNotFound { .. }));
    }

    /// A second download while one is running writes to the same `.part` file
    /// and truncates it. The guard has to hold even when the calls are back to
    /// back, which is exactly what a double-click produces.
    #[tokio::test]
    async fn only_one_download_runs_at_a_time() {
        let addr = serve("200 OK", vec![b'x'; 200 * 1024], 30).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CoreEvent>();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let downloader = Downloader::new(sink, tokio::runtime::Handle::current()).unwrap();

        let first = downloader.start(kind(), format!("http://{addr}/f.jar"), dest.clone());
        assert!(first.is_ok());

        // No await between them — the second must be refused synchronously.
        let second = downloader.start(kind(), format!("http://{addr}/f.jar"), dest.clone());
        assert!(
            matches!(second, Err(CoreError::Precondition { .. })),
            "a second concurrent download must be refused, got {second:?}"
        );

        downloader.cancel(first.unwrap());
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(CoreEvent::Download(p)) = rx.recv().await {
                if !matches!(p.state, DownloadState::Running) {
                    return;
                }
            }
        })
        .await;

        // Once the slot is free the next one is allowed.
        assert!(downloader
            .start(kind(), format!("http://{addr}/f.jar"), dest)
            .is_ok());
    }

    /// The UI disables its button off the first event; if that only arrived
    /// with the first progress tick there is a window where a second click
    /// still gets through.
    #[tokio::test]
    async fn starting_announces_the_download_immediately() {
        let addr = serve("200 OK", vec![b'x'; 64 * 1024], 20).await;
        let tmp = tempfile::tempdir().unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CoreEvent>();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let downloader = Downloader::new(sink, tokio::runtime::Handle::current()).unwrap();

        let id = downloader
            .start(
                kind(),
                format!("http://{addr}/f.jar"),
                tmp.path().join("forge.jar"),
            )
            .unwrap();

        let first = rx
            .try_recv()
            .expect("an event must be emitted before start returns");
        match first {
            CoreEvent::Download(p) => {
                assert_eq!(p.id, id);
                assert_eq!(p.received, 0);
                assert!(matches!(p.state, DownloadState::Running));
            }
            other => panic!("expected a download event, got {other:?}"),
        }

        downloader.cancel(id);
    }

    #[tokio::test]
    async fn cancel_reaches_a_started_download() {
        let addr = serve("200 OK", vec![b'x'; 200 * 1024], 30).await;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("forge.jar");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CoreEvent>();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(e);
        });

        let downloader = Downloader::new(sink, tokio::runtime::Handle::current()).unwrap();
        let id = downloader
            .start(kind(), format!("http://{addr}/f.jar"), dest.clone())
            .unwrap();

        tokio::time::sleep(Duration::from_millis(150)).await;
        downloader.cancel(id);

        let final_state = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(CoreEvent::Download(p)) = rx.recv().await {
                if !matches!(p.state, DownloadState::Running) {
                    return Some(p.state);
                }
            }
            None
        })
        .await
        .unwrap()
        .unwrap();

        assert!(matches!(final_state, DownloadState::Cancelled));
        assert!(!dest.exists());
        assert!(!part_path(&dest).exists());
    }

    #[test]
    fn part_path_appends_rather_than_replacing_the_extension() {
        let p = part_path(Path::new("/tmp/forge-1.20.1-installer.jar"));
        assert_eq!(
            p.file_name().unwrap(),
            "forge-1.20.1-installer.jar.part",
            "replacing .jar would collide across versions of the same file"
        );
    }

    #[test]
    fn speed_seeds_from_the_first_sample() {
        // Starting the EMA at zero would make the readout crawl for a second.
        assert_eq!(smooth_speed(0.0, 1000, Duration::from_secs(1)), 1000.0);

        let next = smooth_speed(1000.0, 2000, Duration::from_secs(1));
        assert!(next > 1000.0 && next < 2000.0, "later samples are smoothed");
    }

    #[test]
    fn a_zero_interval_cannot_divide_by_zero() {
        assert_eq!(smooth_speed(500.0, 100, Duration::ZERO), 500.0);
    }
}
