//! In-app updates through Velopack.
//!
//! Releases are published to GitHub by the release workflow together with one
//! `releases.{channel}.json` feed per platform (`win`, `osx`, `linux`).
//! Velopack reads those feeds, verifies the SHA-256 of every downloaded
//! package against them, swaps the installed version, and restarts the app.
//!
//! Only installations made by the Velopack installer can update themselves.
//! Development builds and unpacked archives report
//! [`app_updates_supported`] as `false`; the client then hides the update
//! actions instead of failing.
//!
//! The macOS build runs in the App Sandbox, which Velopack cannot update.
//! There [`check_for_release_notice`] only reports a newer release and its
//! download link; the user replaces the app from the DMG.

use std::sync::{Mutex, mpsc};
use std::thread;

use velopack::sources::{GithubSource, NoneSource};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackApp};

use semver::Version;

use crate::frb_generated::StreamSink;

/// Repository whose GitHub releases carry the Velopack feeds.
const RELEASES_REPOSITORY: &str = "https://github.com/Tim0theus/maicenta";
/// The newest releases, as GitHub lists them; older ones are never offered.
const RELEASES_API: &str = "https://api.github.com/repos/Tim0theus/maicenta/releases?per_page=10";

/// Update found by the last [`check_for_app_update`], kept so that
/// [`download_app_update`] and [`apply_app_update_and_restart`] act on exactly
/// the release the user confirmed.
static PENDING_UPDATE: Mutex<Option<PendingUpdate>> = Mutex::new(None);

struct PendingUpdate {
    manager: UpdateManager,
    info: UpdateInfo,
    downloaded: bool,
}

/// A newer release the user can install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableAppUpdateDto {
    pub version: String,
    pub current_version: String,
    pub release_notes_markdown: String,
    pub download_size_bytes: u64,
}

/// A newer release that has to be downloaded and installed by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNoticeDto {
    pub version: String,
    pub current_version: String,
    pub release_notes_markdown: String,
    pub release_page_url: String,
    /// Direct link to the platform's download, when the release has one.
    pub download_url: Option<String>,
    pub download_size_bytes: u64,
}

/// Runs Velopack's startup handling. Called from the bridge's `init` hook,
/// i.e. before the Dart side opens the profile.
///
/// For a Velopack installation this exits the process when the installer or
/// updater launched it only to run a hook, and applies an update that was
/// downloaded but not yet installed. Without an installation it does nothing.
pub(crate) fn run_startup_hooks() {
    VelopackApp::build().run();
}

/// Whether this installation can update itself.
#[flutter_rust_bridge::frb(sync)]
#[must_use]
pub fn app_updates_supported() -> bool {
    UpdateManager::new(NoneSource {}, None, None).is_ok()
}

/// Looks for a newer release on this installation's channel. Pre-releases
/// are offered only while a pre-release is installed, so stable users never
/// move to a beta.
///
/// # Errors
///
/// Returns an error when this installation cannot update itself or the
/// release feed cannot be read.
pub fn check_for_app_update() -> Result<Option<AvailableAppUpdateDto>, String> {
    let current_version = UpdateManager::new(NoneSource {}, None, None)
        .map_err(|error| error.to_string())?
        .get_current_version();
    let source = GithubSource::new(RELEASES_REPOSITORY, None, !current_version.pre.is_empty());
    let manager = UpdateManager::new(source, None, None).map_err(|error| error.to_string())?;
    let check = manager
        .check_for_updates()
        .map_err(|error| error.to_string())?;
    let mut pending = lock_pending()?;
    let UpdateCheck::UpdateAvailable(info) = check else {
        *pending = None;
        return Ok(None);
    };
    let target = &info.TargetFullRelease;
    let available = AvailableAppUpdateDto {
        version: target.Version.clone(),
        current_version: current_version.to_string(),
        release_notes_markdown: target.NotesMarkdown.clone(),
        download_size_bytes: target.Size,
    };
    *pending = Some(PendingUpdate {
        manager,
        info: *info,
        downloaded: false,
    });
    Ok(Some(available))
}

/// Downloads the update found by the last check, reporting progress from 0
/// to 100. Velopack verifies the package checksum before it is accepted.
///
/// # Errors
///
/// Returns an error when no update was found before, another update is
/// running, or the download or checksum verification fails.
pub fn download_app_update(progress: StreamSink<i32>) -> Result<(), String> {
    let (manager, info) = {
        let pending = lock_pending()?;
        let pending = pending
            .as_ref()
            .ok_or_else(|| "no update has been found to download".to_owned())?;
        (pending.manager.clone(), pending.info.clone())
    };

    let (sender, receiver) = mpsc::channel::<i16>();
    let forwarder = thread::spawn(move || {
        for percent in receiver {
            if progress.add(i32::from(percent)).is_err() {
                break;
            }
        }
    });
    let result = manager.download_updates(&info, Some(sender));
    // The sender moved into `download_updates` is dropped when it returns,
    // which ends the forwarding loop.
    let _ = forwarder.join();
    result.map_err(|error| error.to_string())?;

    let mut pending = lock_pending()?;
    if let Some(pending) = pending.as_mut()
        && pending.info.TargetFullRelease.Version == info.TargetFullRelease.Version
    {
        pending.downloaded = true;
    }
    Ok(())
}

/// Hands the downloaded update to Velopack's updater, which waits for this
/// process to exit, installs the update, and starts the new version. The
/// process exits immediately on success, so the client must have stored all
/// pending work before calling this.
///
/// # Errors
///
/// Returns an error when no downloaded update is pending or the updater
/// cannot be started.
pub fn apply_app_update_and_restart() -> Result<(), String> {
    let pending = lock_pending()?;
    let pending = pending
        .as_ref()
        .filter(|pending| pending.downloaded)
        .ok_or_else(|| "no downloaded update is ready to install".to_owned())?;
    pending
        .manager
        .apply_updates_and_restart(&pending.info.TargetFullRelease)
        .map_err(|error| error.to_string())
}

/// Looks up the newest GitHub release and returns it when it is newer than
/// `current_version`. Pre-releases count only while a pre-release is
/// installed. `asset_suffix` selects the download, e.g. `-macos.dmg`.
///
/// # Errors
///
/// Returns an error when `current_version` is not a semantic version or the
/// release list cannot be fetched or decoded.
pub async fn check_for_release_notice(
    current_version: String,
    asset_suffix: String,
) -> Result<Option<ReleaseNoticeDto>, String> {
    let current = Version::parse(&current_version)
        .map_err(|error| format!("invalid current version {current_version}: {error}"))?;
    let response = reqwest::Client::new()
        .get(RELEASES_API)
        .header(reqwest::header::USER_AGENT, "MAICENTA")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("release list could not be fetched: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("release list request failed with HTTP {status}"));
    }
    let releases: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("release list was invalid: {error}"))?;
    Ok(newest_release_notice(&releases, &current, &asset_suffix))
}

/// Picks the newest published release above `current` from a GitHub release
/// list.
fn newest_release_notice(
    releases: &serde_json::Value,
    current: &Version,
    asset_suffix: &str,
) -> Option<ReleaseNoticeDto> {
    let include_prereleases = !current.pre.is_empty();
    releases
        .as_array()?
        .iter()
        .filter(|release| release["draft"].as_bool() != Some(true))
        .filter(|release| include_prereleases || release["prerelease"].as_bool() != Some(true))
        .filter_map(|release| {
            let tag = release["tag_name"].as_str()?;
            let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
            (version > *current).then_some((version, release))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(version, release)| {
            let asset = release["assets"].as_array().and_then(|assets| {
                assets.iter().find(|asset| {
                    asset["name"]
                        .as_str()
                        .is_some_and(|name| name.ends_with(asset_suffix))
                })
            });
            ReleaseNoticeDto {
                version: version.to_string(),
                current_version: current.to_string(),
                release_notes_markdown: release["body"].as_str().unwrap_or_default().to_owned(),
                release_page_url: release["html_url"]
                    .as_str()
                    .map_or_else(|| format!("{RELEASES_REPOSITORY}/releases"), str::to_owned),
                download_url: asset
                    .and_then(|asset| asset["browser_download_url"].as_str())
                    .map(str::to_owned),
                download_size_bytes: asset.and_then(|asset| asset["size"].as_u64()).unwrap_or(0),
            }
        })
}

fn lock_pending() -> Result<std::sync::MutexGuard<'static, Option<PendingUpdate>>, String> {
    PENDING_UPDATE
        .lock()
        .map_err(|_| "update state was poisoned".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "prerelease": prerelease,
            "draft": false,
            "body": format!("Notes for {tag}"),
            "html_url": format!("https://github.com/Tim0theus/maicenta/releases/tag/{tag}"),
            "assets": [
                {
                    "name": format!("MAICENTA-{}-macos.dmg", &tag[1..]),
                    "browser_download_url": format!("https://example.invalid/{tag}.dmg"),
                    "size": 42,
                },
                { "name": "SHA256SUMS.txt", "browser_download_url": "https://example.invalid/sums", "size": 1 },
            ],
        })
    }

    fn notice(releases: &[serde_json::Value], current: &str) -> Option<ReleaseNoticeDto> {
        newest_release_notice(
            &serde_json::Value::Array(releases.to_vec()),
            &Version::parse(current).expect("version"),
            "-macos.dmg",
        )
    }

    #[test]
    fn offers_the_newest_release_with_its_download() {
        let found = notice(
            &[
                release("v0.1.0-beta.2", true),
                release("v0.1.0-beta.3", true),
            ],
            "0.1.0-beta.1",
        )
        .expect("update");
        assert_eq!(found.version, "0.1.0-beta.3");
        assert_eq!(found.current_version, "0.1.0-beta.1");
        assert_eq!(found.release_notes_markdown, "Notes for v0.1.0-beta.3");
        assert_eq!(
            found.download_url.as_deref(),
            Some("https://example.invalid/v0.1.0-beta.3.dmg")
        );
        assert_eq!(found.download_size_bytes, 42);
    }

    #[test]
    fn orders_prereleases_before_their_final_release() {
        let found = notice(&[release("v0.1.0", false)], "0.1.0-beta.4").expect("update");
        assert_eq!(found.version, "0.1.0");
        assert!(notice(&[release("v0.1.0-beta.4", true)], "0.1.0").is_none());
    }

    #[test]
    fn stable_installations_ignore_prereleases_and_drafts() {
        let mut draft = release("v0.3.0", false);
        draft["draft"] = serde_json::Value::Bool(true);
        assert!(notice(&[release("v0.2.0-beta.1", true), draft], "0.1.0").is_none());
    }

    #[test]
    fn ignores_older_and_unparsable_tags() {
        assert!(
            notice(
                &[release("v0.1.0-beta.1", true), release("vnext", true)],
                "0.1.0-beta.1"
            )
            .is_none()
        );
    }

    #[test]
    fn development_builds_cannot_update_themselves() {
        assert!(!app_updates_supported());
        assert!(check_for_app_update().is_err());
        assert_eq!(
            apply_app_update_and_restart(),
            Err("no downloaded update is ready to install".to_owned())
        );
    }
}
