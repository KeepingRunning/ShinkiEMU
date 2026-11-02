//! Manager release detection. No package download or installation is performed.
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::{Duration, SystemTime, UNIX_EPOCH}};

pub const SETTINGS_KEY: &str = "manager.self_update";
const RELEASES_API: &str = "https://api.github.com/repos/Project-Shinkirou/ShinkiEMU/releases";
const RELEASES: &str = "https://github.com/Project-Shinkirou/ShinkiEMU/releases/";
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 20;

/// Matches the version displayed by the Manager's About page.
pub const CURRENT_VERSION: &str = crate::version();

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub include_prerelease: Option<bool>,
    pub ignored_versions: BTreeSet<String>,
}
impl Settings {
    pub fn include_prerelease(&self, current: &Version) -> bool {
        self.include_prerelease.unwrap_or(!current.pre.is_empty())
    }
    pub fn is_ignored(&self, version: &Version) -> bool {
        self.ignored_versions.iter().any(|ignored| {
            Version::parse(ignored).is_ok_and(|ignored| ignored.cmp_precedence(version).is_eq())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub download_url: String,
}

#[derive(Debug, Clone)]
pub struct CheckError {
    pub reason: String,
    pub retry_at: Option<u64>,
}
impl CheckError {
    fn invalid(reason: &str) -> Self { Self { reason: reason.into(), retry_at: None } }
}

#[derive(Deserialize)]
struct RemoteRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<RemoteAsset>,
}
#[derive(Deserialize)]
struct RemoteAsset {
    name: String,
    browser_download_url: String,
}

/// Only repository release pages/assets can reach any platform URL opener.
pub fn release_url_allowed(value: &str) -> bool {
    if value.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return false;
    }
    let Ok(url) = url::Url::parse(value) else { return false; };
    if url.scheme() != "https" || url.host_str() != Some("github.com")
        || !url.username().is_empty() || url.password().is_some()
        || url.port().is_some() || url.query().is_some()
        || !matches!(url.fragment(), None | Some("assets"))
    { return false; }
    let Some(path) = url.path().strip_prefix("/Project-Shinkirou/ShinkiEMU/releases/") else { return false; };
    let mut parts = path.split('/');
    let route = parts.next();
    let Some(version) = parts.next().and_then(|tag| tag.strip_prefix("manager-v")) else {
        return false;
    };
    if version.contains('%') || Version::parse(version).is_err() {
        return false;
    }
    match (route, parts.next(), parts.next()) {
        (Some("tag"), None, None) => true,
        (Some("download"), Some(asset), None) => !asset.is_empty() && !asset.contains('%'),
        _ => false,
    }
}

fn platform_asset(name: &str, platform: &str, architecture: &str) -> bool {
    if !name.starts_with("shinki-emu-manager-") || name.contains("sources") { return false; }
    match platform {
        "android" => name.ends_with("-android-universal.apk"),
        "ohos" => name.ends_with(&format!("-ohos-{architecture}-debug-test-signed.hap")),
        "ios" => name.contains(&format!("-ios-{architecture}-device")) && name.ends_with(".zip"),
        "windows" | "linux" | "macos" => name.ends_with(&format!("-{platform}-{architecture}.zip")),
        _ => false,
    }
}

fn select_release(
    entries: Vec<serde_json::Value>, include_prerelease: bool,
    platform: &str, architecture: &str, latest: &mut Option<Release>,
) {
    for entry in entries {
        let Ok(entry) = serde_json::from_value::<RemoteRelease>(entry) else { continue; };
        let Some(version) = entry.tag_name.strip_prefix("manager-v")
            .filter(|v| v.len() <= 128).and_then(|v| Version::parse(v).ok()) else { continue; };
        if entry.draft || (!include_prerelease && (entry.prerelease || !version.pre.is_empty()))
            || latest.as_ref().is_some_and(|old| !version.cmp_precedence(&old.version).is_gt())
        { continue; }
        let page = format!("{RELEASES}tag/manager-v{version}");
        if !release_url_allowed(&page) { continue; }
        let download_url = entry.assets.into_iter().find(|asset| {
            platform_asset(&asset.name, platform, architecture)
                && release_url_allowed(&asset.browser_download_url)
                && asset.browser_download_url.starts_with(&format!("{RELEASES}download/manager-v{version}/"))
        }).map_or_else(|| format!("{page}#assets"), |asset| asset.browser_download_url);
        // Slint displays this as text, never as HTML or actionable remote markup.
        let notes = entry.body.unwrap_or_default().chars().filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .take(32_768).collect::<String>().trim().to_owned();
        *latest = Some(Release { version, notes, download_url });
    }
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn response_error(status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) -> CheckError {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status == reqwest::StatusCode::FORBIDDEN {
        let now = now_seconds();
        let retry_at = headers.get("retry-after").and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok()).map(|delay| now.saturating_add(delay))
            .or_else(|| headers.get("x-ratelimit-reset").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()))
            .unwrap_or_else(|| now.saturating_add(60));
        CheckError { reason: format!("GitHub HTTP {} (retry after Unix {retry_at})", status.as_u16()), retry_at: Some(retry_at) }
    } else {
        CheckError::invalid(&format!("GitHub HTTP {}", status.as_u16()))
    }
}

/// Uses the Manager's existing reqwest transport and platform TLS verifier.
async fn fetch(
    client: &reqwest::Client, endpoint: &str, network_allowed: bool,
    include_prerelease: bool, platform: &str, architecture: &str,
) -> Result<Option<Release>, CheckError> {
    // This guard is before request construction, including for tests/local servers.
    if !network_allowed { return Err(CheckError::invalid("offline")); }
    let mut latest = None;
    for page in 1..=MAX_PAGES {
        let mut response = client.get(endpoint)
            .query(&[("per_page", PAGE_SIZE), ("page", page)])
            .header(reqwest::header::USER_AGENT, format!("ShinkiEMU-Manager/{}", crate::version()))
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send().await.map_err(|error| CheckError::invalid(&error.without_url().to_string()))?;
        if !response.status().is_success() { return Err(response_error(response.status(), response.headers())); }
        if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
            return Err(CheckError::invalid("release response too large"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| CheckError::invalid(&error.without_url().to_string()))? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES { return Err(CheckError::invalid("release response too large")); }
            bytes.extend_from_slice(&chunk);
        }
        let entries: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
            .map_err(|_| CheckError::invalid("invalid release JSON"))?;
        let count = entries.len();
        select_release(entries, include_prerelease, platform, architecture, &mut latest);
        if count < PAGE_SIZE { return Ok(latest); }
    }
    Err(CheckError::invalid("release pagination limit exceeded"))
}

/// One cancellable background request; dropping it cancels requests in flight.
pub struct Job {
    receiver: crossbeam_channel::Receiver<Result<Option<Release>, CheckError>>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}
impl Drop for Job {
    fn drop(&mut self) { if let Some(cancel) = self.cancel.take() { let _ = cancel.send(()); } }
}
impl Job {
    pub fn start(include_prerelease: bool, wake: crate::HostWake) -> Result<Self, String> {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        std::thread::Builder::new().name("manager-self-update".into()).spawn(move || {
            let result = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
                    .map_err(|error| CheckError::invalid(&error.to_string()))?;
                runtime.block_on(async {
                    let client = reqwest::Client::builder().timeout(Duration::from_secs(20))
                        .redirect(reqwest::redirect::Policy::none()).build()
                        .map_err(|error| CheckError::invalid(&error.without_url().to_string()))?;
                    tokio::select! {
                        biased;
                        _ = cancelled => Err(CheckError::invalid("cancelled")),
                        result = fetch(&client, RELEASES_API, true, include_prerelease, platform(), std::env::consts::ARCH) => result,
                    }
                })
            })();
            let _ = sender.send(result);
            wake();
        }).map_err(|error| error.to_string())?;
        Ok(Self { receiver, cancel: Some(cancel) })
    }
    pub fn poll(&self) -> Option<Result<Option<Release>, CheckError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(CheckError::invalid("update worker stopped"))),
        }
    }
}
fn platform() -> &'static str {
    if cfg!(target_env = "ohos") { "ohos" } else { std::env::consts::OS }
}

#[cfg(test)]
mod tests;
