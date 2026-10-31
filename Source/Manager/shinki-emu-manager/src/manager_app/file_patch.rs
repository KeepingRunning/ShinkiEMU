use super::*;
use shinki_emu_manager_ui_slint::{PatchStateViewModel, RecipeViewModel};
use shinki_emu_patch::{
    OutputProfile, PatchError, PatchErrorKind, PatchMode, PatchProgress, PatchRequest, PatchResult,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Receiver, TryRecvError},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use shinki_emu_manager_ui_slint::tr;
use shinki_emu_manager_ui_slint::i18n::pick;

pub(super) fn require_profile_capabilities(
    profile: &OutputProfile,
    descriptor: &shinki_emu_manager_core::FamilyPluginDescriptor,
) -> Result<(), String> {
    if profile.required_capabilities == 0 {
        return Ok(());
    }
    let mut supported = 0;
    if descriptor.has_capability(shinki_emu_manager_core::FamilyCapability::NativeScriptIdentity) {
        supported |= shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY;
    }
    if descriptor.has_capability(shinki_emu_manager_core::FamilyCapability::ConfiguredFontPriority) {
        supported |= shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY;
    }
    if descriptor.plugin_id != profile.plugin_id
        || profile.required_capabilities & !supported != 0
    {
        return Err(format!(
            "SHINKI_EMU_CORE_UPDATE_REQUIRED: {}",
            tr!("此转换结果需要更新所选核心")
        ));
    }
    Ok(())
}

enum PatchOperation {
    Apply(PatchRequest),
    Restore(PathBuf),
}

enum PatchCompletion {
    Applied(PatchResult),
    Restored,
}

pub(super) struct PatchJob {
    pub id: u64,
    pub source_game_id: String,
    pub cancelled: Arc<AtomicBool>,
    pub progress: Arc<Mutex<Option<PatchProgress>>>,
    completion: Receiver<Result<PatchCompletion, PatchError>>,
    pub thread: Option<JoinHandle<()>>,
    pub restoring: bool,
    pub in_place: bool,
}

impl PatchJob {
    fn start(
        id: u64,
        source_game_id: String,
        operation: PatchOperation,
        wake: HostWake,
    ) -> Result<Self, String> {
        let restoring = matches!(&operation, PatchOperation::Restore(_));
        let in_place = restoring || matches!(&operation, PatchOperation::Apply(PatchRequest { mode: PatchMode::InPlace, .. }));
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(None));
        let (sender, completion) = sync_channel(1);
        let signal = cancelled.clone();
        let slot = progress.clone();
        let progress_wake = wake.clone();
        let thread = std::thread::Builder::new()
            .name("shinki-file-patch".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let progress = move |value| {
                        let first = slot.lock().unwrap_or_else(|poison| poison.into_inner())
                            .replace(value).is_none();
                        if first { progress_wake(); }
                    };
                    match operation {
                        PatchOperation::Apply(request) => shinki_emu_patch::apply(request, signal, progress)
                            .map(PatchCompletion::Applied),
                        PatchOperation::Restore(game) => shinki_emu_patch::restore_latest(&game, signal, progress)
                            .map(|()| PatchCompletion::Restored),
                    }
                }))
                .unwrap_or_else(|_| {
                    Err(PatchError::new(
                        PatchErrorKind::Script,
                        "conversion worker panicked",
                    ))
                });
                let _ = sender.send(result);
                wake();
            })
            .map_err(|cause| {
                format!("SHINKI_EMU_PATCH_IO: cannot start conversion worker: {cause}")
            })?;
        Ok(Self {
            restoring,
            in_place,
            id,
            source_game_id,
            cancelled,
            progress,
            completion,
            thread: Some(thread),
        })
    }

    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(super) fn join(&mut self) -> Result<(), String> {
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            Err("SHINKI_EMU_PATCH_SCRIPT: conversion worker could not be joined".into())
        } else {
            Ok(())
        }
    }
}

impl Drop for PatchJob {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
            let _ = self.join();
        }
    }
}

pub(super) struct PendingImport {
    pub output: PathBuf,
    pub source_game_id: String,
    pub profile: OutputProfile,
    pub in_place: bool,
}

fn patch_busy_error() -> String {
    format!(
        "SHINKI_EMU_PATCH_INVALID_REQUEST: {}",
        tr!("已有文件补丁任务或导入正在进行")
    )
}

impl ShinkiEmuManagerController {
    pub(super) fn patch_busy(&self) -> bool {
        self.patch_job.is_some() || self.patch_import.is_some() || self.patch_download.is_some()
    }

    /// `step`: 0 not started, 4 running or failed, 5 imported.
    pub(super) fn patch_state_view(&self) -> PatchStateViewModel {
        let mut view = self.patch_state.clone();
        view.running = self.patch_job.is_some() || self.patch_download.is_some();
        view.downloading = self.patch_download.is_some();
        // A restore owns exclusive Windows backup handles; do not inspect its
        // changing namespace while the worker is active.
        view.has_backup = self.patch_job.is_none() && self.patch_game_id.as_deref()
            .and_then(|id| self.game(id).ok())
            .is_some_and(|game| shinki_emu_patch::has_backup(Path::new(&game.location)).unwrap_or(false));
        if view.running || !view.error.is_empty() {
            view.step = 4;
        }
        view.can_start = !self.patch_busy()
            && self.active.is_none()
            && !self.startup_busy()
            && self.cores_ready()
            && self.patch_game_id.is_some();
        if view.phase.is_empty() && !view.can_start {
            view.phase = if self.session_running() {
                tr!("结束游戏后才能应用文件补丁").into()
            } else if self.active.is_some() {
                tr!("游戏正在关闭，请稍候").into()
            } else if self.startup_busy() {
                tr!("请等待资料库准备完成").into()
            } else if !self.cores_ready() {
                tr!("请先准备资料库").into()
            } else {
                tr!("请先选择游戏").into()
            };
        }
        view
    }

    /// Catalog recipes for a game linked to VNDB work `vn_id`: the work is listed
    /// and the game's chosen emulator (else any probe candidate) consumes the
    /// output. The preset's patch for the game's release and emulator is marked.
    pub(super) fn patch_recipes(
        &self,
        game_id: &str,
        vn_id: &str,
    ) -> Result<Vec<RecipeViewModel>, String> {
        let Some(catalog) = &self.metadata_catalog else {
            return Ok(Vec::new());
        };
        let emulators: Vec<&str> = match self.candidates.get(game_id) {
            Some(candidate) => vec![candidate.report.plugin_id.as_str()],
            None => self
                .probe_choices
                .get(game_id)
                .into_iter()
                .flatten()
                .map(|choice| choice.report.plugin_id.as_str())
                .collect(),
        };
        let recommended = self
            .recommended_preset(game_id)?
            .and_then(|preset| preset.patch.as_deref());
        Ok(catalog
            .recipes
            .iter()
            .filter(|recipe| {
                recipe.vn_ids.iter().any(|id| id == vn_id)
                    && recipe
                        .emulators
                        .iter()
                        .any(|id| emulators.contains(&id.as_str()))
            })
            .map(|recipe| RecipeViewModel {
                id: recipe.id.clone(),
                title: recipe.title.clone(),
                recommended: recommended == Some(recipe.id.as_str()),
            })
            .collect())
    }

    fn patch_emulators(&self, game_id: &str) -> Vec<&str> {
        match self.candidates.get(game_id) {
            Some(chosen) => vec![chosen.report.plugin_id.as_str()],
            None => self.probe_choices.get(game_id).into_iter().flatten()
                .map(|choice| choice.report.plugin_id.as_str()).collect(),
        }
    }

    /// Applicable recipes and the hint shown with them; a game without a VNDB
    /// link has no catalog recipes.
    pub(super) fn patch_choices(
        &self,
        game_id: &str,
        identities: &[shinki_emu_manager_core::ExternalIdentityRecord],
    ) -> Result<(Vec<RecipeViewModel>, String), String> {
        let Some(vn_id) = Self::vndb_id(identities) else {
            return Ok((Vec::new(), tr!("先在 游戏设置→作品资料 关联 VNDB 作品").into()));
        };
        let recipes = self.patch_recipes(game_id, vn_id)?;
        let mut hint = if recipes.is_empty() { tr!("补丁目录中没有适用于这部作品和当前模拟器的补丁，可以选择本地脚本。").to_owned() } else { String::new() };
        if let Some(catalog) = &self.metadata_catalog {
            let emulators = self.patch_emulators(game_id);
            let release = self.pinned_releases.get(game_id).map(|r| r.release_id.as_str());
            for patch in catalog.patches.entries.iter().filter(|p| {
                p.kind != shinki_emu_metadata::PatchKind::Recipe
                    && p.vn_ids.iter().any(|id| id == vn_id)
                    && emulators.iter().any(|id| p.emulators.iter().any(|e| e == id))
                    && (p.release_ids.is_empty() || release.is_some_and(|r| p.release_ids.iter().any(|id| id == r)))
            }) {
                use std::fmt::Write;
                let _ = write!(hint, "\n{}\n{}\n{}", pick(&patch.title), pick(&patch.summary), tr!("许可：{license}", license = patch.license));
                if patch.release_ids.is_empty() { hint.push('\n'); hint.push_str(tr!("发行版本未限定（未验证所有版本）")); }
                if let Some(url) = &patch.source_url { let _ = write!(hint, "\n{}", tr!("来源：{url}", url = url)); }
                if let Some(url) = &patch.authorization_url { let _ = write!(hint, "\n{}", tr!("授权：{url}", url = url)); }
                if let Some(a) = &patch.artifact { let _ = write!(hint, "\n{}", tr!("下载：{url}\nSHA256：{sha256}\n大小：{size} 字节", url = a.url, sha256 = a.sha256, size = a.size)); }
            }
        }
        Ok((recipes, hint))
    }

    pub(super) fn matching_artifacts(&self, game_id: &str,
        identities: &[shinki_emu_manager_core::ExternalIdentityRecord])
        -> Vec<shinki_emu_metadata::PatchCatalogEntry>
    {
        let Some(vn_id) = Self::vndb_id(identities) else { return Vec::new() };
        let Some(catalog) = &self.metadata_catalog else { return Vec::new() };
        let emulators = self.patch_emulators(game_id);
        let release = self.pinned_releases.get(game_id).map(|r| r.release_id.as_str());
        catalog.patches.entries.iter().filter(|patch| {
            patch.kind != shinki_emu_metadata::PatchKind::Recipe && patch.artifact.is_some()
                && patch.vn_ids.iter().any(|id| id == vn_id)
                && emulators.iter().any(|id| patch.emulators.iter().any(|e| e == id))
                && (patch.release_ids.is_empty() || release.is_some_and(|r| patch.release_ids.iter().any(|id| id == r)))
        }).cloned().collect()
    }


    pub(super) fn search_patch_games(&mut self, query: &str) -> Result<ManagerViewModel, String> {
        if query.chars().count() > 256 {
            return Err("SHINKI_EMU_SEARCH_QUERY_BOUNDS".into());
        }
        self.patch_query = query.into();
        self.model()
    }

    /// Choose the source game. Another game, or a finished or failed run, starts
    /// over with the game directory as patch input.
    pub(super) fn select_patch_game(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        if self.patch_busy() {
            return Err(patch_busy_error());
        }
        let game = self.game(game_id)?;
        let unchanged = self.patch_game_id.as_deref() == Some(game_id)
            && self.patch_state.step == 0
            && self.patch_state.error.is_empty();
        if !unchanged {
            self.patch_state = PatchStateViewModel {
                patch_directory: game.location,
                ..Default::default()
            };
            self.patch_game_id = Some(game.game_id);
        }
        self.model()
    }

    fn patch_game(&self) -> Result<GameRecord, String> {
        self.patch_game_id
            .as_deref()
            .ok_or_else(|| "SHINKI_EMU_PATCH_GAME_REQUIRED".to_owned())
            .and_then(|id| self.game(id))
    }

    /// Choose a catalog recipe that applies to the selected game.
    pub(super) fn select_patch_recipe(&mut self, id: &str) -> Result<ManagerViewModel, String> {
        let game = self.patch_game()?;
        let identities = self
            .library
            .external_identities(&game.game_id)
            .map_err(|error| error.to_string())?;
        let (recipes, _) = self.patch_choices(&game.game_id, &identities)?;
        if !recipes.iter().any(|recipe| recipe.id == id) {
            return Err("SHINKI_EMU_PATCH_RECIPE_UNKNOWN".into());
        }
        self.set_patch_selection(|state| state.script = id.into())
    }

    /// Choose a trusted local Luau script.
    pub(super) fn set_patch_script(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        let path = path
            .to_str()
            .ok_or("SHINKI_EMU_PATCH_PATH: script path must be Unicode")?
            .to_owned();
        self.set_patch_selection(|state| state.script = path)
    }

    pub(super) fn set_patch_mode(&mut self, in_place: bool) -> Result<ManagerViewModel, String> {
        self.set_patch_selection(|state| state.in_place = in_place)
    }

    pub(super) fn download_patch(&mut self, id: &str) -> Result<ManagerViewModel, String> {
        if self.patch_busy() { return Err(patch_busy_error()); }
        if !self.network_allowed { return Err("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: offline".into()); }
        let game = self.patch_game()?;
        let identities = self.library.external_identities(&game.game_id)
            .map_err(|error| error.to_string())?;
        let patch = self.matching_artifacts(&game.game_id, &identities).into_iter()
            .find(|item| item.id == id)
            .ok_or("SHINKI_EMU_PATCH_INVALID_REQUEST: patch artifact not available for game")?;
        let wake = self.host_wake.clone().ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        self.patch_download = Some(patch_download::PatchDownloadJob::start(
            PathBuf::from(&game.location), patch.artifact.expect("filtered artifact"), patch.kind, wake)?);
        self.patch_state.phase = tr!("正在下载并验证补丁文件…").into();
        self.patch_state.error.clear();
        self.model()
    }

    pub(super) fn restore_patch(&mut self) -> Result<ManagerViewModel, String> {
        if self.patch_busy() { return Err(patch_busy_error()); }
        self.ensure_owner_idle()?;
        let game = self.patch_game()?;
        let wake = self.host_wake.clone().ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        let id = self.patch_sequence.checked_add(1).ok_or("SHINKI_EMU_PATCH_BOUNDS")?;
        let game_path = PathBuf::from(&game.location);
        self.patch_job = Some(PatchJob::start(id, game.game_id,
            PatchOperation::Restore(game_path.clone()), wake)?);
        self.patch_sequence = id;
        self.patch_state.in_place = true;
        self.patch_state.output = game_path.to_string_lossy().into_owned();
        self.patch_state.phase = tr!("正在还原最近一次补丁…").into();
        self.patch_state.error.clear();
        self.model()
    }

    /// `kind` is `patch` (input directory, initially the game directory) or `output`.
    pub(super) fn set_patch_directory(
        &mut self,
        kind: &str,
        path: &Path,
    ) -> Result<ManagerViewModel, String> {
        let path = path
            .to_str()
            .ok_or("SHINKI_EMU_PATCH_PATH: directory path must be Unicode")?
            .to_owned();
        match kind {
            "patch" => self.set_patch_selection(|state| state.patch_directory = path),
            "output" => self.set_patch_selection(|state| state.output_directory = path),
            _ => Err("SHINKI_EMU_PATCH_INVALID_REQUEST: unknown directory kind".into()),
        }
    }

    fn set_patch_selection(
        &mut self,
        change: impl FnOnce(&mut PatchStateViewModel),
    ) -> Result<ManagerViewModel, String> {
        if self.patch_busy() {
            return Err(patch_busy_error());
        }
        self.patch_game()?;
        change(&mut self.patch_state);
        self.patch_state.step = 0;
        self.patch_state.phase.clear();
        self.patch_state.error.clear();
        self.model()
    }

    fn recipe_script(&self, id: &str) -> Result<PathBuf, String> {
        let catalog = self
            .metadata_catalog
            .as_ref()
            .ok_or("SHINKI_EMU_PATCH_RECIPE_UNKNOWN")?;
        catalog
            .recipes
            .iter()
            .find(|recipe| recipe.id == id)
            .map(|recipe| catalog.patch_directory().join(&recipe.script))
            .ok_or_else(|| "SHINKI_EMU_PATCH_RECIPE_UNKNOWN".into())
    }

    /// Start the selected recipe or local script on the selected game.
    pub(super) fn start_patch(&mut self) -> Result<ManagerViewModel, String> {
        let state = &self.patch_state;
        let script = if state.script.is_empty() {
            PathBuf::new()
        } else {
            self.recipe_script(&state.script)
                .unwrap_or_else(|_| PathBuf::from(&state.script))
        };
        let patch = PathBuf::from(&state.patch_directory);
        let output = PathBuf::from(&state.output_directory);
        self.start_file_patch(&script, &patch, &output)
    }

    /// Select the game imported from the finished conversion.
    pub(super) fn open_patch_output(&mut self) -> Result<ManagerViewModel, String> {
        let output = fs::canonicalize(&self.patch_state.output)
            .map_err(|_| "SHINKI_EMU_PATCH_PATH: completed output is no longer available")?;
        let location = output.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?;
        let game = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|game| game.location == location)
            .ok_or("SHINKI_EMU_GAME_NOT_FOUND")?;
        self.select_case(&game.game_id)
    }

    pub(super) fn start_file_patch(
        &mut self,
        script: &Path,
        patch: &Path,
        output: &Path,
    ) -> Result<ManagerViewModel, String> {
        if self.patch_busy() {
            return Err(patch_busy_error());
        }
        self.ensure_owner_idle()?;
        if self.startup_busy() {
            return Err("SHINKI_EMU_FAMILY_INITIALIZING".into());
        }
        if !self.cores_ready() {
            return Err("SHINKI_EMU_CORES_NOT_READY".into());
        }
        if script.as_os_str().is_empty() || (!self.patch_state.in_place && output.as_os_str().is_empty()) {
            return Err(format!("SHINKI_EMU_PATCH_INVALID_REQUEST: {}", tr!("请选择脚本和新的输出目录")));
        }
        let game = self.patch_game()?;
        let source = PathBuf::from(&game.location);
        let request = PatchRequest {
            script: script.to_owned(),
            patch: if patch.as_os_str().is_empty() {
                source.clone()
            } else {
                patch.to_owned()
            },
            source,
            mode: if self.patch_state.in_place { PatchMode::InPlace } else {
                PatchMode::NewDirectory { output: output.to_owned() }
            },
        };
        let id = self
            .patch_sequence
            .checked_add(1)
            .ok_or("SHINKI_EMU_PATCH_BOUNDS: conversion identity exhausted")?;
        let wake = self
            .host_wake
            .clone()
            .ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        let job = PatchJob::start(id, game.game_id, PatchOperation::Apply(request), wake)?;
        self.patch_sequence = id;
        self.patch_job = Some(job);
        self.patch_state = PatchStateViewModel {
            phase: tr!("准备文件补丁任务…").into(),
            output: if self.patch_state.in_place { game.location } else { output.to_string_lossy().into_owned() },
            script: std::mem::take(&mut self.patch_state.script),
            patch_directory: std::mem::take(&mut self.patch_state.patch_directory),
            output_directory: std::mem::take(&mut self.patch_state.output_directory),
            in_place: self.patch_state.in_place,
            ..Default::default()
        };
        self.model()
    }

    pub(super) fn cancel_file_patch(&mut self) -> Result<ManagerViewModel, String> {
        if let Some(job) = &self.patch_job {
            job.cancel();
            self.patch_state.phase = tr!("正在取消，等待文件句柄和工作线程回收…").into();
        }
        if let Some(job) = &self.patch_download {
            job.cancel();
            self.patch_state.phase = tr!("正在取消补丁下载…").into();
        }
        self.model()
    }

    pub(super) fn poll_file_patch(&mut self) -> Result<bool, String> {
        if let Some(download) = &self.patch_download {
            let result = match download.result() {
                Ok(Some(result)) => result,
                Ok(None) => return Ok(false),
                Err(error) => Err(error),
            };
            let mut download = self.patch_download.take().expect("completed download exists");
            match download.join().and(result) {
                Ok(directory) => {
                    self.patch_state.patch_directory = directory.to_string_lossy().into_owned();
                    self.patch_state.phase = tr!("补丁已下载并验证，已设置补丁输入目录").into();
                    self.patch_state.error.clear();
                }
                Err(error) => {
                    self.patch_state.phase = tr!("补丁下载失败").into();
                    self.patch_state.error = error;
                }
            }
            return Ok(true);
        }
        let Some(job) = self.patch_job.as_ref() else {
            return Ok(false);
        };
        let mut changed = false;
        if let Some(progress) = job
            .progress
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            self.patch_state.phase = if job.cancelled.load(Ordering::Acquire) {
                tr!("正在取消…").into()
            } else {
                progress.phase
            };
            self.patch_state.completed = progress.completed;
            self.patch_state.total = progress.total;
            changed = true;
        }
        let result = match job.completion.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(changed),
            Err(TryRecvError::Disconnected) => Err(PatchError::new(
                PatchErrorKind::Script,
                "conversion worker disconnected",
            )),
        };
        let mut job = self
            .patch_job
            .take()
            .expect("completed job retained until join");
        let joined = job.join();
        if job.id != self.patch_sequence {
            return Err("SHINKI_EMU_PATCH_INTEGRITY: stale conversion completion".into());
        }
        match result {
            Ok(PatchCompletion::Applied(result)) => {
                self.patch_state.output = result.output.to_string_lossy().into_owned();
                self.patch_state.completed = 1;
                self.patch_state.total = Some(1);
                if let Err(error) = joined {
                    self.patch_import_failed(error);
                    return Ok(true);
                }
                self.patch_state.phase = if job.in_place {
                    tr!("原位覆盖完成，正在重新探测游戏…")
                } else { tr!("文件转换完成，正在导入独立目录…") }.into();
                self.patch_import = Some(PendingImport {
                    output: result.output.clone(),
                    source_game_id: std::mem::take(&mut job.source_game_id),
                    profile: result.profile,
                    in_place: job.in_place,
                });
                if let Err(error) = self.begin_scan(probe_worker::ScanKind::Import, vec![result.output]) {
                    self.patch_import.take();
                    self.patch_import_failed(error);
                }
            }
            Ok(PatchCompletion::Restored) => {
                if let Err(error) = joined {
                    self.patch_import_failed(error);
                    return Ok(true);
                }
                self.patch_state.phase = tr!("原文件已还原，正在重新探测游戏…").into();
                self.patch_state.error.clear();
                self.patch_state.step = 5;
                let game = self.game(&job.source_game_id)?;
                if let Err(error) = self.begin_scan(probe_worker::ScanKind::Import, vec![PathBuf::from(game.location)]) {
                    self.patch_import_failed(error);
                }
            }
            Err(error) => {
                if error.kind() == PatchErrorKind::Cancelled && joined.is_ok() {
                    self.patch_state.phase = tr!("已取消，游戏文件保持不变").into();
                    self.patch_state.error.clear();
                } else {
                    self.patch_state.phase = if job.restoring { tr!("还原失败") } else { tr!("文件补丁处理失败") }.into();
                    self.patch_state.error = joined.err().unwrap_or_else(|| error.to_string());
                }
            }
        }
        Ok(true)
    }

    pub(super) fn patch_import_failed(&mut self, error: String) {
        self.patch_state.phase = tr!("文件转换完成，导入失败").into();
        self.patch_state.error = tr!("文件转换完成，导入失败：{error}", error = error);
    }

    pub(super) fn patch_import_succeeded(&mut self) {
        self.patch_state.phase = if self.patch_state.in_place {
            tr!("原位覆盖完成，游戏已重新探测")
        } else { tr!("文件转换完成，已加入独立库条目") }.into();
        self.patch_state.error.clear();
        self.patch_state.step = 5;
    }

    pub(super) fn select_import_profile(
        &self,
        choices: &[FamilyProbeCandidate],
        profile: &OutputProfile,
    ) -> Result<usize, String> {
        let index = choices.iter().position(|candidate| candidate.report.plugin_id == profile.plugin_id)
            .ok_or("SHINKI_EMU_EMULATOR_SELECTION_INVALID: patch output is not recognized by its requested emulator")?;
        let selected = &choices[index];
        if selected.descriptor.family_id != selected.report.family_id
            || self.registry.descriptor(&profile.plugin_id) != Some(&selected.descriptor)
        {
            return Err(
                "SHINKI_EMU_EMULATOR_SELECTION_INVALID: patch output descriptor mismatch".into(),
            );
        }
        require_profile_capabilities(profile, &selected.descriptor)?;
        Ok(index)
    }

    pub(super) fn apply_import_profile(
        &mut self,
        game_id: &str,
        choices: Vec<FamilyProbeCandidate>,
        selected: usize,
        profile: &OutputProfile,
    ) -> Result<(), String> {
        let selected = choices[selected].clone();
        let schema = self
            .registry
            .configuration(&profile.plugin_id)
            .map_err(|error| error.to_string())?;
        let values = family_config::parse_config_entries(
            &schema,
            profile
                .configuration
                .iter()
                .map(|(id, value)| (id.as_str(), value.as_str())),
        )?;
        self.library
            .apply_patch_profile(
                game_id,
                &selected.report.family_id,
                &profile.plugin_id,
                &schema,
                &values,
            )
            .map_err(|error| error.to_string())?;
        // No candidate cache mutation precedes the complete SQLite transaction.
        self.probe_choices.insert(game_id.to_owned(), choices);
        self.candidates.insert(game_id.to_owned(), selected);
        Ok(())
    }

    /// A patch output is another installed copy of the source's work: it
    /// takes the source's VNDB identity, cached metadata and cover, but not
    /// its pinned release, settings or play records.
    pub(super) fn inherit_source_work(&mut self, source_game_id: &str, game_id: &str) -> Result<(), String> {
        let inherited = self
            .library
            .inherit_external_identity(
                source_game_id,
                game_id,
                MetadataProviderId::Vndb.as_str(),
                unix_time_ms()?,
            )
            .map_err(|error| error.to_string())?;
        if inherited {
            copy_cover(&self.data_dir, source_game_id, game_id)?;
        }
        Ok(())
    }
}

/// Copies the source's stored cover, whatever its extension, to `game_id`.
fn copy_cover(data_dir: &Path, source_game_id: &str, game_id: &str) -> Result<(), String> {
    const EXTENSIONS: [&str; 5] = ["png", "jpg", "webp", "gif", "bmp"];
    let directory = data_dir.join("covers");
    let Some(extension) = EXTENSIONS
        .into_iter()
        .find(|extension| directory.join(format!("{source_game_id}.{extension}")).is_file())
    else {
        return Ok(());
    };
    for previous in EXTENSIONS {
        match fs::remove_file(directory.join(format!("{game_id}.{previous}"))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("SHINKI_EMU_METADATA_COVER_REMOVE".into()),
        }
    }
    fs::copy(
        directory.join(format!("{source_game_id}.{extension}")),
        directory.join(format!("{game_id}.{extension}")),
    )
    .map(drop)
    .map_err(|_| "SHINKI_EMU_METADATA_COVER_WRITE".into())
}

#[cfg(test)]
#[path = "file_patch_tests.rs"]
mod tests;
