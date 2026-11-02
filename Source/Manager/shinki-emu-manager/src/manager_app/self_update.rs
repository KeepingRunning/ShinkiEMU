use super::*;
use shinki_emu_manager::self_update::{self as updates, Release};
use shinki_emu_manager_ui_slint::{SelfUpdateAction, SelfUpdateViewModel, tr};

pub(super) struct State {
    settings: updates::Settings,
    current: semver::Version,
    job: Option<updates::Job>,
    latest: Option<Release>,
    status: &'static str,
    failure: String,
    last_checked: String,
    dismissed: bool,
    manual: bool,
    startup_pending: bool,
    retry_at: Option<u64>,
}
impl State {
    pub(super) fn load(library: &Library) -> Result<Self, String> {
        Ok(Self {
            settings: library.app_state(updates::SETTINGS_KEY).map_err(|error| error.to_string())?.unwrap_or_default(),
            current: semver::Version::parse(updates::CURRENT_VERSION).map_err(|error| error.to_string())?,
            job: None, latest: None, status: "idle", failure: String::new(),
            last_checked: String::new(), dismissed: false, manual: false,
            startup_pending: true, retry_at: None,
        })
    }
    fn available(&self) -> bool {
        self.latest.as_ref().is_some_and(|latest| latest.version.cmp_precedence(&self.current).is_gt())
    }
    fn view(&self, network_allowed: bool) -> SelfUpdateViewModel {
        let ignored = self.latest.as_ref().is_some_and(|latest| self.settings.is_ignored(&latest.version));
        SelfUpdateViewModel {
            current_version: updates::CURRENT_VERSION.into(),
            include_prerelease: self.settings.include_prerelease(&self.current),
            offline: !network_allowed,
            status: self.status.into(), failure_reason: self.failure.clone(),
            latest_version: self.latest.as_ref().map(|r| r.version.to_string()).unwrap_or_default(),
            notes_summary: self.latest.as_ref().map(|r| r.notes.clone()).unwrap_or_default(),
            download_url: self.latest.as_ref().map(|r| r.download_url.clone()).unwrap_or_default(),
            last_checked: self.last_checked.clone(),
            ignored_version: if ignored { self.latest.as_ref().map(|r| r.version.to_string()).unwrap_or_default() } else { String::new() },
            banner_visible: network_allowed && self.status == "available" && !ignored && !self.dismissed,
        }
    }
    fn accept(&mut self, result: Result<Option<Release>, updates::CheckError>) {
        let now = time::OffsetDateTime::now_utc();
        self.last_checked = format!("{:04}-{:02}-{:02} {:02}:{:02} UTC",
            now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute());
        match result {
            Ok(latest) => {
                if self.latest.as_ref().map(|release| &release.version)
                    != latest.as_ref().map(|release| &release.version) {
                    self.dismissed = false;
                }
                self.latest = latest;
                self.status = if self.available() { "available" } else { "up-to-date" };
                self.failure.clear();
                self.retry_at = None;
                tracing::info!(event = "shinki.emu.manager.self_update_checked", current = updates::CURRENT_VERSION,
                    latest = ?self.latest.as_ref().map(|r| r.version.to_string()), status = self.status);
            }
            Err(error) => {
                self.retry_at = error.retry_at;
                self.status = if self.manual { "failed" } else { "idle" };
                self.failure = if self.manual && error.retry_at.is_some() {
                    tr!("GitHub 请求已达速率限制，请稍后再试").into()
                } else if self.manual { error.reason } else { String::new() };
            }
        }
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn self_update_view(&self) -> SelfUpdateViewModel { self.self_update.view(self.network_allowed) }

    pub(super) fn begin_self_update(&mut self, manual: bool) {
        if !self.network_allowed {
            return;
        }
        if self.self_update.job.is_some() {
            self.self_update.manual |= manual;
            return;
        }
        if self.self_update.retry_at.is_some_and(|retry| {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() < retry
        }) {
            if manual {
                self.self_update.status = "failed";
                self.self_update.failure = tr!("GitHub 请求已达速率限制，请稍后再试").into();
            }
            return;
        }
        let Some(wake) = self.host_wake.clone() else { return; };
        self.self_update.manual = manual;
        self.self_update.failure.clear();
        let prerelease = self.self_update.settings.include_prerelease(&self.self_update.current);
        match updates::Job::start(prerelease, wake) {
            Ok(job) => { self.self_update.job = Some(job); self.self_update.status = "checking"; }
            Err(error) => self.self_update.accept(Err(updates::CheckError { reason: error, retry_at: None })),
        }
    }

    pub(super) fn poll_self_update(&mut self) -> bool {
        let Some(result) = self.self_update.job.as_ref().and_then(updates::Job::poll) else { return false; };
        self.self_update.job = None;
        self.self_update.accept(result);
        true
    }

    pub(super) fn self_update_network_changed(&mut self) {
        if !self.network_allowed {
            self.self_update.job = None;
            self.self_update.status = "idle";
            self.self_update.failure.clear();
        }
    }

    pub(super) fn start_self_update_once(&mut self) {
        if self.self_update.startup_pending {
            self.self_update.startup_pending = false;
            self.begin_self_update(false);
        }
    }

    pub(super) fn self_update_action(&mut self, action: SelfUpdateAction) -> Result<ManagerViewModel, String> {
        match action {
            SelfUpdateAction::CheckNow => self.begin_self_update(true),
            SelfUpdateAction::SetIncludePrerelease(enabled) => {
                let mut settings = self.self_update.settings.clone();
                settings.include_prerelease = Some(enabled);
                self.library.set_app_state(updates::SETTINGS_KEY, &settings).map_err(|error| error.to_string())?;
                self.self_update.settings = settings;
                self.self_update.job = None;
                self.self_update.latest = None;
                self.self_update.status = "idle";
                self.self_update.dismissed = false;
                self.begin_self_update(false);
            }
            SelfUpdateAction::OpenDownload => {
                let release = self.self_update.latest.as_ref().ok_or("SHINKI_EMU_MANAGER_UPDATE_UNAVAILABLE")?;
                if !updates::release_url_allowed(&release.download_url) { return Err("SHINKI_EMU_MANAGER_UPDATE_URL".into()); }
                shinki_emu_manager::platform::open_url(&release.download_url)?;
            }
            SelfUpdateAction::IgnoreVersion => {
                if let Some(latest) = &self.self_update.latest {
                    let mut settings = self.self_update.settings.clone();
                    settings.ignored_versions.insert(latest.version.to_string());
                    self.library.set_app_state(updates::SETTINGS_KEY, &settings).map_err(|error| error.to_string())?;
                    self.self_update.settings = settings;
                }
            }
            SelfUpdateAction::DismissUntilLater => self.self_update.dismissed = true,
        }
        self.model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignored_release_survives_restart_and_newer_version_is_prompted() {
        let root = tempfile::tempdir().unwrap();
        let mut library = Library::open(root.path().join("library.sqlite3")).unwrap();
        let mut state = State::load(&library).unwrap();
        let next = semver::Version::parse("99.0.0-beta.10").unwrap();
        state.settings.ignored_versions.insert(next.to_string());
        state.settings.include_prerelease = Some(false);
        library.set_app_state(updates::SETTINGS_KEY, &state.settings).unwrap();
        drop(library);
        let library = Library::open(root.path().join("library.sqlite3")).unwrap();
        let mut restored = State::load(&library).unwrap();
        assert_eq!(restored.settings.include_prerelease, Some(false));
        restored.accept(Ok(Some(Release { version: next, notes: "notes".into(), download_url: String::new() })));
        assert!(!restored.view(true).banner_visible);
        assert_eq!(restored.view(true).status, "available");
        restored.accept(Ok(Some(Release { version: semver::Version::parse("99.0.0").unwrap(), notes: String::new(), download_url: String::new() })));
        assert!(restored.view(true).banner_visible);
        restored.dismissed = true;
        assert!(!restored.view(true).banner_visible);
    }
    #[test]
    fn silent_errors_do_not_prompt_but_manual_errors_have_a_reason() {
        let library = Library::in_memory().unwrap();
        let mut state = State::load(&library).unwrap();
        state.accept(Err(updates::CheckError { reason: "bad JSON".into(), retry_at: None }));
        assert_eq!(state.status, "idle");
        assert!(state.failure.is_empty());
        state.manual = true;
        state.accept(Err(updates::CheckError { reason: "bad JSON".into(), retry_at: None }));
        assert_eq!(state.status, "failed");
        assert!(state.failure.contains("bad JSON"));
        assert!(!state.view(true).banner_visible);
    }
    #[test]
    fn offline_controller_skips_startup_and_manual_requests() {
        let root = tempfile::tempdir().unwrap();
        let mut app = ShinkiEmuManagerController::open_library(
            root.path().into(), FrameMailbox::new(), Vec::new(),
        ).unwrap();
        app.set_host_wake(std::sync::Arc::new(|| {}));
        assert!(app.self_update.job.is_none());
        assert!(!app.self_update.startup_pending);
        let view = app.self_update_action(SelfUpdateAction::CheckNow).unwrap();
        assert!(view.self_update.offline);
        assert_eq!(view.self_update.status, "idle");
        assert!(app.self_update.job.is_none());
        app.self_update_action(SelfUpdateAction::SetIncludePrerelease(false)).unwrap();
        assert!(app.self_update.job.is_none());
        assert_eq!(app.library.app_state::<updates::Settings>(updates::SETTINGS_KEY)
            .unwrap().unwrap().include_prerelease, Some(false));
    }

    #[test]
    fn equal_version_is_current_and_new_release_clears_session_dismissal() {
        let library = Library::in_memory().unwrap();
        let mut state = State::load(&library).unwrap();
        state.accept(Ok(Some(Release { version: state.current.clone(), notes: String::new(), download_url: String::new() })));
        assert_eq!(state.status, "up-to-date");
        assert!(!state.view(true).banner_visible);
        state.dismissed = true;
        state.accept(Ok(Some(Release { version: semver::Version::parse("99.0.0").unwrap(), notes: String::new(), download_url: String::new() })));
        assert!(state.view(true).banner_visible);
        assert!(!state.view(false).banner_visible);
    }
}
