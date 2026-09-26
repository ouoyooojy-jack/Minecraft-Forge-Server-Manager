//! Forge version discovery.
//!
//! One endpoint, one document: Forge publishes every build it has ever made in
//! a Maven metadata file. This module fetches it, turns it into the two-level
//! list the download page's dropdowns need, and builds installer URLs.
//!
//! Deliberately no XML crate. The document is a fixed, known shape and the only
//! thing wanted from it is the text of `<version>` elements — a dependency to
//! read one tag is the kind of weight that never comes back off.

use std::cmp::Reverse;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::types::{CoreError, CoreResult, ForgeVersionGroup, HTTP_TIMEOUT_SEC};

const METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";

/// ponytail: hardcoded upstream. Lift to a setting when someone actually needs
/// a mirror; a config knob for a value nobody changes is dead weight.
const INSTALLER_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/{v}/forge-{v}-installer.jar";

/// A version string longer than this is not a version string.
const MAX_VERSION_LEN: usize = 64;

/// Where a loader unpacks the arguments and jars it generates, relative to the
/// server folder. Forge and NeoForge use the same layout under different
/// coordinates, so everything downstream only has to know the list.
///
/// Ordered: a folder holds one loader, and checking Forge first keeps the
/// answer stable for the installs that already exist.
pub const LOADER_LIBS: [&str; 2] = [
    "libraries/net/minecraftforge/forge",
    "libraries/net/neoforged/neoforge",
];

pub struct Forge {
    client: reqwest::Client,
    /// Fetched once per run. Forge does not publish hourly, and re-fetching a
    /// megabyte of XML every time the download page opens is pure waste — the
    /// UI can force a refresh when it wants one.
    cache: Mutex<Option<Vec<ForgeVersionGroup>>>,
}

impl Forge {
    pub fn new() -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(HTTP_TIMEOUT_SEC))
            .user_agent(concat!("McServerManager/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| CoreError::Network {
                message: e.to_string(),
            })?;
        Ok(Self {
            client,
            cache: Mutex::new(None),
        })
    }

    /// Every published Forge build, grouped by Minecraft line, newest first.
    pub async fn versions(&self, refresh: bool) -> CoreResult<Vec<ForgeVersionGroup>> {
        if !refresh {
            if let Some(cached) = self.cache.lock().expect("forge mutex poisoned").clone() {
                return Ok(cached);
            }
        }
        let groups = self.fetch(METADATA_URL).await?;
        *self.cache.lock().expect("forge mutex poisoned") = Some(groups.clone());
        Ok(groups)
    }

    /// `pub(crate)` so tests can point it at a loopback server.
    pub(crate) async fn fetch(&self, url: &str) -> CoreResult<Vec<ForgeVersionGroup>> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| CoreError::Network {
                message: e.to_string(),
            })?;
        if !response.status().is_success() {
            return Err(CoreError::Network {
                message: format!("HTTP {}", response.status()),
            });
        }
        let xml = response.text().await.map_err(|e| CoreError::Network {
            message: e.to_string(),
        })?;

        let versions = parse_versions(&xml);
        if versions.is_empty() {
            // A 200 with an unusable body is not a success. Failing loudly here
            // beats handing the UI an empty dropdown that looks like "Forge has
            // no releases".
            return Err(CoreError::Network {
                message: "Forge metadata contained no versions".into(),
            });
        }
        Ok(group_by_mc_major(versions))
    }
}

// ─────────────────────────────────────────────────────────────
// parsing and grouping
// ─────────────────────────────────────────────────────────────

/// Pull the text of every `<version>` element, in document order.
///
/// Anything that does not look like a version is dropped rather than trusted:
/// this text ends up in a URL and, downstream, in a filename.
fn parse_versions(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        let Some(end) = after.find("</version>") else {
            break;
        };
        let value = after[..end].trim();
        if is_valid_version(value) {
            out.push(value.to_owned());
        }
        rest = &after[end..];
    }
    out
}

/// Conservative allowlist. Forge versions are ASCII alphanumerics with dots,
/// dashes, and the occasional underscore; anything else is either corruption or
/// an attempt to escape a path.
fn is_valid_version(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_VERSION_LEN
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Split `1.20.1-47.2.0` into its Minecraft line, `1.20`.
///
/// Old builds carry oddities (`1.7.10_pre4-10.12.2.1121`), so anything that
/// does not split cleanly is grouped under whatever came before the first dash
/// rather than discarded — a user looking for an ancient version should still
/// find it.
fn mc_major_of(version: &str) -> String {
    let mc = version.split('-').next().unwrap_or(version);
    let mut parts = mc.split('.');
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => format!("{a}.{b}"),
        _ => mc.to_owned(),
    }
}

/// Sort key for a Minecraft line. `"1.9"` must come before `"1.10"`, which
/// string ordering gets backwards.
fn major_sort_key(major: &str) -> (u32, u32) {
    let mut parts = major.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// Leading numbers of each dot/underscore/dash separated segment.
///
/// Tolerates the decorations Forge has used over the years — `1.7.10_pre4`,
/// `…-1149-prerelease` — by taking the digits it can and ignoring the rest.
fn numbers(s: &str) -> Vec<u32> {
    s.split(['.', '_', '-'])
        .filter_map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect()
}

/// Numeric ordering key: the Minecraft version, then the Forge build.
fn version_key(version: &str) -> (Vec<u32>, Vec<u32>) {
    let (mc, build) = version.split_once('-').unwrap_or((version, ""));
    (numbers(mc), numbers(build))
}

/// Group into newest-first lines of newest-first builds.
///
/// Sorted explicitly rather than by reversing the document. The real metadata
/// is *mostly* newest-first already — it opens on 1.21 builds counting down and
/// reaches 1.7.10 near the end — but newly published versions are appended, so
/// the tail holds the newest releases of all. Neither the document order nor
/// its reverse is correct; only comparing versions is.
fn group_by_mc_major(versions: Vec<String>) -> Vec<ForgeVersionGroup> {
    let mut by_major: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for version in versions {
        by_major
            .entry(mc_major_of(&version))
            .or_default()
            .push(version);
    }

    let mut groups: Vec<ForgeVersionGroup> = by_major
        .into_iter()
        .map(|(mc_major, mut versions)| {
            versions.sort_by_key(|v| Reverse(version_key(v)));
            versions.dedup();
            ForgeVersionGroup { mc_major, versions }
        })
        .collect();

    groups.sort_by_key(|g| Reverse(major_sort_key(&g.mc_major)));
    groups
}

// ─────────────────────────────────────────────────────────────
// URLs
// ─────────────────────────────────────────────────────────────

/// Where to fetch the installer for `version`.
pub fn installer_url(version: &str) -> CoreResult<String> {
    reject_bad_version(version)?;
    Ok(INSTALLER_URL.replace("{v}", version))
}

/// What to call the installer on disk. Matches Forge's own naming so a jar
/// dropped in by hand is indistinguishable from one this app fetched.
pub fn installer_filename(version: &str) -> CoreResult<String> {
    reject_bad_version(version)?;
    Ok(format!("forge-{version}-installer.jar"))
}

/// Split `1.20.1-47.2.0` into its Minecraft and Forge halves.
///
/// Recorded on the server after a successful install so the card can say which
/// versions it is actually running, rather than "尚未安裝 Forge" forever.
pub fn split_version(version: &str) -> (String, String) {
    match version.split_once('-') {
        Some((mc, build)) => (mc.to_owned(), build.to_owned()),
        // NeoForge carries no Minecraft version in its own, so the line has to
        // be derived or the card says nothing about which Minecraft this is.
        None => match neoforge_mc_line(version) {
            Some(mc) => (mc, version.to_owned()),
            None => (version.to_owned(), String::new()),
        },
    }
}

/// The Minecraft line a NeoForge version targets.
///
/// NeoForge numbers its builds after the Minecraft version rather than
/// alongside it: `21.1.249` is a build for 1.21.1. A `0` minor is the `.0`
/// release, which Minecraft writes without it -- `21.0.167` is 1.21, not
/// 1.21.0.
///
/// Minecraft's move to year-based versions changed the shape again:
/// `26.2.0.75` carries four parts and targets 26.2, which needs no `1.` in
/// front. Anything that is not all-numeric is not a NeoForge version.
fn neoforge_mc_line(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    let nums: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    if nums.len() != parts.len() {
        return None;
    }
    match nums.as_slice() {
        [major, minor, _, _] => Some(format!("{major}.{minor}")),
        [major, 0, _] => Some(format!("1.{major}")),
        [major, minor, _] => Some(format!("1.{major}.{minor}")),
        _ => None,
    }
}

/// Recover the version from an installer's filename.
///
/// `forge-1.20.1-47.2.0-installer.jar` → `1.20.1-47.2.0`. The inverse of
/// `installer_filename`, used when a server is built from a jar already on
/// disk — the version is not otherwise known at that point.
///
/// `None` for a jar that does not follow Forge's naming, which is allowed: the
/// server still installs, it just shows no version until it is started.
pub fn version_from_filename(name: &str) -> Option<String> {
    let stem = name.strip_suffix("-installer.jar")?;
    // `neoforge-` cannot be reached by stripping `forge-`: the prefix has to
    // match from the start, and NeoForge's begins with `neo`.
    let version = stem
        .strip_prefix("forge-")
        .or_else(|| stem.strip_prefix("neoforge-"))?;
    is_valid_version(version).then(|| version.to_owned())
}

/// The Forge version an existing installation was built from, read off disk.
///
/// An imported folder carries no record of what it is, and the version decides
/// which Java it needs — without it, importing a server means losing the one
/// thing that makes the missing-runtime prompt possible.
///
/// Forge unpacks itself into `libraries/net/minecraftforge/forge/<mc>-<build>/`,
/// which has held that shape since 1.13. Older installs have no such directory;
/// their `run.bat` does not exist either, so they launch from `server.jar` and
/// run on Java 8 regardless.
pub fn version_from_install(dir: &Path) -> Option<String> {
    // One folder, one loader: take the first root that has anything rather
    // than comparing a Forge version against a NeoForge one, which are not on
    // the same scale and would sort nonsensically against each other.
    LOADER_LIBS
        .iter()
        .find_map(|lib| newest_build(&dir.join(lib)))
}

/// The newest build directory under one loader's library root.
fn newest_build(root: &Path) -> Option<String> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|name| is_valid_version(name))
        // A folder that has been upgraded in place keeps both versions; the
        // newest is the one its run script points at.
        .max_by_key(|name| version_key(name))
}

fn reject_bad_version(version: &str) -> CoreResult<()> {
    if is_valid_version(version) {
        Ok(())
    } else {
        // The version reaches a URL and a filename, so this is the last place
        // to stop a crafted one.
        Err(CoreError::VersionNotFound {
            version: version.chars().take(MAX_VERSION_LEN).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::serve;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.minecraftforge</groupId>
  <artifactId>forge</artifactId>
  <versioning>
    <release>1.20.1-47.2.0</release>
    <versions>
      <version>1.9.4-12.17.0.1976</version>
      <version>1.10.2-12.18.3.2511</version>
      <version>1.20.1-47.1.0</version>
      <version>1.20.1-47.2.0</version>
      <version>1.20.4-49.0.30</version>
    </versions>
  </versioning>
</metadata>"#;

    #[test]
    fn versions_are_read_in_document_order() {
        let got = parse_versions(SAMPLE);
        assert_eq!(
            got,
            [
                "1.9.4-12.17.0.1976",
                "1.10.2-12.18.3.2511",
                "1.20.1-47.1.0",
                "1.20.1-47.2.0",
                "1.20.4-49.0.30",
            ]
        );
    }

    #[test]
    fn junk_versions_are_dropped_not_trusted() {
        let xml = r#"
            <version>1.20.1-47.2.0</version>
            <version>../../../etc/passwd</version>
            <version>..\..\windows\system32</version>
            <version></version>
            <version>   </version>
            <version>a/b</version>
        "#;
        assert_eq!(parse_versions(xml), ["1.20.1-47.2.0"]);
    }

    #[test]
    fn an_overlong_version_is_rejected() {
        let long = "1.".repeat(MAX_VERSION_LEN);
        assert!(!is_valid_version(&long));
    }

    #[test]
    fn an_unterminated_tag_does_not_loop_forever() {
        assert!(parse_versions("<version>1.20.1-47.2.0").is_empty());
    }

    #[test]
    fn lines_are_ordered_numerically_not_lexically() {
        let groups = group_by_mc_major(parse_versions(SAMPLE));
        let majors: Vec<&str> = groups.iter().map(|g| g.mc_major.as_str()).collect();
        // "1.9" sorts after "1.10" as a string; it must not here.
        assert_eq!(majors, ["1.20", "1.10", "1.9"]);
    }

    #[test]
    fn newest_build_comes_first_within_a_line() {
        let groups = group_by_mc_major(parse_versions(SAMPLE));
        let line = groups.iter().find(|g| g.mc_major == "1.20").unwrap();
        assert_eq!(
            line.versions,
            ["1.20.4-49.0.30", "1.20.1-47.2.0", "1.20.1-47.1.0"]
        );
    }

    /// The shape the live document actually has: a mostly-descending historical
    /// block, with newly published versions appended at the end. Neither taking
    /// it as-is nor reversing it produces newest-first.
    #[test]
    fn ordering_survives_the_real_documents_layout() {
        let versions: Vec<String> = [
            // descending block, as the file opens
            "1.21-51.0.33",
            "1.21-51.0.32",
            "1.20.1-47.2.0",
            "1.20.1-47.1.0",
            "1.7.10-10.13.4.1614",
            // appended later — the newest releases sit at the very end
            "1.21.11-61.1.14",
            "1.20.1-47.4.0",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let groups = group_by_mc_major(versions);
        let majors: Vec<&str> = groups.iter().map(|g| g.mc_major.as_str()).collect();
        assert_eq!(majors, ["1.21", "1.20", "1.7"]);

        let latest = &groups[0].versions;
        assert_eq!(
            latest[0], "1.21.11-61.1.14",
            "an appended release must outrank the block it was added after"
        );
        assert_eq!(
            groups[1].versions[0], "1.20.1-47.4.0",
            "and the same within a line"
        );
    }

    #[test]
    fn build_numbers_compare_numerically_not_as_text() {
        let groups = group_by_mc_major(
            [
                "1.12.2-14.23.3.2676",
                "1.12.2-14.23.3.999",
                "1.12.2-14.23.4.1",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );
        assert_eq!(
            groups[0].versions,
            [
                "1.12.2-14.23.4.1",
                "1.12.2-14.23.3.2676",
                "1.12.2-14.23.3.999"
            ],
            "\"999\" must not sort above \"2676\""
        );
    }

    #[test]
    fn decorated_legacy_versions_still_order() {
        let groups = group_by_mc_major(
            [
                "1.7.10_pre4-10.12.2.1149-prerelease",
                "1.7.10_pre4-10.12.2.1143-prerelease",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );
        assert_eq!(groups[0].versions[0], "1.7.10_pre4-10.12.2.1149-prerelease");
    }

    #[test]
    fn the_new_minecraft_numbering_outranks_the_old() {
        // Minecraft moved to calendar-style versions; "26.2" is newer than
        // "1.21", and both parse as numbers so the comparison holds.
        let groups = group_by_mc_major(
            ["1.21.11-61.1.14", "26.2-65.1.0"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        assert_eq!(groups[0].mc_major, "26.2");
    }

    #[test]
    fn duplicates_collapse() {
        let groups = group_by_mc_major(
            ["1.20.1-47.2.0", "1.20.1-47.2.0"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        assert_eq!(groups[0].versions.len(), 1);
    }

    #[test]
    fn odd_legacy_versions_still_land_in_a_group() {
        let groups = group_by_mc_major(vec!["1.7.10_pre4-10.12.2.1121".into()]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].mc_major, "1.7");
    }

    #[test]
    fn urls_and_filenames_follow_forge_naming() {
        assert_eq!(
            installer_url("1.20.1-47.2.0").unwrap(),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar"
        );
        assert_eq!(
            installer_filename("1.20.1-47.2.0").unwrap(),
            "forge-1.20.1-47.2.0-installer.jar"
        );
    }

    #[test]
    fn an_installed_folder_reveals_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries/net/minecraftforge/forge");
        std::fs::create_dir_all(libs.join("1.20.1-47.2.0")).unwrap();
        assert_eq!(
            version_from_install(dir.path()),
            Some("1.20.1-47.2.0".into())
        );

        // Upgraded in place: the newest wins, not whatever the OS lists first.
        std::fs::create_dir_all(libs.join("1.20.1-47.3.11")).unwrap();
        assert_eq!(
            version_from_install(dir.path()),
            Some("1.20.1-47.3.11".into())
        );
    }

    #[test]
    fn a_neoforge_install_reveals_its_version_and_minecraft_line() {
        let dir = tempfile::tempdir().unwrap();
        let libs = dir.path().join("libraries/net/neoforged/neoforge");
        std::fs::create_dir_all(libs.join("21.1.249")).unwrap();
        assert_eq!(version_from_install(dir.path()), Some("21.1.249".into()));
        assert_eq!(
            split_version("21.1.249"),
            ("1.21.1".into(), "21.1.249".into())
        );
    }

    #[test]
    fn a_neoforge_version_names_the_minecraft_it_targets() {
        for (version, mc) in [
            ("21.1.249", "1.21.1"),
            ("20.4.237", "1.20.4"),
            // A `0` minor is the `.0` release, which Minecraft writes without it.
            ("21.0.167", "1.21"),
            // Year-based Minecraft: four parts, and no `1.` in front.
            ("26.2.0.75", "26.2"),
        ] {
            assert_eq!(neoforge_mc_line(version).as_deref(), Some(mc), "{version}");
        }
        // Forge versions carry a dash and are not NeoForge's shape.
        assert_eq!(neoforge_mc_line("1.20.1-47.2.0"), None);
    }

    #[test]
    fn a_neoforge_installer_filename_yields_its_version() {
        assert_eq!(
            version_from_filename("neoforge-21.1.249-installer.jar").as_deref(),
            Some("21.1.249")
        );
    }

    #[test]
    fn a_folder_with_no_forge_libraries_has_no_version() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(version_from_install(dir.path()), None);
    }

    #[test]
    fn filenames_round_trip_back_to_versions() {
        let version = "1.20.1-47.2.0";
        let name = installer_filename(version).unwrap();
        assert_eq!(version_from_filename(&name).as_deref(), Some(version));
    }

    #[test]
    fn a_jar_that_is_not_a_forge_installer_yields_no_version() {
        for name in [
            "server.jar",
            "forge-1.20.1.jar",
            "1.20.1-47.2.0-installer.jar",
            "forge-../../evil-installer.jar",
        ] {
            assert_eq!(version_from_filename(name), None, "{name}");
        }
    }

    #[test]
    fn a_crafted_version_cannot_reach_a_url_or_a_filename() {
        for bad in ["../../evil", "a b", "1.20.1-47.2.0/../x", ""] {
            assert!(installer_url(bad).is_err(), "{bad:?} must not build a URL");
            assert!(
                installer_filename(bad).is_err(),
                "{bad:?} must not build a filename"
            );
        }
    }

    #[tokio::test]
    async fn fetch_parses_a_served_document() {
        let addr = serve("200 OK", SAMPLE.as_bytes().to_vec(), 0).await;
        let forge = Forge::new().unwrap();
        let groups = forge
            .fetch(&format!("http://{addr}/maven-metadata.xml"))
            .await
            .unwrap();

        assert_eq!(groups[0].mc_major, "1.20");
        assert_eq!(groups[0].versions[0], "1.20.4-49.0.30");
    }

    #[tokio::test]
    async fn a_200_with_no_versions_is_an_error_not_an_empty_list() {
        let addr = serve("200 OK", b"<metadata/>".to_vec(), 0).await;
        let forge = Forge::new().unwrap();
        let err = forge
            .fetch(&format!("http://{addr}/maven-metadata.xml"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, CoreError::Network { .. }),
            "an empty dropdown must not be presented as 'Forge has no releases'"
        );
    }

    #[tokio::test]
    async fn a_server_error_surfaces_as_a_network_error() {
        let addr = serve("503 Service Unavailable", Vec::new(), 0).await;
        let forge = Forge::new().unwrap();
        assert!(matches!(
            forge.fetch(&format!("http://{addr}/x")).await.unwrap_err(),
            CoreError::Network { .. }
        ));
    }

    /// Opt-in check against the live endpoint: `cargo test -- --ignored`.
    ///
    /// Not part of the suite — a test that needs maven.minecraftforge.net goes
    /// red when the network does. But the ordering rules here are guesses about
    /// a document this project does not control, and one of them was already
    /// wrong, so there needs to be a one-command way to re-check them.
    #[tokio::test]
    #[ignore = "hits the network"]
    async fn live_metadata_is_sorted_newest_first() {
        let forge = Forge::new().unwrap();
        let groups = forge.fetch(METADATA_URL).await.unwrap();

        assert!(groups.len() > 20, "expected many Minecraft lines");

        let majors: Vec<(u32, u32)> = groups.iter().map(|g| major_sort_key(&g.mc_major)).collect();
        assert!(
            majors.windows(2).all(|w| w[0] >= w[1]),
            "lines must descend: {:?}",
            groups
                .iter()
                .map(|g| &g.mc_major)
                .take(8)
                .collect::<Vec<_>>()
        );

        for group in &groups {
            let keys: Vec<_> = group.versions.iter().map(|v| version_key(v)).collect();
            assert!(
                keys.windows(2).all(|w| w[0] >= w[1]),
                "builds must descend within {}: {:?}",
                group.mc_major,
                &group.versions[..group.versions.len().min(5)]
            );
        }

        println!(
            "newest line {} → {}",
            groups[0].mc_major, groups[0].versions[0]
        );
    }

    #[tokio::test]
    async fn the_second_call_is_served_from_cache() {
        // The loopback server accepts exactly one connection, so a second
        // fetch would fail — succeeding proves nothing went out.
        let addr = serve("200 OK", SAMPLE.as_bytes().to_vec(), 0).await;
        let forge = Forge::new().unwrap();
        let first = forge
            .fetch(&format!("http://{addr}/maven-metadata.xml"))
            .await
            .unwrap();
        *forge.cache.lock().unwrap() = Some(first.clone());

        assert_eq!(forge.versions(false).await.unwrap(), first);
    }
}
