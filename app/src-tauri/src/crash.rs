//! Why a server stopped, in a sentence someone can act on.
//!
//! A stack trace is not an answer. Nearly every real failure here is one of
//! five things — the port is taken, the heap is too small, the Java is wrong,
//! a mod is missing its dependency, the EULA was never accepted — and each of
//! those has a fix that is one click away in this app. So the job is not to
//! render the exception; it is to recognise it and point at the fix.
//!
//! Anything unrecognised falls back to showing the text verbatim. Guessing
//! would be worse than saying "here is what it said".

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// What the UI should offer as the next step.
///
/// A variant per destination in the app rather than a free-text instruction:
/// "把記憶體調高" is advice, a button that opens the memory field is a fix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fix {
    /// Raise `-Xmx`, in the general section of the settings tab.
    Memory,
    /// Change `server-port`, in the connection section.
    Port,
    /// Download a JRE. `major` is what the class file actually wanted.
    Java { major: u8 },
    /// Open the mod list.
    Mods,
    /// Accept the EULA.
    Eula,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashInfo {
    /// One line, in Chinese, saying what went wrong.
    pub headline: String,
    /// The evidence — the lines this was read off. Shown in a monospace block,
    /// never parsed by the UI.
    pub detail: String,
    pub fix: Option<Fix>,
    /// The crash report on disk, when there is one to open.
    pub path: Option<String>,
}

/// How many lines of raw text to keep when nothing matched.
const FALLBACK_LINES: usize = 14;

/// Work out what killed a server.
///
/// `log_tail` is the console buffer, oldest first. `started` is when this run
/// began — crash reports from *earlier* runs are still sitting in the folder,
/// and blaming today's failure on a file from March would be worse than
/// offering no diagnosis at all.
pub fn diagnose(dir: &Path, log_tail: &[String], code: Option<i32>, started: SystemTime) -> CrashInfo {
    let report = newest_report(dir, started);
    let (source, path) = match &report {
        Some((path, text)) => (text.as_str(), Some(path.to_string_lossy().into_owned())),
        None => ("", None),
    };

    let log = log_tail.join("\n");
    // The report is the better evidence when it exists; the log always has
    // something, because the failure had to be printed before the exit.
    let haystack = if source.is_empty() { &log } else { source };

    let (headline, fix) = classify(haystack);
    CrashInfo {
        headline: headline.unwrap_or_else(|| match code {
            Some(c) => format!("伺服器意外結束（結束碼 {c}）。"),
            None => "伺服器意外結束。".to_owned(),
        }),
        detail: evidence(haystack, log_tail),
        fix,
        path,
    }
}

/// Match the text against the failures worth naming. `None` means unrecognised.
fn classify(text: &str) -> (Option<String>, Option<Fix>) {
    let has = |needle: &str| text.contains(needle);

    if has("BindException") || has("Address already in use") || has("already running on that port")
    {
        return (
            Some("連接埠已被占用。另一座伺服器或別的程式正在用這個埠。".to_owned()),
            Some(Fix::Port),
        );
    }
    if has("OutOfMemoryError") || has("GC overhead limit exceeded") {
        return (
            Some("記憶體不足。伺服器要的比 -Xmx 給的多。".to_owned()),
            Some(Fix::Memory),
        );
    }
    if has("UnsupportedClassVersionError") {
        let major = wanted_java_major(text);
        return (
            Some(match major {
                Some(m) => format!("Java 版本不對，這些檔案要 Java {m}。"),
                None => "Java 版本不對，現在這個太舊了。".to_owned(),
            }),
            Some(Fix::Java {
                major: major.unwrap_or(21),
            }),
        );
    }
    if has("agree to the EULA") || has("eula.txt") && has("false") {
        return (
            Some("還沒同意 Minecraft EULA。".to_owned()),
            Some(Fix::Eula),
        );
    }
    if has("Missing or unsupported mandatory dependencies") || has("MissingModsException") {
        return (
            Some("有模組缺少它需要的前置模組。".to_owned()),
            Some(Fix::Mods),
        );
    }
    if has("Mixin apply failed") || has("InvalidInjectionException") || has("MixinApplyError") {
        return (
            Some("模組衝突，其中一個改不到它預期的程式碼。".to_owned()),
            Some(Fix::Mods),
        );
    }
    if has("Failed to load mods") || has("LoadingFailedException") {
        return (Some("模組載入失敗。".to_owned()), Some(Fix::Mods));
    }
    (None, None)
}

/// `class file version 65.0` — the JVM's own way of saying which Java it wanted.
/// Class file 65 is Java 21, and every version before it counts down by one.
fn wanted_java_major(text: &str) -> Option<u8> {
    let after = text.split("class file version ").nth(1)?;
    let major: u32 = after
        .split(['.', ' ', ')'])
        .next()?
        .trim()
        .parse()
        .ok()?;
    u8::try_from(major.checked_sub(44)?).ok()
}

/// The lines worth showing under the headline.
///
/// A Minecraft crash report opens with a `Description:` and the exception, and
/// that pair is the whole story. Without a report, the tail of the log is the
/// best available — the failure is always the last thing printed.
fn evidence(haystack: &str, log_tail: &[String]) -> String {
    if let Some(rest) = haystack.split_once("Description: ") {
        let lines: Vec<&str> = rest
            .1
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(FALLBACK_LINES)
            .collect();
        if !lines.is_empty() {
            return lines.join("\n");
        }
    }

    let start = log_tail.len().saturating_sub(FALLBACK_LINES);
    log_tail[start..].join("\n")
}

/// The newest `crash-reports/crash-*.txt` written since this run started.
fn newest_report(dir: &Path, started: SystemTime) -> Option<(std::path::PathBuf, String)> {
    let mut best: Option<(SystemTime, std::path::PathBuf)> = None;

    for entry in std::fs::read_dir(dir.join("crash-reports")).ok()?.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "txt") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        // Reports from previous runs are still in the folder. Only a file
        // written after this launch can describe this launch.
        if modified < started {
            continue;
        }
        if best.as_ref().map_or(true, |(t, _)| modified > *t) {
            best = Some((modified, path));
        }
    }

    let (_, path) = best?;
    let text = std::fs::read_to_string(&path).ok()?;
    Some((path, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_five_named_failures_each_get_their_fix() {
        let cases = [
            ("java.net.BindException: Address already in use", Fix::Port),
            ("java.lang.OutOfMemoryError: Java heap space", Fix::Memory),
            (
                "Missing or unsupported mandatory dependencies: jei",
                Fix::Mods,
            ),
            ("You need to agree to the EULA", Fix::Eula),
            (
                "UnsupportedClassVersionError: has been compiled by a more recent version \
                 of the Java Runtime (class file version 65.0)",
                Fix::Java { major: 21 },
            ),
        ];
        for (text, want) in cases {
            let (headline, fix) = classify(text);
            assert!(headline.is_some(), "{text} produced no headline");
            assert_eq!(fix, Some(want), "{text}");
        }
    }

    #[test]
    fn an_unrecognised_failure_shows_the_log_instead_of_guessing() {
        let dir = tempfile::tempdir().unwrap();
        let log: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();

        let info = diagnose(dir.path(), &log, Some(1), SystemTime::now());
        assert!(info.fix.is_none());
        assert!(info.headline.contains("結束碼 1"));
        assert!(info.detail.ends_with("line 39"), "shows the tail");
        assert!(!info.detail.contains("line 0"), "not the whole buffer");
    }

    #[test]
    fn a_crash_report_from_an_earlier_run_is_not_blamed_for_this_one() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join("crash-reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(
            reports.join("crash-2026-03-01.txt"),
            "Description: Exception in server tick loop\njava.lang.OutOfMemoryError",
        )
        .unwrap();

        // The run started after that file was written.
        let started = SystemTime::now() + Duration::from_secs(60);
        let info = diagnose(dir.path(), &["nothing useful".to_owned()], Some(1), started);
        assert_eq!(info.fix, None, "an old report must not become a diagnosis");
        assert_eq!(info.path, None);
    }

    #[test]
    fn a_report_written_by_this_run_supplies_both_the_fix_and_the_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join("crash-reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(
            reports.join("crash-now.txt"),
            "---- Minecraft Crash Report ----\n\
             Description: Exception in server tick loop\n\
             java.lang.OutOfMemoryError: Java heap space\n",
        )
        .unwrap();

        let started = SystemTime::now() - Duration::from_secs(60);
        let info = diagnose(dir.path(), &[], Some(1), started);
        assert_eq!(info.fix, Some(Fix::Memory));
        assert!(info.detail.starts_with("Exception in server tick loop"));
        assert!(info.path.is_some());
    }
}
