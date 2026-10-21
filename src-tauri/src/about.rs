//! App-info + update-check + safe URL opener.
//!
//! Tiny module: the version and GitHub link on the profile screen, and a
//! check for whether a newer release exists on GitHub. The actual download
//! is intentionally NOT automated — we surface the release URL and let
//! the user pick whether to grab it (auto-update infrastructure is a
//! separate, bigger problem involving signed updates).
//!
//! The fork is used two ways (build.rs): as a release, which every merge to
//! main is (v0.3.8-fork.1, v0.3.8-fork.2, …: the original project's version
//! and a count), or built from source. Besides releases, the check says how
//! many commits the fork's main has that a build from source doesn't — to
//! pull and rebuild — and how many the original project has that the fork
//! doesn't yet (for a sync pull request, FORK.md).

use serde::Serialize;

/// `<owner>/<repo>` on GitHub (canonical casing). Used to build the API URL
/// for the releases query and the user-facing repo URL.
pub const GITHUB_REPO: &str = "paulm981/Submarine";

/// The project GITHUB_REPO was forked from.
pub const UPSTREAM_REPO: &str = "SinaXhpm/Submarine";

/// The commit this build was made from (build.rs); empty when it wasn't
/// built from a git checkout, and in a release build.
const BUILD_COMMIT: &str = env!("SUBMARINE_GIT_SHA");
/// "source" or "release" (build.rs).
const BUILD_KIND: &str = env!("SUBMARINE_BUILD_KIND");
/// This build's version (build.rs): a release's whole tag, such as
/// "0.3.8-fork.1", even where the installer's own version can't carry the
/// "-fork.1" (Windows), and otherwise Cargo's.
const VERSION: &str = env!("SUBMARINE_VERSION");

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: String,
    pub github_repo_url: String,
    pub github_releases_url: String,
    /// The first 7 characters of the commit it was built from, when known —
    /// so two builds from source of the same version can be told apart.
    pub commit: Option<String>,
}

/// "0.3.8-fork.3 (source, 23d8806)": which build this is, for the log and
/// "Copy diagnostics".
pub fn build_label() -> String {
    let mut label = VERSION.to_string();
    let commit = BUILD_COMMIT.get(..7).unwrap_or("");
    label.push_str(&format!(" ({}{}{})", BUILD_KIND, if commit.is_empty() { "" } else { ", " }, commit));
    label
}

/// Static info bundled with the binary. Version comes from CARGO_PKG_VERSION
/// which Cargo + tauri-action set from the git tag during release builds.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: VERSION.to_string(),
        github_repo_url: format!("https://github.com/{}", GITHUB_REPO),
        github_releases_url: format!("https://github.com/{}/releases", GITHUB_REPO),
        commit: BUILD_COMMIT.get(..7).map(str::to_string),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    /// Current bundled version (semver, no "v" prefix).
    pub current: String,
    /// Latest release tag on GitHub, with "v" stripped to match `current`.
    /// `None` if the API call failed or no release exists.
    pub latest: Option<String>,
    /// True when the newest published, non-prerelease release on GitHub is
    /// newer than the running build (see `semver_greater`). Frontend uses
    /// this to colour the banner (green = up-to-date, amber = update
    /// available).
    pub has_update: bool,
    /// Direct link to the latest release page. Frontend shows an "Open
    /// release notes" button when has_update is true.
    pub release_url: Option<String>,
    /// Commits on the fork's main that this build from source doesn't have —
    /// None when that couldn't be told (a release, a build not made from a
    /// pushed commit, GitHub unreachable) — and the GitHub page comparing
    /// them, whenever the build knows its commit.
    pub main_ahead: Option<u64>,
    pub main_url: Option<String>,
    /// Commits on the original project's main not yet in the fork's main
    /// (None when that couldn't be told), and GitHub's view of them.
    pub upstream_ahead: Option<u64>,
    pub upstream_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GhCompare {
    #[serde(default)]
    ahead_by: u64,
}

/// `ahead_by` of a GitHub compare: the commits in `head` that `base` lacks.
async fn ahead_by(client: &reqwest::Client, base: &str, head: &str) -> Option<u64> {
    // One commit per page: only the count is read, and a compare otherwise
    // lists up to 250 commits.
    let url = format!("https://api.github.com/repos/{}/compare/{}...{}?per_page=1", GITHUB_REPO, base, head);
    let resp = client.get(&url).header("Accept", "application/vnd.github+json").send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // One commit's worth of JSON; read no more than 1 MB of it (cloud.rs).
    let bytes = crate::cloud::read_capped(resp, 1024 * 1024).await.ok()?;
    serde_json::from_slice::<GhCompare>(&bytes).ok().map(|c| c.ahead_by)
}

/// Subset of GitHub's `/releases/latest` JSON we actually care about.
/// Marked non_exhaustive-friendly via #[serde(default)] so a future
/// addition / rename on GitHub's side doesn't break us.
#[derive(Debug, serde::Deserialize)]
struct GhRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Ask GitHub which release is newest and whether it beats the running build.
///
/// We pull the RELEASES LIST rather than `/releases/latest`: GitHub's "latest"
/// flag is date-based (and manually overridable), so a hotfix published on an
/// old line could point it at a lower version than what's actually shipped.
/// Instead we scan every published, non-prerelease release and pick the highest
/// SEMVER ourselves, then compare that to `current`. Anonymous calls are rate-
/// limited to ~60/hour per IP — fine for an occasional check. The timeout is
/// short so a hung connection can't leave the check spinning.
///
/// `watch_upstream`: also count the original project's new commits — for
/// whoever maintains the fork (Settings › Updates), the only one who can
/// bring them in; everyone else is spared the notice and the call.
#[tauri::command]
pub async fn check_for_updates(watch_upstream: Option<bool>) -> Result<UpdateInfo, String> {
    let current = VERSION.to_string();
    let url = format!("https://api.github.com/repos/{}/releases?per_page=100", GITHUB_REPO);

    // Build a fresh client per call rather than holding state — this is
    // a one-shot probe, not a hot path. User-Agent is required by the
    // GitHub API; an empty/missing UA gets a 403.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent(concat!("submarine-app/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("[UPDATE] CLIENT: {}", e))?;

    // The releases and the commit counts at the same time, so the manual
    // check waits for the slowest request rather than all of them in turn.
    let watch_upstream = watch_upstream.unwrap_or(false);
    let (info, (main_ahead, upstream_ahead)) =
        tokio::join!(latest_release(&client, &url, current), commit_counts(&client, watch_upstream));
    let mut info = info?;
    if !BUILD_COMMIT.is_empty() {
        info.main_ahead = main_ahead;
        info.main_url = Some(format!("https://github.com/{}/compare/{}...main", GITHUB_REPO, BUILD_COMMIT));
    }
    if watch_upstream {
        info.upstream_ahead = upstream_ahead;
        // What the original project has that the fork's main doesn't, to
        // bring in with a sync pull request (FORK.md). Not the fork's own
        // page: once the fork has its own commits, GitHub's Sync fork there
        // offers to discard them.
        let (upstream_owner, upstream_name) = UPSTREAM_REPO.split_once('/').unwrap_or((UPSTREAM_REPO, ""));
        info.upstream_url = Some(format!(
            "https://github.com/{}/compare/main...{}:{}:main",
            GITHUB_REPO, upstream_owner, upstream_name
        ));
    }
    Ok(info)
}

/// The highest published release on `url` (GITHUB_REPO's releases), and
/// whether it beats `current`.
async fn latest_release(client: &reqwest::Client, url: &str, current: String) -> Result<UpdateInfo, String> {
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("[UPDATE] NETWORK: {}", e))?;

    if !resp.status().is_success() {
        // 404 means the repo path is wrong; anything else is a transient
        // GitHub / rate-limit error. Either way, don't nag — report "no
        // newer release known" so the UI stays quiet rather than scary.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(UpdateInfo::none(current));
        }
        return Err(format!("[UPDATE] HTTP {}", resp.status()));
    }

    // An empty repo returns `[]` (200), not 404 — handled naturally below.
    // The list of releases is a few hundred KB at most; read no more than 8 MB.
    let bytes = crate::cloud::read_capped(resp, 8 * 1024 * 1024).await.map_err(|e| format!("[UPDATE] {}", e))?;
    let releases: Vec<GhRelease> = serde_json::from_slice(&bytes).map_err(|e| format!("[UPDATE] BAD_JSON: {}", e))?;

    Ok(match newest_release(releases) {
        None => UpdateInfo::none(current),
        Some((latest_clean, html_url)) => UpdateInfo {
            has_update: semver_greater(&latest_clean, &current),
            latest: Some(latest_clean),
            release_url: if html_url.is_empty() { None } else { Some(html_url) },
            ..UpdateInfo::none(current)
        },
    })
}

/// The highest published, non-prerelease version, without its "v", and its
/// page. Drafts are only visible with auth (we're anonymous) but we skip them
/// defensively all the same. The old "Latest build" release (tag
/// latest-main) is a pre-release, so it never counts.
fn newest_release(releases: Vec<GhRelease>) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for r in releases {
        if r.draft || r.prerelease || r.tag_name.is_empty() {
            continue;
        }
        let clean = r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name).to_string();
        if best.as_ref().is_none_or(|(cur_best, _)| semver_greater(&clean, cur_best)) {
            best = Some((clean, r.html_url));
        }
    }
    best
}

impl UpdateInfo {
    fn none(current: String) -> Self {
        UpdateInfo {
            current,
            latest: None,
            has_update: false,
            release_url: None,
            main_ahead: None,
            main_url: None,
            upstream_ahead: None,
            upstream_url: None,
        }
    }
}

/// The commits the fork's main has that this build doesn't (for a build made
/// from source), and — when asked — the commits the original project's main
/// has that the fork's doesn't. Best effort, like the rest: None when GitHub
/// couldn't tell, or it wasn't asked.
async fn commit_counts(client: &reqwest::Client, watch_upstream: bool) -> (Option<u64>, Option<u64>) {
    let upstream_owner = UPSTREAM_REPO.split('/').next().unwrap_or(UPSTREAM_REPO);
    let upstream_head = format!("{}:main", upstream_owner);
    let main = async {
        if BUILD_COMMIT.is_empty() {
            None
        } else {
            ahead_by(client, BUILD_COMMIT, "main").await
        }
    };
    let upstream = async {
        if watch_upstream {
            ahead_by(client, "main", &upstream_head).await
        } else {
            None
        }
    };
    tokio::join!(main, upstream)
}

/// True when `a > b` under dotted-number compare: "0.2.0" > "0.1.5",
/// "1.0.0" > "0.9.9". The fork's releases of a version come after it, in
/// order: "0.3.8" < "0.3.8-fork.1" < "0.3.8-fork.2" < "0.3.9". Any other
/// suffix, like "-beta", sorts EQUAL to its base — preferring stable over
/// pre.
fn semver_greater(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> (Vec<u64>, u64) {
        let (base, suffix) = s.split_once(['-', '+']).unwrap_or((s, ""));
        let parts = base.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
        let fork = suffix.strip_prefix("fork.").and_then(|n| n.parse().ok()).unwrap_or(0);
        (parts, fork)
    };
    let ((aa, a_fork), (bb, b_fork)) = (parse(a), parse(b));
    for i in 0..aa.len().max(bb.len()) {
        let av = aa.get(i).copied().unwrap_or(0);
        let bv = bb.get(i).copied().unwrap_or(0);
        if av != bv {
            return av > bv;
        }
    }
    a_fork > b_fork
}

/// Open a URL in the user's default browser. URL must be http(s) — we
/// refuse file://, javascript:, and anything else to avoid being a
/// privileged drop-tool for the webview. tauri-plugin-opener dispatches
/// to ACTION_VIEW on Android and to xdg-open / open / start on desktop,
/// so the same call works on every platform we ship (the `open` crate
/// has no Android backend, which is why we used to return "not wired on
/// Android yet" — now fixed).
#[tauri::command]
pub fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let lowered = url.to_ascii_lowercase();
    if !(lowered.starts_with("https://") || lowered.starts_with("http://")) {
        return Err("[OPEN] only http(s) urls are allowed".into());
    }
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| format!("[OPEN] {}", e))
}

#[cfg(test)]
mod tests {
    use super::{newest_release, semver_greater, GhRelease};

    fn release(tag: &str, prerelease: bool) -> GhRelease {
        GhRelease { tag_name: tag.into(), html_url: format!("page/{tag}"), draft: false, prerelease }
    }

    #[test]
    fn the_newest_fork_release_wins_and_latest_build_never_counts() {
        let releases = vec![
            release("latest-main", true),
            release("v0.3.8-fork.2", false),
            release("v0.3.8-fork.10", false),
            release("v0.3.8-fork.9", false),
        ];
        assert_eq!(newest_release(releases), Some(("0.3.8-fork.10".into(), "page/v0.3.8-fork.10".into())));
        assert_eq!(newest_release(vec![release("latest-main", true)]), None);
        // A build up to 244 says "0.3.8": the first fork release is newer.
        assert!(semver_greater("0.3.8-fork.1", "0.3.8"));
    }

    #[test]
    fn fork_releases_come_after_their_version_in_order() {
        assert!(semver_greater("0.3.8-fork.1", "0.3.8"));
        assert!(semver_greater("0.3.8-fork.2", "0.3.8-fork.1"));
        assert!(semver_greater("0.3.8-fork.10", "0.3.8-fork.9"));
        assert!(semver_greater("0.3.9", "0.3.8-fork.5"));
        assert!(!semver_greater("0.3.8-fork.1", "0.3.8-fork.1"));
        assert!(!semver_greater("0.3.8", "0.3.8-fork.1"));
        // Other suffixes still sort equal to their base.
        assert!(!semver_greater("0.3.8-beta", "0.3.8"));
        assert!(!semver_greater("0.3.8", "0.3.8-beta"));
        assert!(semver_greater("0.10.0", "0.9.9"));
    }
}
