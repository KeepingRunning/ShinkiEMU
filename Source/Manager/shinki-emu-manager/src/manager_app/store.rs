use super::*;
use serde::{Deserialize, Serialize};
use shinki_emu_manager_ui_slint::{
    ChoiceViewModel, StoreAction, StoreDepotViewModel, StoreGameViewModel, StoreLoadState,
    StoreLoginState, StoreTransferState, StoreTransferViewModel, StoreViewModel,
};
use shinki_emu_store::{self as provider, SecretStore};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tokio::sync::mpsc as async_mpsc;
use shinki_emu_manager_ui_slint::tr;

/// Platforms whose Manager host can show the embedded sign-in webview.
const WEB_LOGIN_SUPPORTED: bool = !cfg!(target_env = "ohos");

fn registry() -> provider::Registry {
    let mut registry = provider::Registry::new();
    #[cfg(feature = "store-steam")]
    registry
        .register(shinki_emu_steam::Provider)
        .expect("unique Steam provider");
    #[cfg(feature = "store-dlsite")]
    registry
        .register(shinki_emu_dlsite::Provider)
        .expect("unique DLsite provider");
    #[cfg(feature = "store-dmm")]
    registry
        .register(shinki_emu_dmm::Provider)
        .expect("unique DMM provider");
    registry
}
fn provider_view(registry: &provider::Registry, id: &str, root: &Path) -> StoreViewModel {
    let mut view = StoreViewModel {
        providers: registry
            .providers()
            .iter()
            .map(|p| ChoiceViewModel {
                id: p.descriptor().id.into(),
                label: p.descriptor().display_name.into(),
            })
            .collect(),
        provider_id: id.into(),
        library_root: root
            .parent()
            .and_then(Path::parent)
            .unwrap_or(root)
            .to_string_lossy()
            .into_owned(),
        ..Default::default()
    };
    if let Some(p) = registry.get(id) {
        let d = p.descriptor();
        view.provider_name = d.display_name.into();
        view.can_qr = d.login_methods.contains(&provider::LoginKind::Qr);
        view.can_credentials = d.login_methods.contains(&provider::LoginKind::Credentials);
        view.can_web = d.web_login().is_some() && WEB_LOGIN_SUPPORTED;
        view.can_import_local = d.login_methods.contains(&provider::LoginKind::LocalImport)
            && cfg!(any(
                target_os = "windows",
                target_os = "linux",
                target_os = "macos"
            ));
        view.has_branches = d.capabilities.branches;
        view.has_depots = d.capabilities.depots;
        view.can_verify = d.capabilities.verify;
    }
    view
}
struct StoreSecrets {
    root: PathBuf,
    legacy_root: PathBuf,
}
impl StoreSecrets {
    fn marker(&self, provider: &str) -> Result<PathBuf, provider::StoreError> {
        if !provider
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'-')
            || provider.is_empty()
        {
            return Err(provider::StoreError::Metadata);
        }
        Ok(self.root.join(provider).join("login-present"))
    }
}
impl SecretStore for StoreSecrets {
    fn load(&self, provider: &str) -> Result<Option<provider::StoredLogin>, provider::StoreError> {
        let marker = self.marker(provider)?;
        let legacy = !marker.try_exists()?
            && provider == "steam"
            && self.legacy_root.join("steam-login-present").try_exists()?;
        if !marker.try_exists()? && !legacy {
            return Ok(None);
        }
        let store = ManagerSecretStore::open().map_err(|_| provider::StoreError::SecretStore)?;
        let secret = zeroize::Zeroizing::new(
            store
                .resolve(&format!("{provider}.login"))
                .map_err(|_| provider::StoreError::SecretStore)?,
        );
        if legacy {
            #[derive(Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
            struct LegacyLogin {
                account: String,
                refresh_token: String,
            }
            let old: LegacyLogin =
                serde_json::from_str(&secret).map_err(|_| provider::StoreError::SecretStore)?;
            let migrated = provider::StoredLogin {
                account: old.account.clone(),
                secret: old.refresh_token.clone(),
            };
            self.save(provider, &migrated)?;
            std::fs::remove_file(self.legacy_root.join("steam-login-present"))?;
            return Ok(Some(migrated));
        }
        serde_json::from_str(&secret)
            .map(Some)
            .map_err(|_| provider::StoreError::SecretStore)
    }
    fn save(
        &self,
        provider: &str,
        login: &provider::StoredLogin,
    ) -> Result<(), provider::StoreError> {
        let marker = self.marker(provider)?;
        std::fs::create_dir_all(marker.parent().ok_or(provider::StoreError::Metadata)?)?;
        let encoded = zeroize::Zeroizing::new(
            serde_json::to_string(login).map_err(|_| provider::StoreError::SecretStore)?,
        );
        let store = ManagerSecretStore::open().map_err(|_| provider::StoreError::SecretStore)?;
        store
            .store(&format!("{provider}.login"), &encoded)
            .map_err(|_| provider::StoreError::SecretStore)?;
        std::fs::write(marker, [])?;
        Ok(())
    }
    fn clear(&self, provider: &str) -> Result<(), provider::StoreError> {
        let marker = self.marker(provider)?;
        let existed = marker.try_exists()?;
        let legacy = self.legacy_root.join("steam-login-present");
        let legacy_exists = provider == "steam" && legacy.try_exists()?;
        let store = ManagerSecretStore::open().map_err(|_| provider::StoreError::SecretStore)?;
        if store.delete(&format!("{provider}.login")).is_err() && (existed || legacy_exists) {
            return Err(provider::StoreError::SecretStore);
        }
        if existed {
            std::fs::remove_file(marker)?;
        }
        if legacy_exists {
            std::fs::remove_file(legacy)?;
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Installation {
    provider: String,
    title_id: String,
    branch: String,
    depots: Vec<String>,
    directory: PathBuf,
}
struct CompletedInstallation {
    name: String,
    directory: PathBuf,
}

struct Shared {
    view: Mutex<StoreViewModel>,
    changed: AtomicBool,
    busy: AtomicBool,
    wake: Mutex<Option<HostWake>>,
}
impl Shared {
    fn update(&self, update: impl FnOnce(&mut StoreViewModel)) {
        update(&mut self.view.lock().unwrap_or_else(|e| e.into_inner()));
        self.changed.store(true, Ordering::Release);
        if let Some(wake) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            wake();
        }
    }
}

/// Work for the provider thread: a UI action, or the cookies the embedded
/// sign-in webview captured for the selected provider.
enum Job {
    Action(StoreAction),
    WebLogin(Vec<provider::WebCookie>),
}

enum StoreCommand {
    Run(Job, provider::CancelToken),
    Disconnect,
}

pub(super) struct StoreRuntime {
    shared: Arc<Shared>,
    commands: async_mpsc::Sender<StoreCommand>,
    codes: async_mpsc::Sender<String>,
    cancel: Arc<Mutex<provider::CancelToken>>,
    completed: mpsc::Receiver<CompletedInstallation>,
    pending: Option<Worker>,
    imports: VecDeque<PathBuf>,
    /// Embedded-webview sign-in rules by provider id.
    web_logins: Vec<(&'static str, &'static provider::WebLogin)>,
}
struct Worker {
    shared: Arc<Shared>,
    commands: async_mpsc::Receiver<StoreCommand>,
    codes: async_mpsc::Receiver<String>,
    cancel: provider::CancelToken,
    completed: mpsc::SyncSender<CompletedInstallation>,
    session: Option<Box<dyn provider::StoreSession>>,
    games: Vec<provider::OwnedTitle>,
    installations: Vec<Installation>,
    path: PathBuf,
    secrets: StoreSecrets,
    registry: provider::Registry,
    selected_provider: String,
    detail: Option<provider::TitleDetail>,
    work_root: PathBuf,
}

impl StoreRuntime {
    pub(super) fn new(data_dir: &Path) -> Self {
        let registry = registry();
        let selected_provider = registry
            .providers()
            .first()
            .map_or("", |p| p.descriptor().id)
            .to_owned();
        let work_root = data_dir.join(".shinki").join("store");
        let web_logins = registry
            .providers()
            .iter()
            .filter_map(|p| {
                let d = p.descriptor();
                d.web_login().map(|login| (d.id, login))
            })
            .collect();
        let shared = Arc::new(Shared {
            view: Mutex::new(provider_view(&registry, &selected_provider, &work_root)),
            changed: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            wake: Mutex::new(None),
        });
        let (commands, receiver) = async_mpsc::channel(8);
        let (codes, code_receiver) = async_mpsc::channel(2);
        let cancel_receiver = provider::CancelToken::new();
        let cancel = Arc::new(Mutex::new(cancel_receiver.clone()));
        let (completed, completion_receiver) = mpsc::sync_channel(8);
        let worker = Worker {
            shared: shared.clone(),
            commands: receiver,
            codes: code_receiver,
            cancel: cancel_receiver,
            completed,
            session: None,
            games: Vec::new(),
            installations: Vec::new(),
            path: work_root.join("installations.json"),
            secrets: StoreSecrets {
                root: work_root.clone(),
                legacy_root: data_dir.into(),
            },
            registry,
            selected_provider,
            detail: None,
            work_root,
        };
        Self {
            shared,
            commands,
            codes,
            cancel,
            completed: completion_receiver,
            pending: Some(worker),
            imports: VecDeque::new(),
            web_logins,
        }
    }
    pub(super) fn view(&self) -> StoreViewModel {
        let mut view = self
            .shared
            .view
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let query = view.query.to_lowercase();
        view.games.retain(|game| {
            game.name.to_lowercase().contains(&query) || game.title_id.contains(&query)
        });
        view
    }
    pub(super) fn cancel(&self) {
        if self.shared.busy.load(Ordering::Acquire) {
            self.cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancel();
            self.shared.update(|v| {
                if matches!(
                    v.transfer.state,
                    StoreTransferState::Preparing
                        | StoreTransferState::Downloading
                        | StoreTransferState::Verifying
                ) {
                    v.transfer.state = StoreTransferState::Cancelling;
                }
            });
        }
    }
    pub(super) fn suspend(&self) -> Result<(), String> {
        self.cancel();
        if self.pending.is_none() {
            self.commands.try_send(StoreCommand::Disconnect)
                .map_err(|_| tr!("无法停止 Store 连接，请重新打开管理器").to_owned())?;
        }
        Ok(())
    }
    pub(super) fn set_wake(&self, wake: HostWake) {
        *self.shared.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake);
    }
    pub(super) fn changed(&self) -> bool {
        self.shared.changed.swap(false, Ordering::AcqRel)
    }
    fn completed(&self) -> Option<CompletedInstallation> {
        self.completed.try_recv().ok()
    }
    pub(super) fn dispatch(&mut self, action: StoreAction) -> Result<(), String> {
        if let StoreAction::SubmitGuard { code } = action {
            return self
                .codes
                .try_send(code)
                .map_err(|_| tr!("请等待验证码提交完成").into());
        }
        if matches!(action, StoreAction::Cancel) {
            self.cancel();
            return Ok(());
        }
        if matches!(action, StoreAction::Logout) {
            self.cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancel();
        } else if let StoreAction::Search { query } = action {
            self.shared.update(|v| v.query = query);
            return Ok(());
        } else {
            self.begin()?;
        }
        self.start(Job::Action(action))
    }
    /// The embedded-webview sign-in rule of the selected provider, while no
    /// other Store operation runs.
    pub(super) fn web_login(&self) -> Result<&'static provider::WebLogin, String> {
        if self.shared.busy.load(Ordering::Acquire) {
            return Err(tr!("请等待当前 Store 操作完成，或先取消").into());
        }
        let id = self
            .shared
            .view
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .provider_id
            .clone();
        self.web_logins
            .iter()
            .find(|(provider, _)| *provider == id)
            .map(|(_, login)| *login)
            .ok_or_else(|| provider::StoreError::UnsupportedLogin.to_string())
    }
    /// Sign in to the selected provider with the cookies its webview captured.
    pub(super) fn web_signed_in(&mut self, cookies: Vec<provider::WebCookie>) -> Result<(), String> {
        self.begin()?;
        self.start(Job::WebLogin(cookies))
    }
    fn begin(&self) -> Result<(), String> {
        if self.shared.busy.swap(true, Ordering::AcqRel) {
            return Err(tr!("请等待当前 Store 操作完成，或先取消").into());
        }
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = provider::CancelToken::new();
        Ok(())
    }
    fn start(&mut self, job: Job) -> Result<(), String> {
        if let Some(worker) = self.pending.take() {
            let shared = worker.shared.clone();
            std::thread::Builder::new()
                .name("store-provider".into())
                .spawn(move || {
                    match tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime.block_on(worker.run()),
                        Err(error) => shared.update(|v| {
                            v.error = error.to_string();
                            v.login_state = StoreLoginState::Error;
                        }),
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        self.commands
            .try_send(StoreCommand::Run(
                job,
                self.cancel
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            ))
            .map_err(|_| {
                self.shared.busy.store(false, Ordering::Release);
                tr!("Store 工作线程不可用，请重新打开管理器")
                    .into()
            })
    }
}
impl Drop for StoreRuntime {
    fn drop(&mut self) {
        self.cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel();
    }
}

impl Worker {
    fn view(&self) -> StoreViewModel {
        self.shared
            .view
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    async fn run(mut self) {
        if let Err(error) = self.load_installations() {
            self.shared.update(|v| v.error = error.to_string());
        }
        while let Some(command) = self.commands.recv().await {
            let action = match command {
                StoreCommand::Disconnect => {
                    self.session = None;
                    self.shared.update(|v| {
                        v.login_state = StoreLoginState::SignedOut;
                        v.qr_url.clear();
                        v.branch_password.clear();
                    });
                    self.shared.busy.store(false, Ordering::Release);
                    continue;
                }
                StoreCommand::Run(job, cancel) => {
                    self.cancel = cancel;
                    job
                }
            };
            let download = matches!(
                action,
                Job::Action(StoreAction::Download | StoreAction::Verify)
            );
            let auth = matches!(
                action,
                Job::WebLogin(_)
                    | Job::Action(
                        StoreAction::Open
                            | StoreAction::SelectProvider { .. }
                            | StoreAction::Login { .. }
                            | StoreAction::QrLogin
                            | StoreAction::ImportLocal
                    )
            );
            let logout = matches!(action, Job::Action(StoreAction::Logout));
            let cancel = self.cancel.clone();
            let result = if logout || download {
                self.run_job(action).await
            } else {
                tokio::select! { result = self.run_job(action) => result, _ = cancel.cancelled() => Err(provider::StoreError::Cancelled) }
            };
            match result {
                Err(provider::StoreError::Cancelled) => {
                    if auth {
                        self.session = None;
                    }
                    self.shared.update(|v| {
                        if auth {
                            v.login_state = StoreLoginState::SignedOut;
                            v.qr_url.clear();
                        }
                        if download {
                            v.transfer.state = StoreTransferState::Cancelled;
                        }
                        if v.library_state == StoreLoadState::Loading {
                            v.library_state = StoreLoadState::Idle;
                        }
                    });
                }
                Err(error) => {
                    if error == provider::StoreError::SessionExpired {
                        self.session = None;
                    }
                    self.shared.update(|v| {
                        v.error = error.to_string();
                        if (auth || logout || error == provider::StoreError::SessionExpired)
                            && self.session.is_none()
                        {
                            v.login_state = StoreLoginState::Error;
                        }
                        if v.library_state == StoreLoadState::Loading {
                            v.library_state = StoreLoadState::Error;
                        }
                        if download {
                            v.transfer.state = StoreTransferState::Error;
                            v.transfer.error = error.to_string();
                        }
                    });
                }
                Ok(()) => {}
            }
            self.shared.busy.store(false, Ordering::Release);
            self.shared.update(|_| {});
        }
    }
    fn load_installations(&mut self) -> Result<(), provider::StoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                self.installations =
                    serde_json::from_slice(&bytes).map_err(|_| provider::StoreError::Metadata)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Convert old Steam records once; existing installations stay in place.
                let legacy_path = self.secrets.legacy_root.join("steam-installations.json");
                let bytes = match std::fs::read(&legacy_path) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(e) => return Err(e.into()),
                };
                #[derive(Deserialize)]
                struct OldInstallation {
                    app_id: u32,
                    branch: String,
                    depots: Vec<u32>,
                    directory: PathBuf,
                }
                let old: Vec<OldInstallation> =
                    serde_json::from_slice(&bytes).map_err(|_| provider::StoreError::Metadata)?;
                self.installations = old
                    .into_iter()
                    .map(|i| Installation {
                        provider: "steam".into(),
                        title_id: i.app_id.to_string(),
                        branch: i.branch,
                        depots: i.depots.into_iter().map(|id| id.to_string()).collect(),
                        directory: i.directory,
                    })
                    .collect();
                self.persist()?;
                std::fs::remove_file(legacy_path)?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    fn persist(&self) -> Result<(), provider::StoreError> {
        std::fs::create_dir_all(&self.work_root)?;
        let bytes =
            serde_json::to_vec(&self.installations).map_err(|_| provider::StoreError::Metadata)?;
        let temp = self.path.with_extension("json.new");
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, &self.path)?;
        Ok(())
    }
    fn selected(&self) -> Result<Arc<dyn provider::StoreProvider>, provider::StoreError> {
        self.registry
            .get(&self.selected_provider)
            .ok_or(provider::StoreError::Unavailable)
    }
    async fn authenticate(
        &mut self,
        request: provider::LoginRequest,
    ) -> Result<(), provider::StoreError> {
        self.session = None;
        self.games.clear();
        self.detail = None;
        self.shared.update(|v| {
            v.games.clear();
            v.depots.clear();
            v.selected_title_id.clear();
            v.error.clear();
            v.qr_url.clear();
        });
        while self.codes.try_recv().is_ok() {}
        let shared = self.shared.clone();
        self.session = Some(
            self.selected()?
                .login(
                    request,
                    &self.secrets,
                    &mut self.codes,
                    Arc::new(move |update| shared.update(|v| apply_login(v, update))),
                    self.cancel.clone(),
                )
                .await?,
        );
        self.refresh().await
    }
    async fn restore(&mut self) -> Result<(), provider::StoreError> {
        self.session = self
            .selected()?
            .restore(&self.secrets, self.cancel.clone())
            .await?;
        if let Some(session) = &self.session {
            self.shared.update(|v| {
                apply_login(v, provider::LoginUpdate::SignedIn(session.account().into()))
            });
            self.refresh().await?;
        }
        Ok(())
    }
    async fn refresh(&mut self) -> Result<(), provider::StoreError> {
        self.shared.update(|v| {
            v.library_state = StoreLoadState::Loading;
            v.error.clear();
        });
        self.games = self
            .session
            .as_ref()
            .ok_or(provider::StoreError::SessionExpired)?
            .library()
            .await?;
        let games = self
            .games
            .iter()
            .map(|game| game_view(game, &self.selected_provider, &self.installations))
            .collect();
        self.shared.update(|v| {
            v.games = games;
            v.library_state = StoreLoadState::Ready;
        });
        self.check_updates().await?;
        Ok(())
    }
    async fn check_updates(&self) -> Result<(), provider::StoreError> {
        self.shared
            .update(|v| v.library_state = StoreLoadState::Loading);
        let session = self
            .session
            .as_ref()
            .ok_or(provider::StoreError::SessionExpired)?;
        for i in self
            .installations
            .iter()
            .filter(|i| i.provider == self.selected_provider)
        {
            let view = self.view();
            let options = provider::PlanOptions {
                branch: i.branch.clone(),
                depots: i.depots.clone(),
                branch_password: zeroize::Zeroizing::new(
                    if view.selected_title_id == i.title_id && view.selected_branch == i.branch {
                        view.branch_password
                    } else {
                        String::new()
                    },
                ),
                verify: true,
            };
            let update = session
                .has_update(
                    &provider::TitleId(i.title_id.clone()),
                    &i.directory,
                    options,
                )
                .await?;
            self.shared.update(|v| {
                if let Some(game) = v.games.iter_mut().find(|g| g.title_id == i.title_id) {
                    game.update_available = update == provider::UpdateState::Available;
                    game.update_unknown = update == provider::UpdateState::Unknown;
                }
            });
        }
        self.shared
            .update(|v| v.library_state = StoreLoadState::Ready);
        Ok(())
    }
    async fn run_job(&mut self, job: Job) -> Result<(), provider::StoreError> {
        match job {
            Job::WebLogin(cookies) => {
                self.authenticate(provider::LoginRequest::Web { cookies })
                    .await
            }
            Job::Action(action) => self.execute(action).await,
        }
    }
    async fn execute(&mut self, action: StoreAction) -> Result<(), provider::StoreError> {
        match action {
            StoreAction::SelectProvider { id } => {
                if self.registry.get(&id).is_none() {
                    return Err(provider::StoreError::Unavailable);
                }
                self.session = None;
                self.games.clear();
                self.detail = None;
                self.selected_provider = id;
                let view = provider_view(&self.registry, &self.selected_provider, &self.work_root);
                self.shared.update(|v| *v = view);
                self.restore().await?;
            }
            StoreAction::Open => {
                if self.session.is_none() {
                    self.restore().await?;
                }
            }
            StoreAction::OpenDestination => {
                let destination = self.view().transfer.destination;
                if destination.is_empty() {
                    return Err(provider::StoreError::LibraryRootRequired);
                }
                shinki_emu_manager::platform::open_path(Path::new(&destination))
                    .map_err(|_| provider::StoreError::Io)?;
            }
            StoreAction::Login { account, password } => {
                self.authenticate(provider::LoginRequest::Credentials {
                    account,
                    password: zeroize::Zeroizing::new(password),
                })
                .await?
            }
            StoreAction::QrLogin => self.authenticate(provider::LoginRequest::Qr).await?,
            StoreAction::ImportLocal => {
                self.authenticate(provider::LoginRequest::LocalImport)
                    .await?
            }
            StoreAction::Logout => {
                if let Some(session) = &self.session {
                    session.logout(&self.secrets).await?;
                } else {
                    self.secrets.clear(&self.selected_provider)?;
                }
                self.session = None;
                self.games.clear();
                self.detail = None;
                let view = provider_view(&self.registry, &self.selected_provider, &self.work_root);
                self.shared.update(|v| *v = view);
            }
            StoreAction::RefreshLibrary => self.refresh().await?,
            StoreAction::SelectGame { title_id } => {
                self.shared.update(|v| {
                    v.library_state = StoreLoadState::Loading;
                    v.error.clear();
                });
                let detail = self
                    .session
                    .as_ref()
                    .ok_or(provider::StoreError::SessionExpired)?
                    .detail(&provider::TitleId(title_id.clone()))
                    .await?;
                let installed = self
                    .installations
                    .iter()
                    .find(|i| i.provider == self.selected_provider && i.title_id == title_id);
                self.shared.update(|v| {
                    v.selected_title_id = title_id;
                    v.branches = detail
                        .branches
                        .iter()
                        .map(|b| ChoiceViewModel {
                            id: b.id.clone(),
                            label: b.name.clone(),
                        })
                        .collect();
                    v.selected_branch = installed.map_or_else(
                        || {
                            detail
                                .branches
                                .iter()
                                .find(|b| b.id == "public")
                                .or_else(|| detail.branches.first())
                                .map_or_else(String::new, |b| b.id.clone())
                        },
                        |i| i.branch.clone(),
                    );
                    v.branch_password.clear();
                    v.depots = detail
                        .depots
                        .iter()
                        .map(|d| StoreDepotViewModel {
                            depot_id: d.id.clone(),
                            name: d.name.clone(),
                            platform: d.platform.clone(),
                            language: d.language.clone(),
                            size_bytes: d.sizes.get(&v.selected_branch).copied().unwrap_or(0),
                            selected: installed.map_or_else(
                                || {
                                    (d.platform.is_empty()
                                        || d.platform.split(',').any(|p| p == "windows"))
                                        && (d.language.is_empty() || d.language == "english")
                                },
                                |i| i.depots.contains(&d.id),
                            ),
                        })
                        .collect();
                    if let Some(game) = v.games.iter_mut().find(|g| g.title_id == detail.title.id.0)
                    {
                        game.unavailable_reason =
                            detail.title.unavailable_reason.clone().unwrap_or_default();
                    }
                    v.library_state = StoreLoadState::Ready;
                });
                self.detail = Some(detail);
            }
            StoreAction::SelectDepot { depot_id, selected } => self.shared.update(|v| {
                if let Some(d) = v.depots.iter_mut().find(|d| d.depot_id == depot_id) {
                    d.selected = selected;
                }
            }),
            StoreAction::SelectBranch { branch, password } => self.shared.update(|v| {
                for depot in &mut v.depots {
                    depot.size_bytes = self
                        .detail
                        .as_ref()
                        .and_then(|d| d.depots.iter().find(|d| d.id == depot.depot_id))
                        .and_then(|d| d.sizes.get(&branch))
                        .copied()
                        .unwrap_or(0);
                }
                v.selected_branch = branch;
                v.branch_password = password;
            }),
            StoreAction::SetLibraryRoot { path } => self.shared.update(|v| v.library_root = path),
            StoreAction::Download | StoreAction::Verify => {
                self.download(matches!(action, StoreAction::Verify)).await?
            }
            StoreAction::CheckUpdates => self.check_updates().await?,
            StoreAction::Search { .. }
            | StoreAction::SubmitGuard { .. }
            | StoreAction::Cancel
            | StoreAction::ChooseLibraryRoot
            | StoreAction::WebLogin
            | StoreAction::CancelWebLogin => {}
        }
        Ok(())
    }
    async fn download(&mut self, verify: bool) -> Result<(), provider::StoreError> {
        let view = self.view();
        if view.library_root.trim().is_empty() {
            return Err(provider::StoreError::LibraryRootRequired);
        }
        if view.selected_title_id.is_empty() {
            return Err(provider::StoreError::GameRequired);
        }
        let session = self
            .session
            .as_ref()
            .ok_or(provider::StoreError::SessionExpired)?;
        self.shared.update(|v| {
            v.error.clear();
            v.transfer = StoreTransferViewModel {
                title_id: view.selected_title_id.clone(),
                state: if verify {
                    StoreTransferState::Verifying
                } else {
                    StoreTransferState::Preparing
                },
                ..Default::default()
            };
        });
        let options = provider::PlanOptions {
            branch: view.selected_branch,
            depots: view
                .depots
                .iter()
                .filter(|d| d.selected)
                .map(|d| d.depot_id.clone())
                .collect(),
            branch_password: zeroize::Zeroizing::new(view.branch_password),
            verify: true,
        };
        let id = provider::TitleId(view.selected_title_id);
        let plan = tokio::select! { result = session.plan(&id, options) => result?, _ = self.cancel.cancelled() => return Err(provider::StoreError::Cancelled) };
        self.shared.update(|v| v.branch_password.clear());
        // IDs are opaque strings; hex encoding prevents separators and case collisions.
        let directory = self
            .installations
            .iter()
            .find(|i| i.provider == self.selected_provider && i.title_id == id.0)
            .map_or_else(
                || {
                    Path::new(&view.library_root)
                        .join(".shinki")
                        .join("store")
                        .join(&self.selected_provider)
                        .join("installs")
                        .join(hex::encode(id.0.as_bytes()))
                },
                |i| i.directory.clone(),
            );
        let work_dir = self.work_root.join(&self.selected_provider);
        std::fs::create_dir_all(&work_dir)?;
        let shared = self.shared.clone();
        let result = session
            .download(
                &plan,
                &directory,
                &work_dir,
                Arc::new(move |p| {
                    shared.update(|v| {
                        if !verify {
                            v.transfer.state = StoreTransferState::Downloading;
                        }
                        v.transfer.bytes_done = p.bytes_done;
                        v.transfer.bytes_total = p.bytes_total;
                        v.transfer.files_done = p.files_done;
                        v.transfer.files_total = p.files_total;
                        v.transfer.speed_bytes_per_second = p.speed_bytes_per_second;
                        v.transfer.eta_seconds = p.eta_seconds;
                        v.transfer.progress =
                            (p.bytes_total > 0).then(|| p.bytes_done as f32 / p.bytes_total as f32);
                    })
                }),
                self.cancel.clone(),
            )
            .await?;
        if result.cancelled {
            return Err(provider::StoreError::Cancelled);
        }
        let mut depots = plan.depots.clone();
        if let Some(previous) = self.installations.iter().find(|i| {
            i.provider == self.selected_provider && i.title_id == id.0 && i.directory == directory
        }) {
            for d in &previous.depots {
                if !depots.contains(d) {
                    depots.push(d.clone());
                }
            }
        }
        self.installations.retain(|i| {
            i.provider != self.selected_provider || i.title_id != id.0 || i.directory != directory
        });
        self.installations.push(Installation {
            provider: self.selected_provider.clone(),
            title_id: id.0.clone(),
            branch: plan.branch.clone(),
            depots,
            directory: directory.clone(),
        });
        self.persist()?;
        let destination = directory.to_string_lossy().into_owned();
        self.completed
            .send(CompletedInstallation {
                name: plan.title.name.clone(),
                directory,
            })
            .map_err(|_| provider::StoreError::SessionExpired)?;
        self.shared.update(|v| {
            v.transfer.state = StoreTransferState::Complete;
            v.transfer.progress = Some(1.0);
            v.transfer.status = tr!("下载完成，正在识别核心").into();
            v.transfer.notice = result
                .notice
                .map_or_else(String::new, |notice| notice.to_string());
            v.transfer.destination = destination;
            if result.notice.is_some() {
                v.transfer.status.clear();
            }
            if let Some(game) = v.games.iter_mut().find(|g| g.title_id == id.0) {
                game.installed = true;
                game.update_available = false;
            }
        });
        Ok(())
    }
}
fn apply_login(view: &mut StoreViewModel, update: provider::LoginUpdate) {
    view.error.clear();
    view.login_state = match update {
        provider::LoginUpdate::Connecting => StoreLoginState::Connecting,
        provider::LoginUpdate::EmailCode => StoreLoginState::EmailCode,
        provider::LoginUpdate::MobileCode => StoreLoginState::MobileCode,
        provider::LoginUpdate::MobileConfirmation => StoreLoginState::MobileConfirmation,
        provider::LoginUpdate::QrCode(url) => {
            view.qr_url = url;
            StoreLoginState::QrCode
        }
        provider::LoginUpdate::InvalidCode => {
            view.error = provider::StoreError::InvalidGuardCode.to_string();
            view.login_state
        }
        provider::LoginUpdate::SignedIn(account) => {
            view.account_name = account;
            view.qr_url.clear();
            StoreLoginState::SignedIn
        }
    };
}
fn game_view(
    game: &provider::OwnedTitle,
    provider: &str,
    installed: &[Installation],
) -> StoreGameViewModel {
    StoreGameViewModel {
        title_id: game.id.0.clone(),
        name: game.name.clone(),
        platforms: game.platforms.clone(),
        languages: game.languages.clone(),
        size_bytes: game.size_bytes,
        installed: installed
            .iter()
            .any(|i| i.provider == provider && i.title_id == game.id.0),
        update_available: false,
        update_unknown: false,
        unavailable_reason: game.unavailable_reason.clone().unwrap_or_default(),
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn store_action(&mut self, action: StoreAction) -> Result<ManagerViewModel, String> {
        if !self.network_allowed
            && !matches!(
                action,
                StoreAction::Logout
                    | StoreAction::Cancel
                    | StoreAction::OpenDestination
                    | StoreAction::Search { .. }
                    | StoreAction::SetLibraryRoot { .. }
                    | StoreAction::SelectDepot { .. }
                    | StoreAction::SelectBranch { .. }
            )
        {
            return Err(tr!("请先在设置中允许网络访问").into());
        }
        self.store.dispatch(action)?;
        self.model()
    }
    /// The sign-in rule the host's webview follows for the selected provider.
    pub(super) fn store_web_login(&self) -> Result<&'static provider::WebLogin, String> {
        if !self.network_allowed {
            return Err(tr!("请先在设置中允许网络访问").into());
        }
        self.store.web_login()
    }
    /// Hand the cookies the webview captured to the selected provider.
    pub(super) fn store_web_signed_in(
        &mut self,
        cookies: Vec<provider::WebCookie>,
    ) -> Result<ManagerViewModel, String> {
        if !self.network_allowed {
            return Err(tr!("请先在设置中允许网络访问").into());
        }
        self.store.web_signed_in(cookies)?;
        self.model()
    }
    pub(super) fn poll_store(&mut self) -> Result<bool, String> {
        let mut changed = self.store.changed();
        while let Some(completed) = self.store.completed() {
            let path = std::fs::canonicalize(completed.directory).map_err(|e| e.to_string())?;
            let id = path_id(&path);
            if self.library.game(&id).map_err(|e| e.to_string())?.is_none() {
                self.library
                    .add_game(&GameRecord {
                        game_id: id.clone(),
                        title: completed.name,
                        user_title: None,
                        location: path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?.into(),
                        family_id: None,
                        content_fingerprint: None,
                        added_at_unix_ms: unix_time_ms()?,
                    })
                    .map_err(|e| e.to_string())?;
            }
            self.selected_case_id = Some(id);
            self.store.imports.push_back(path);
            changed = true;
        }
        if self.active.is_none()
            && !self.startup_busy()
            && self.patch_job.is_none()
            && let Some(path) = self.store.imports.front().cloned()
        {
            self.add_game_directory(&path)?;
            self.store.imports.pop_front();
            changed = true;
        }
        Ok(changed)
    }
}

#[cfg(all(test, feature = "store-dlsite", feature = "store-steam"))]
mod web_login_tests {
    use super::*;

    fn select(runtime: &StoreRuntime, id: &str) {
        let registry = registry();
        let view = provider_view(&registry, id, Path::new("store"));
        runtime.shared.update(|v| *v = view);
    }

    #[test]
    fn webview_rule_follows_selected_provider_and_waits_for_idle_store() {
        let root = tempfile::tempdir().unwrap();
        let runtime = StoreRuntime::new(root.path());
        select(&runtime, "dlsite");
        assert!(runtime.view().can_web);
        let rule = runtime.web_login().unwrap();
        assert!(rule.start_url.starts_with("https://") && rule.start_url.contains("dlsite.com"));
        runtime.shared.busy.store(true, Ordering::Release);
        assert!(runtime.web_login().is_err());
        runtime.shared.busy.store(false, Ordering::Release);
        select(&runtime, "steam");
        assert!(!runtime.view().can_web);
        assert!(runtime.web_login().is_err());
    }

    #[test]
    fn captured_cookies_without_a_session_end_in_a_visible_error() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = StoreRuntime::new(root.path());
        let (wake, wakeups) = mpsc::channel();
        runtime.set_wake(Arc::new(move || {
            let _ = wake.send(());
        }));
        runtime.pending.as_mut().unwrap().selected_provider = "dlsite".into();
        select(&runtime, "dlsite");
        runtime.web_signed_in(Vec::new()).unwrap();
        assert!(runtime.web_signed_in(Vec::new()).is_err(), "a second sign-in waits for the first");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            wakeups.recv_timeout(remaining).expect("worker reports the result");
            let view = runtime.view();
            if view.login_state == StoreLoginState::Error && !runtime.shared.busy.load(Ordering::Acquire) {
                assert_eq!(view.error, provider::StoreError::WebLoginIncomplete.to_string());
                break;
            }
        }
        assert!(!root.path().join(".shinki/store/dlsite/login-present").exists());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_login_does_not_leave_download_controls_busy() {
        let root = tempfile::tempdir().unwrap();
        let runtime = StoreRuntime::new(root.path());
        runtime.shared.busy.store(true, Ordering::Release);
        runtime.cancel();
        assert!(runtime.cancel.lock().unwrap().is_cancelled());
        assert_eq!(runtime.view().transfer.state, StoreTransferState::Idle);
    }

    #[test]
    fn old_steam_installs_migrate_once_without_moving_files() {
        let root = tempfile::tempdir().unwrap();
        let installed = root.path().join("existing-game");
        std::fs::create_dir(&installed).unwrap();
        std::fs::write(installed.join("game.dat"), b"owned game content").unwrap();
        let old = root.path().join("steam-installations.json");
        std::fs::write(
            &old,
            serde_json::to_vec(&serde_json::json!([{
                "app_id": 480, "branch": "public", "depots": [481], "directory": installed,
            }]))
            .unwrap(),
        )
        .unwrap();
        let mut runtime = StoreRuntime::new(root.path());
        let worker = runtime.pending.as_mut().unwrap();
        worker.load_installations().unwrap();
        assert!(!old.exists());
        assert_eq!(worker.installations[0].title_id, "480");
        assert_eq!(worker.installations[0].directory, installed);
        assert_eq!(
            std::fs::read(installed.join("game.dat")).unwrap(),
            b"owned game content"
        );
        worker.installations.clear();
        worker.load_installations().unwrap();
        assert_eq!(worker.installations.len(), 1);
        let title = provider::OwnedTitle {
            id: provider::TitleId("480".into()),
            ..Default::default()
        };
        assert!(game_view(&title, "steam", &worker.installations).installed);
        assert!(!game_view(&title, "dlsite", &worker.installations).installed);
    }

    #[test]
    fn guard_retry_preserves_challenge_and_success_clears_qr() {
        let mut view = StoreViewModel::default();
        apply_login(&mut view, provider::LoginUpdate::Connecting);
        assert_eq!(view.login_state, StoreLoginState::Connecting);
        apply_login(&mut view, provider::LoginUpdate::EmailCode);
        apply_login(&mut view, provider::LoginUpdate::InvalidCode);
        assert_eq!(view.login_state, StoreLoginState::EmailCode);
        assert!(!view.error.is_empty());
        apply_login(
            &mut view,
            provider::LoginUpdate::QrCode("https://s.team/q/test".into()),
        );
        assert!(!view.qr_url.is_empty());
        apply_login(&mut view, provider::LoginUpdate::SignedIn("account".into()));
        assert_eq!(view.login_state, StoreLoginState::SignedIn);
        assert!(view.qr_url.is_empty());
        assert!(view.error.is_empty());
    }

    #[test]
    fn completed_download_is_added_even_without_a_matching_core() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("downloaded");
        std::fs::create_dir(&directory).unwrap();
        let mut app = ShinkiEmuManagerController::open_library(
            root.path().join("manager"),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        app.store
            .pending
            .as_ref()
            .unwrap()
            .completed
            .send(CompletedInstallation {
                name: "Downloaded Store game".into(),
                directory: directory.clone(),
            })
            .unwrap();
        assert!(app.poll_store().unwrap());
        super::super::probe_state_tests::settle_background(&mut app);
        let id = path_id(&std::fs::canonicalize(directory).unwrap());
        let game = app.library.game(&id).unwrap().unwrap();
        assert_eq!(game.title, "Downloaded Store game");
        assert!(game.family_id.is_none());
        assert!(app.store.imports.is_empty());
    }

    #[test]
    fn search_filters_owned_games_without_starting_network_work() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = StoreRuntime::new(root.path());
        runtime.shared.update(|v| {
            v.games = vec![
                StoreGameViewModel {
                    title_id: "480".into(),
                    name: "Spacewar".into(),
                    ..Default::default()
                },
                StoreGameViewModel {
                    title_id: "20".into(),
                    name: "Other".into(),
                    ..Default::default()
                },
            ]
        });
        runtime
            .dispatch(StoreAction::Search {
                query: "SPACE".into(),
            })
            .unwrap();
        assert_eq!(runtime.view().games.len(), 1);
        assert_eq!(runtime.view().games[0].title_id, "480");
        assert!(runtime.pending.is_some());
        runtime
            .dispatch(StoreAction::Search { query: "20".into() })
            .unwrap();
        assert_eq!(runtime.view().games[0].title_id, "20");
    }

    #[test]
    #[ignore = "contacts the metadata service and requests a real Store QR challenge"]
    fn live_startup_network_clients_and_qr_challenge_wake_host() {
        super::super::initialize_network_crypto();
        let http_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let metadata = shinki_emu_metadata::MetadataSyncClient::new(
            shinki_emu_metadata::DEFAULT_METADATA_BASE_URL,
            std::time::Duration::from_secs(20),
        )
        .unwrap();
        let synchronized = http_runtime
            .block_on(metadata.sync(None, &std::collections::BTreeMap::new()))
            .expect("Manager metadata HTTPS must work after startup crypto initialization");
        match synchronized {
            shinki_emu_metadata::SyncOutcome::Updated { index, files, .. } => {
                assert!(!index.files.is_empty());
                assert_eq!(files.len(), index.files.len());
            }
            shinki_emu_metadata::SyncOutcome::Unchanged => panic!("no cached ETag was supplied"),
        }
        #[cfg(not(target_os = "android"))]
        let _bangumi =
            shinki_emu_metadata::BangumiProvider::new(shinki_emu_metadata::BangumiProviderConfig {
                access_token: None,
                timeout: std::time::Duration::from_secs(20),
            })
            .expect("Manager's reqwest 0.12 client must initialize alongside reqwest 0.13");
        let _translation = shinki_emu_translation::TranslationClient::new(
            shinki_emu_translation::TranslationProfile {
                model: "startup-tls-check".into(),
                ..Default::default()
            },
            Arc::new(crate::platform_secret::ManagerSecretStore::open().unwrap()),
        )
        .expect("Manager translation HTTPS client must initialize without reading a credential");
        let root = tempfile::tempdir().unwrap();
        let mut runtime = StoreRuntime::new(root.path());
        let (wake, wakeups) = mpsc::channel();
        runtime.set_wake(Arc::new(move || {
            let _ = wake.send(());
        }));
        let started = std::time::Instant::now();
        runtime.dispatch(StoreAction::QrLogin).unwrap();
        let deadline = started + std::time::Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            wakeups
                .recv_timeout(remaining)
                .expect("Manager worker must wake the host with a QR challenge within 20 seconds");
            let view = runtime.view();
            assert_ne!(view.login_state, StoreLoginState::Error, "{}", view.error);
            if view.login_state == StoreLoginState::QrCode {
                assert!(view.qr_url.starts_with("https://s.team/q/"));
                runtime.cancel();
                break;
            }
        }
    }
}
