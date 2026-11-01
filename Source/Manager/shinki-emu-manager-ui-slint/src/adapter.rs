use std::{cell::RefCell, rc::Rc};

use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel,
};

use crate::tr;

use crate::{
    App, Appearance, AppearanceViewModel, BangumiProgress, BangumiViewModel, Choice,
    ChoiceViewModel, CompatInfo, ConfigField, ConfigFieldViewModel, CoreInfo, CoreViewModel, Cores,
    EngineAction, EngineActionViewModel, Excluded, ExcludedViewModel, Filters, GameCard,
    GameCardViewModel, GameSettings, GamepadBinding, GamepadBindingViewModel, Library,
    ManagerViewModel, ManagerWindow, MatchReview, MatchViewModel, Network, Notices, Onboarding,
    Patch, PatchCheck, PatchCheckViewModel, PatchDownload, PatchDownloadViewModel, PatchGame,
    PatchGameViewModel, PatchState, PatchStateViewModel, PatchVerify, PlaySession,
    PlaySessionViewModel, Recipe, RecipeViewModel, Release, ReleaseViewModel, RenderState,
    RenderViewModel, ResaleNoticeState, SelectedGame, Selection, Session, Settings, Store,
    StoreAction, StoreDepot, StoreDepotViewModel, StoreGame, StoreGameViewModel, StoreLoadState,
    StoreLoginState, StoreTransfer, StoreTransferState, StoreTransferViewModel, TranslationForm,
    TranslationFormViewModel, covers, list_model::ListModel, notices,
};

pub struct SlintManagerAdapter {
    window: ManagerWindow,
    games: Rc<covers::CoverModel<GameCard>>,
    patch_games: Rc<covers::CoverModel<PatchGame>>,
    notices: Rc<notices::NoticeModel>,
    emulator_choices: ListModel<ChoiceViewModel, Choice>,
    releases: ListModel<ReleaseViewModel, Release>,
    other_versions: ListModel<ChoiceViewModel, Choice>,
    history: ListModel<PlaySessionViewModel, PlaySession>,
    matches: ListModel<MatchViewModel, MatchReview>,
    game_emulator_choices: ListModel<ChoiceViewModel, Choice>,
    render_options: ListModel<ChoiceViewModel, Choice>,
    game_fields: Rc<VecModel<ConfigField>>,
    game_bindings: ListModel<GamepadBindingViewModel, GamepadBinding>,
    recipes: ListModel<RecipeViewModel, Recipe>,
    patch_downloads: ListModel<PatchDownloadViewModel, PatchDownload>,
    patch_checks: ListModel<PatchCheckViewModel, PatchCheck>,
    cores: ListModel<CoreViewModel, CoreInfo>,
    default_fields: Rc<VecModel<ConfigField>>,
    default_render_options: ListModel<ChoiceViewModel, Choice>,
    models: ListModel<String, SharedString>,
    bindings: ListModel<GamepadBindingViewModel, GamepadBinding>,
    pad_state: ListModel<String, SharedString>,
    roots: ListModel<String, SharedString>,
    excluded: ListModel<ExcludedViewModel, Excluded>,
    filter_fields: Rc<VecModel<ConfigField>>,
    actions: ListModel<EngineActionViewModel, EngineAction>,
    onboarding_roots: ListModel<String, SharedString>,
    store_games: ListModel<StoreGameViewModel, StoreGame>,
    store_depots: ListModel<StoreDepotViewModel, StoreDepot>,
    store_language_depots: ListModel<StoreDepotViewModel, StoreDepot>,
    store_branches: ListModel<ChoiceViewModel, Choice>,
    cached_store_depots: RefCell<Vec<StoreDepotViewModel>>,
    cached_store_qr: RefCell<String>,
    cached_games: RefCell<Vec<GameCardViewModel>>,
    cached_patch_games: RefCell<Vec<PatchGameViewModel>>,
    cached_game_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_default_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_filter_fields: RefCell<Vec<ConfigFieldViewModel>>,
    cached_bangumi: RefCell<Option<(String, BangumiViewModel)>>,
    cached_translation: RefCell<Option<TranslationFormViewModel>>,
    cached_snackbar: RefCell<u32>,
    cached_launch: RefCell<String>,
}

impl SlintManagerAdapter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let window = ManagerWindow::new()?;
        let adapter = Self {
            games: Rc::new(covers::CoverModel::new(window.as_weak())?),
            patch_games: Rc::new(covers::CoverModel::new(window.as_weak())?),
            notices: Rc::new(notices::NoticeModel::new()),
            emulator_choices: ListModel::new(choice),
            releases: ListModel::new(release),
            other_versions: ListModel::new(choice),
            history: ListModel::new(play_session),
            matches: ListModel::new(match_review),
            game_emulator_choices: ListModel::new(choice),
            render_options: ListModel::new(choice),
            game_fields: Rc::new(VecModel::default()),
            game_bindings: ListModel::new(binding),
            recipes: ListModel::new(recipe),
            patch_downloads: ListModel::new(patch_download),
            patch_checks: ListModel::new(patch_check),
            cores: ListModel::new(core_info),
            default_fields: Rc::new(VecModel::default()),
            default_render_options: ListModel::new(choice),
            models: ListModel::strings(),
            bindings: ListModel::new(binding),
            pad_state: ListModel::strings(),
            roots: ListModel::strings(),
            excluded: ListModel::new(excluded),
            filter_fields: Rc::new(VecModel::default()),
            actions: ListModel::new(engine_action),
            onboarding_roots: ListModel::strings(),
            store_games: ListModel::new(store_game),
            store_depots: ListModel::new(store_depot),
            store_language_depots: ListModel::new(store_depot),
            store_branches: ListModel::new(choice),
            cached_store_depots: RefCell::default(),
            cached_store_qr: RefCell::default(),
            cached_games: RefCell::default(),
            cached_patch_games: RefCell::default(),
            cached_game_fields: RefCell::default(),
            cached_default_fields: RefCell::default(),
            cached_filter_fields: RefCell::default(),
            cached_bangumi: RefCell::default(),
            cached_translation: RefCell::default(),
            cached_snackbar: RefCell::new(0),
            cached_launch: RefCell::default(),
            window,
        };
        adapter.bind_models();
        adapter.install_internal_callbacks();
        Ok(adapter)
    }

    fn bind_models(&self) {
        let window = &self.window;
        window
            .global::<crate::MaterialWindowAdapter>()
            .set_touch(cfg!(any(target_os = "android", target_os = "ios")));
        window
            .global::<Library>()
            .set_games(ModelRc::from(self.games.clone()));
        window
            .global::<Notices>()
            .set_items(ModelRc::from(self.notices.clone()));

        let selection = window.global::<Selection>();
        selection.set_emulator_choices(self.emulator_choices.model());
        selection.set_releases(self.releases.model());
        selection.set_other_versions(self.other_versions.model());
        selection.set_history(self.history.model());
        selection.set_matches(self.matches.model());

        let game_settings = window.global::<GameSettings>();
        game_settings.set_emulator_choices(self.game_emulator_choices.model());
        game_settings.set_render_options(self.render_options.model());
        game_settings.set_fields(ModelRc::from(self.game_fields.clone()));
        game_settings.set_bindings(self.game_bindings.model());

        let patch = window.global::<Patch>();
        patch.set_games(ModelRc::from(self.patch_games.clone()));
        patch.set_recipes(self.recipes.model());
        patch.set_downloads(self.patch_downloads.model());
        patch.set_checks(self.patch_checks.model());

        let cores = window.global::<Cores>();
        cores.set_items(self.cores.model());
        cores.set_default_fields(ModelRc::from(self.default_fields.clone()));

        let settings = window.global::<Settings>();
        settings.set_models(self.models.model());
        settings.set_bindings(self.bindings.model());
        settings.set_pad_state(self.pad_state.model());
        settings.set_roots(self.roots.model());
        settings.set_excluded(self.excluded.model());

        window
            .global::<Filters>()
            .set_fields(ModelRc::from(self.filter_fields.clone()));
        window.global::<Session>().set_actions(self.actions.model());
        window
            .global::<Onboarding>()
            .set_roots(self.onboarding_roots.model());

        let store = window.global::<Store>();
        store.set_games(self.store_games.model());
        store.set_depots(self.store_depots.model());
        store.set_language_depots(self.store_language_depots.model());
        store.set_branches(self.store_branches.model());
    }

    fn install_internal_callbacks(&self) {
        let cover_window = self.window.as_weak();
        self.window
            .global::<Library>()
            .on_cover_demand(move |row, case_id, needed, view| {
                if let Some(window) = cover_window.upgrade() {
                    let games = window.global::<Library>().get_games();
                    if let Some(covers) = games
                        .as_any()
                        .downcast_ref::<covers::CoverModel<GameCard>>()
                    {
                        covers.demand(row, case_id.as_str(), needed, view.as_str(), &window);
                    }
                }
            });
        let patch_window = self.window.as_weak();
        self.window
            .global::<Patch>()
            .on_cover_demand(move |row, game_id, needed| {
                if let Some(window) = patch_window.upgrade() {
                    let games = window.global::<Patch>().get_games();
                    if let Some(covers) = games
                        .as_any()
                        .downcast_ref::<covers::CoverModel<PatchGame>>()
                    {
                        covers.demand(row, game_id.as_str(), needed, "patch", &window);
                    }
                }
            });

        let notice_window = self.window.as_weak();
        self.window.global::<Notices>().on_dismiss(move |id| {
            if let Some(window) = notice_window.upgrade() {
                let items = window.global::<Notices>().get_items();
                if let Some(model) = items.as_any().downcast_ref::<notices::NoticeModel>() {
                    model.dismiss(id.as_str());
                }
            }
        });
    }

    pub fn window(&self) -> &ManagerWindow {
        &self.window
    }

    /// Routes every `Store` UI callback to one handler as a [`StoreAction`].
    /// `ChooseLibraryRoot` is emitted as-is; the host owns the folder picker.
    pub fn set_on_store_action(&self, handler: impl Fn(StoreAction) + 'static) {
        let handler = Rc::new(handler);
        let store = self.window.global::<Store>();
        let h = handler.clone();
        store.on_open(move || h(StoreAction::Open));
        let h = handler.clone();
        store.on_select_provider(move |id| h(StoreAction::SelectProvider { id: id.to_string() }));
        let h = handler.clone();
        store.on_web_login(move || h(StoreAction::WebLogin));
        let h = handler.clone();
        store.on_cancel_web_login(move || h(StoreAction::CancelWebLogin));
        let h = handler.clone();
        store.on_open_destination(move || h(StoreAction::OpenDestination));
        let h = handler.clone();
        store.on_login(move |account, password| {
            h(StoreAction::Login {
                account: account.trim().to_owned(),
                password: password.as_str().to_owned(),
            })
        });
        let h = handler.clone();
        store.on_qr_login(move || h(StoreAction::QrLogin));
        let h = handler.clone();
        store.on_import_local(move || h(StoreAction::ImportLocal));
        let h = handler.clone();
        store.on_submit_guard(move |code| {
            h(StoreAction::SubmitGuard {
                code: code.trim().to_owned(),
            })
        });
        let h = handler.clone();
        store.on_logout(move || h(StoreAction::Logout));
        let h = handler.clone();
        store.on_refresh_library(move || h(StoreAction::RefreshLibrary));
        let h = handler.clone();
        store.on_search(move |query| {
            h(StoreAction::Search {
                query: query.as_str().to_owned(),
            })
        });
        let h = handler.clone();
        // Store IDs stay opaque strings across the UI boundary.
        store.on_select_game(move |title_id| {
            h(StoreAction::SelectGame {
                title_id: title_id.to_string(),
            })
        });
        let h = handler.clone();
        store.on_select_depot(move |depot_id, selected| {
            h(StoreAction::SelectDepot {
                depot_id: depot_id.to_string(),
                selected,
            })
        });
        let h = handler.clone();
        store.on_select_branch(move |branch, password| {
            h(StoreAction::SelectBranch {
                branch: branch.as_str().to_owned(),
                password: password.as_str().to_owned(),
            })
        });
        let h = handler.clone();
        store.on_choose_library_root(move || h(StoreAction::ChooseLibraryRoot));
        let h = handler.clone();
        store.on_download(move || h(StoreAction::Download));
        let h = handler.clone();
        store.on_cancel(move || h(StoreAction::Cancel));
        let h = handler.clone();
        store.on_check_updates(move || h(StoreAction::CheckUpdates));
        store.on_verify(move || handler(StoreAction::Verify));
    }

    pub fn set_session_active(&self, active: bool) {
        self.window.global::<Session>().set_active(active);
    }

    /// Gamepad buttons held right now, by binding id; owned by the host's input pump.
    pub fn set_pad_state(&self, pressed: &[String]) {
        self.pad_state.update(pressed);
    }

    pub fn apply(&self, model: &ManagerViewModel) {
        self.apply_app(model);
        self.apply_library(model);
        self.apply_selection(model);
        self.apply_game_settings(model);
        self.apply_patch(model);
        self.apply_cores(model);
        self.apply_notices(model);
        self.apply_settings(model);
        self.apply_filters(model);
        self.apply_session(model);
        self.onboarding_roots.update(&model.onboarding.roots);
        self.apply_resale_notice(model);
        self.apply_store(model);
    }

    fn apply_resale_notice(&self, model: &ManagerViewModel) {
        let notice = self.window.global::<ResaleNoticeState>();
        let view = &model.resale_notice;
        notice.set_checked(view.checked);
        notice.set_remaining_seconds(view.remaining_seconds);
        notice.set_acknowledged(view.acknowledged);
    }

    fn apply_app(&self, model: &ManagerViewModel) {
        let app = self.window.global::<App>();
        app.set_version(model.app.version.as_str().into());
        app.set_build(model.app.build.as_str().into());
        app.set_appearance(appearance(&model.app.appearance));

        let network = self.window.global::<Network>();
        network.set_allowed(model.network.allowed);
        network.set_syncing(model.network.syncing);
        network.set_sync_status(model.network.sync_status.as_str().into());
        network.set_source(model.network.source.as_str().into());
    }

    fn apply_library(&self, model: &ManagerViewModel) {
        let library = self.window.global::<Library>();
        let view = &model.library;
        library.set_query(view.query.as_str().into());
        library.set_sort(view.sort.as_str().into());
        library.set_compat_filter(view.compat_filter.as_str().into());
        library.set_prepare_state(view.prepare_state.as_str().into());
        library.set_prepare_label(view.prepare_label.as_str().into());
        library.set_prepare_detail(view.prepare_detail.as_str().into());
        library.set_prepare_progress(view.prepare_progress.unwrap_or(-1.0));
        library.set_can_cancel_prepare(view.can_cancel_prepare);
        library.set_selected_id(view.selected_id.as_str().into());

        if *self.cached_games.borrow() != view.games {
            let cards = view
                .games
                .iter()
                .map(|game| (game_card(game), game.cover_uri.clone()))
                .collect::<Vec<_>>();
            self.games.replace(cards, &self.window);
            view.games.clone_into(&mut self.cached_games.borrow_mut());
        } else {
            self.games.refresh(&self.window);
        }
    }

    fn apply_selection(&self, model: &ManagerViewModel) {
        let selection = self.window.global::<Selection>();
        let view = &model.selection;
        let game = &view.game;
        selection.set_game(SelectedGame {
            id: game.id.as_str().into(),
            title: game.title.as_str().into(),
            engine: game.engine.as_str().into(),
            emulator: game.emulator.as_str().into(),
            emulator_id: game.emulator_id.as_str().into(),
            launchable: game.launchable,
            problem: game.problem.as_str().into(),
            play_time: game.play_time.as_str().into(),
            last_played: game.last_played.as_str().into(),
            developer: game.developer.as_str().into(),
            release_date: game.release_date.as_str().into(),
            platforms: game.platforms.as_str().into(),
            description: game.description.as_str().into(),
            tags: game.tags.as_str().into(),
            aliases: game.aliases.as_str().into(),
        });
        self.emulator_choices.update(&view.emulator_choices);
        let compat = &view.compat;
        selection.set_compat(CompatInfo {
            status: compat.status.as_str().into(),
            notes: compat.notes.as_str().into(),
            updated: compat.updated.as_str().into(),
            release_id: compat.release_id.as_str().into(),
            emulator_label: compat.emulator_label.as_str().into(),
            report_url: compat.report_url.as_str().into(),
            hint: compat.hint.as_str().into(),
        });
        self.releases.update(&view.releases);
        selection.set_pinned_release(view.pinned_release.as_str().into());
        self.other_versions.update(&view.other_versions);
        self.history.update(&view.history);
        let bangumi_key = (game.id.clone(), view.bangumi.clone());
        if self.cached_bangumi.borrow().as_ref() != Some(&bangumi_key) {
            selection.set_bangumi(bangumi(&view.bangumi));
            *self.cached_bangumi.borrow_mut() = Some(bangumi_key);
        }
        selection.set_bangumi_summary(view.bangumi_summary.as_str().into());
        selection.set_metadata_busy(view.metadata_busy);
        selection.set_metadata_error(view.metadata_error);
        selection.set_metadata_status(view.metadata_status.as_str().into());
        self.matches.update(&view.matches);
    }

    fn apply_game_settings(&self, model: &ManagerViewModel) {
        let settings = self.window.global::<GameSettings>();
        let view = &model.game_settings;
        settings.set_game_id(view.game_id.as_str().into());
        settings.set_title(view.title.as_str().into());
        self.game_emulator_choices.update(&view.emulator_choices);
        settings.set_emulator_id(view.emulator_id.as_str().into());
        let render = &view.render;
        settings.set_render_supported(render.supported);
        self.render_options.update(&render.options);
        settings.set_render_mode(render.mode.as_str().into());
        settings.set_custom_width(render.custom_width);
        settings.set_custom_height(render.custom_height);
        settings.set_render_min(render.min.as_str().into());
        settings.set_render_max(render.max.as_str().into());
        settings.set_render_note(render.note.as_str().into());
        settings.set_render_error(render.error.as_str().into());
        crate::config_model::update(&self.game_fields, &self.cached_game_fields, &view.fields);
        settings.set_recommended_available(view.recommended_available);
        settings.set_recommended_summary(view.recommended_summary.as_str().into());
        settings.set_input_override(view.input_override);
        self.game_bindings.update(&view.bindings);
    }

    fn apply_patch(&self, model: &ManagerViewModel) {
        let patch = self.window.global::<Patch>();
        let view = &model.patch;
        patch.set_query(view.query.as_str().into());
        patch.set_game(patch_game(&view.game));
        if *self.cached_patch_games.borrow() != view.games {
            let rows = view
                .games
                .iter()
                .map(|game| (patch_game(game), game.cover_uri.clone()))
                .collect::<Vec<_>>();
            self.patch_games.replace(rows, &self.window);
            view.games
                .clone_into(&mut self.cached_patch_games.borrow_mut());
        } else {
            self.patch_games.refresh(&self.window);
        }
        self.recipes.update(&view.recipes);
        self.patch_downloads.update(&view.state.downloads);
        self.patch_checks.update(&view.state.checks);
        patch.set_hint(view.hint.as_str().into());
        patch.set_state(patch_state(&view.state));
    }

    fn apply_cores(&self, model: &ManagerViewModel) {
        let cores = self.window.global::<Cores>();
        let view = &model.cores;
        self.cores.update(&view.items);
        cores.set_restart_required(view.restart_required);
        cores.set_updates_available(view.updates_available);
        crate::config_model::update(
            &self.default_fields,
            &self.cached_default_fields,
            &view.default_fields,
        );
        cores.set_default_plugin(view.default_plugin.as_str().into());
        self.default_render_options
            .update(&view.default_render.options);
        cores.set_default_render(render_state(
            &view.default_render,
            self.default_render_options.model(),
        ));
    }

    fn apply_notices(&self, model: &ManagerViewModel) {
        self.notices.set_model(model.notices.items.clone());
        let snackbar = &model.notices.snackbar;
        if *self.cached_snackbar.borrow() != snackbar.serial {
            *self.cached_snackbar.borrow_mut() = snackbar.serial;
            let notices = self.window.global::<Notices>();
            notices.set_snackbar_text(snackbar.text.as_str().into());
            notices.set_snackbar_action(snackbar.action.as_str().into());
            notices.set_snackbar_serial(notices.get_snackbar_serial().wrapping_add(1));
        }
    }

    fn apply_settings(&self, model: &ManagerViewModel) {
        let settings = self.window.global::<Settings>();
        let view = &model.settings;
        if self.cached_translation.borrow().as_ref() != Some(&view.translation) {
            settings.set_translation(translation_form(&view.translation));
            *self.cached_translation.borrow_mut() = Some(view.translation.clone());
        }
        settings.set_translation_enabled(view.translation_enabled);
        settings.set_secret_saved(view.secret_saved);
        self.models.update(&view.models);
        settings.set_translation_status(view.translation_status.as_str().into());
        settings.set_sensitive_covers(view.sensitive_covers);
        settings.set_bangumi_token_saved(view.bangumi_token_saved);
        settings.set_compat_summary(view.compat_summary.as_str().into());
        settings.set_gamepad_enabled(view.gamepad_enabled);
        settings.set_deadzone(view.deadzone.as_str().into());
        self.bindings.update(&view.bindings);
        self.roots.update(&view.roots);
        self.excluded.update(&view.excluded);
    }

    fn apply_filters(&self, model: &ManagerViewModel) {
        self.window
            .global::<Filters>()
            .set_preset(model.filters.preset.as_str().into());
        crate::config_model::update(
            &self.filter_fields,
            &self.cached_filter_fields,
            &model.filters.fields,
        );
    }

    fn apply_session(&self, model: &ManagerViewModel) {
        let session = self.window.global::<Session>();
        let view = &model.session;
        session.set_phase(view.phase.as_str().into());
        session.set_launch_title(view.launch_title.as_str().into());
        if *self.cached_launch.borrow() != view.launch_game_id {
            let (cover, _) = self
                .games
                .cover_of(view.launch_game_id.as_str())
                .unwrap_or_default();
            session.set_launch_cover(cover);
            view.launch_game_id
                .clone_into(&mut self.cached_launch.borrow_mut());
        }
        self.actions.update(&view.actions);
        session.set_action_message(view.action_message.as_str().into());
        session.set_action_serial(view.action_serial as i32);
    }

    fn apply_store(&self, model: &ManagerViewModel) {
        let store = self.window.global::<Store>();
        let view = &model.store;
        store.set_providers(ModelRc::new(VecModel::from(
            view.providers.iter().map(choice).collect::<Vec<_>>(),
        )));
        store.set_provider_id(view.provider_id.as_str().into());
        store.set_provider_name(view.provider_name.as_str().into());
        store.set_can_qr(view.can_qr);
        store.set_can_credentials(view.can_credentials);
        store.set_can_web(view.can_web);
        store.set_has_branches(view.has_branches);
        store.set_has_depots(view.has_depots);
        store.set_can_verify(view.can_verify);
        store.set_login_state(login_state(view.login_state).into());
        store.set_account_name(view.account_name.as_str().into());
        store.set_can_import_local(view.can_import_local);
        if *self.cached_store_qr.borrow() != view.qr_url {
            store.set_qr(qr_image(&view.qr_url));
            view.qr_url
                .clone_into(&mut self.cached_store_qr.borrow_mut());
        }
        store.set_guard_hint(view.guard_hint.as_str().into());
        store.set_status(view.status.as_str().into());
        store.set_error(view.error.as_str().into());
        store.set_library_state(load_state(view.library_state).into());
        self.store_games.update(&view.games);
        store.set_query(view.query.as_str().into());

        store.set_selected_title_id(view.selected_title_id.as_str().into());
        let selected = view.games.iter().find(|game| {
            game.title_id == view.selected_title_id && !view.selected_title_id.is_empty()
        });
        store.set_selected(selected.map(store_game).unwrap_or_default());

        if *self.cached_store_depots.borrow() != view.depots {
            let (languages, content): (Vec<_>, Vec<_>) = view
                .depots
                .iter()
                .cloned()
                .partition(|depot| !depot.language.is_empty());
            self.store_depots.update(&content);
            self.store_language_depots.update(&languages);
            let (count, bytes) = view
                .depots
                .iter()
                .filter(|depot| depot.selected)
                .fold((0i32, 0u64), |(count, bytes), depot| {
                    (count + 1, bytes.saturating_add(depot.size_bytes))
                });
            store.set_selected_depot_count(count);
            store.set_selected_size(size_text(bytes).into());
            view.depots
                .clone_into(&mut self.cached_store_depots.borrow_mut());
        }
        self.store_branches.update(&view.branches);
        store.set_selected_branch(view.selected_branch.as_str().into());
        // Presence only: the branch password itself never goes back into the UI.
        store.set_branch_password_set(!view.branch_password.is_empty());
        store.set_library_root(view.library_root.as_str().into());
        let name = view
            .games
            .iter()
            .find(|game| game.title_id == view.transfer.title_id)
            .map_or("", |game| game.name.as_str());
        store.set_transfer(store_transfer(&view.transfer, name));
    }
}

fn login_state(state: StoreLoginState) -> &'static str {
    match state {
        StoreLoginState::SignedOut => "signed-out",
        StoreLoginState::Connecting => "connecting",
        StoreLoginState::EmailCode => "email-code",
        StoreLoginState::MobileCode => "mobile-code",
        StoreLoginState::MobileConfirmation => "mobile-confirmation",
        StoreLoginState::QrCode => "qr-code",
        StoreLoginState::SignedIn => "signed-in",
        StoreLoginState::Error => "error",
    }
}

fn load_state(state: StoreLoadState) -> &'static str {
    match state {
        StoreLoadState::Idle => "idle",
        StoreLoadState::Loading => "loading",
        StoreLoadState::Ready => "ready",
        StoreLoadState::Error => "error",
    }
}

fn transfer_state(state: StoreTransferState) -> &'static str {
    match state {
        StoreTransferState::Idle => "idle",
        StoreTransferState::Preparing => "preparing",
        StoreTransferState::Downloading => "downloading",
        StoreTransferState::Verifying => "verifying",
        StoreTransferState::Cancelling => "cancelling",
        StoreTransferState::Cancelled => "cancelled",
        StoreTransferState::Complete => "complete",
        StoreTransferState::Error => "error",
    }
}

fn store_game(value: &StoreGameViewModel) -> StoreGame {
    StoreGame {
        title_id: value.title_id.as_str().into(),
        name: value.name.as_str().into(),
        size: size_text(value.size_bytes).into(),
        platforms: value.platforms.join(" · ").into(),
        languages: value.languages.join(" · ").into(),
        installed: value.installed,
        update_available: value.update_available,
        update_unknown: value.update_unknown,
        unavailable_reason: value.unavailable_reason.as_str().into(),
    }
}

fn store_depot(value: &StoreDepotViewModel) -> StoreDepot {
    StoreDepot {
        depot_id: value.depot_id.as_str().into(),
        name: value.name.as_str().into(),
        platform: value.platform.as_str().into(),
        language: value.language.as_str().into(),
        size: size_text(value.size_bytes).into(),
        selected: value.selected,
    }
}

fn store_transfer(value: &StoreTransferViewModel, name: &str) -> StoreTransfer {
    let bytes = match (value.bytes_done, value.bytes_total) {
        (_, 0) if value.bytes_done == 0 => String::new(),
        (done, 0) => byte_text(done),
        (done, total) => format!("{} / {}", byte_text(done), byte_text(total)),
    };
    let files = match (value.files_done, value.files_total) {
        (0, 0) => String::new(),
        (done, 0) => tr!("{done} 个文件", done = done),
        (done, total) => tr!("{done} / {total} 个文件", done = done, total = total),
    };
    let speed = if value.speed_bytes_per_second == 0 {
        String::new()
    } else {
        format!("{}/s", byte_text(value.speed_bytes_per_second))
    };
    StoreTransfer {
        state: transfer_state(value.state).into(),
        notice: value.notice.as_str().into(),
        can_open_destination: !value.destination.is_empty(),
        title_id: value.title_id.as_str().into(),
        name: name.into(),
        bytes: bytes.into(),
        files: files.into(),
        speed: speed.into(),
        eta: eta_text(value.eta_seconds).into(),
        progress: value.progress.map_or(-1.0, |p| p.clamp(0.0, 1.0)),
        status: value.status.as_str().into(),
        error: value.error.as_str().into(),
    }
}

/// Size of a game or depot; empty while unknown.
fn size_text(bytes: u64) -> String {
    if bytes == 0 {
        String::new()
    } else {
        byte_text(bytes)
    }
}

/// Binary units labelled like Windows Explorer and the Store client ("4.1 GB").
fn byte_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Remaining time; empty while unknown.
fn eta_text(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    match (hours, minutes, seconds) {
        (0, 0, 0) => String::new(),
        (0, 0, s) => tr!("剩余 {s} 秒", s = s),
        (0, m, s) => tr!("剩余 {m} 分 {s} 秒", m = m, s = s),
        (h, m, _) => tr!("剩余 {h} 小时 {m} 分", h = h, m = m),
    }
}

/// One pixel per module; the page scales it with nearest-neighbour filtering
/// and supplies the white quiet zone.
fn qr_image(url: &str) -> Image {
    if url.is_empty() {
        return Image::default();
    }
    let Ok(code) = qrcodegen::QrCode::encode_text(url, qrcodegen::QrCodeEcc::Medium) else {
        // The challenge URL is a login secret; never log it.
        tracing::warn!(
            event = "shinki.emu.manager.store.qr_failed",
            length = url.len()
        );
        return Image::default();
    };
    let size = code.size();
    let side = size as u32;
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(side, side);
    let dark = Rgba8Pixel::new(0, 0, 0, 255);
    let light = Rgba8Pixel::new(255, 255, 255, 255);
    for (index, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = ((index as i32) % size, (index as i32) / size);
        *pixel = if code.get_module(x, y) { dark } else { light };
    }
    Image::from_rgba8(pixels)
}

fn strings(values: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        values
            .iter()
            .map(|value| SharedString::from(value.as_str()))
            .collect::<Vec<_>>(),
    ))
}

fn choice(value: &ChoiceViewModel) -> Choice {
    Choice {
        id: value.id.as_str().into(),
        label: value.label.as_str().into(),
    }
}

fn release(value: &ReleaseViewModel) -> Release {
    Release {
        id: value.release_id.as_str().into(),
        title: value.title.as_str().into(),
        // One wrapped line per card: a platform list is display text, and the
        // card wraps instead of eliding it.
        platforms: value.platforms.join(" · ").into(),
        released: value.released.as_deref().unwrap_or_default().into(),
    }
}

fn appearance(value: &AppearanceViewModel) -> Appearance {
    Appearance {
        theme_mode: value.theme_mode.as_str().into(),
        density: value.density.as_str().into(),
        cover_size: value.cover_size.as_str().into(),
        view_mode: value.view_mode.as_str().into(),
        reduce_motion: value.reduce_motion,
        cover_badges: value.cover_badges,
        language: value.language.as_str().into(),
    }
}

fn game_card(value: &GameCardViewModel) -> GameCard {
    GameCard {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        subtitle: value.subtitle.as_str().into(),
        version: value.version.as_str().into(),
        cover: slint::Image::default(),
        tint: slint::Color::default(),
        ready: value.ready,
        problem: value.problem.as_str().into(),
        play_time: value.play_time.as_str().into(),
        last_played: value.last_played.as_str().into(),
        compat: value.compat.as_str().into(),
    }
}

fn play_session(value: &PlaySessionViewModel) -> PlaySession {
    PlaySession {
        started: value.started.as_str().into(),
        duration: value.duration.as_str().into(),
        ended_by: value.ended_by.as_str().into(),
    }
}

fn match_review(value: &MatchViewModel) -> MatchReview {
    MatchReview {
        id: value.id.as_str().into(),
        provider: value.provider.as_str().into(),
        title: value.title.as_str().into(),
        aliases: value.aliases.as_str().into(),
        release_date: value.release_date.as_str().into(),
        developer: value.developer.as_str().into(),
    }
}

fn bangumi(value: &BangumiViewModel) -> BangumiProgress {
    BangumiProgress {
        status: value.status.as_str().into(),
        rating: value.rating,
        note: value.note.as_str().into(),
    }
}

fn binding(value: &GamepadBindingViewModel) -> GamepadBinding {
    GamepadBinding {
        id: value.id.as_str().into(),
        label: value.label.as_str().into(),
        key: value.key.as_str().into(),
    }
}

fn recipe(value: &RecipeViewModel) -> Recipe {
    Recipe {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        recommended: value.recommended,
    }
}

fn patch_game(value: &PatchGameViewModel) -> PatchGame {
    PatchGame {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        subtitle: value.subtitle.as_str().into(),
        linked: value.linked,
        cover: slint::Image::default(),
        tint: slint::Color::default(),
    }
}

fn patch_state(value: &PatchStateViewModel) -> PatchState {
    PatchState {
        step: value.step,
        running: value.running,
        phase: value.phase.as_str().into(),
        completed: value.completed as f32,
        total: value.total.map_or(-1.0, |total| total as f32),
        output: value.output.as_str().into(),
        error: value.error.as_str().into(),
        can_start: value.can_start,
        script: value.script.as_str().into(),
        patch_directory: value.patch_directory.as_str().into(),
        output_directory: value.output_directory.as_str().into(),
        in_place: value.in_place,
        has_backup: value.has_backup,
        rolled_back: value.rolled_back,
    }
}

fn patch_download(value: &PatchDownloadViewModel) -> PatchDownload {
    let bytes = match value.total {
        Some(total) => format!("{} / {}", byte_text(value.completed), byte_text(total)),
        None => byte_text(value.completed),
    };
    PatchDownload {
        name: value.name.as_str().into(),
        bytes: bytes.into(),
        progress: match value.total {
            Some(total) if total > 0 => (value.completed as f32 / total as f32).clamp(0.0, 1.0),
            _ => -1.0,
        },
        sha256: value.sha256.as_str().into(),
        verify: match value.verify {
            PatchVerify::Pending => "pending",
            PatchVerify::Ok => "ok",
            PatchVerify::Failed => "failed",
        }
        .into(),
    }
}

fn patch_check(value: &PatchCheckViewModel) -> PatchCheck {
    PatchCheck {
        label: value.label.as_str().into(),
        state: match value.passed {
            None => "running",
            Some(true) => "passed",
            Some(false) => "failed",
        }
        .into(),
        detail: value.detail.as_str().into(),
    }
}

fn render_state(value: &RenderViewModel, options: ModelRc<Choice>) -> RenderState {
    RenderState {
        supported: value.supported,
        options,
        mode: value.mode.as_str().into(),
        custom_width: value.custom_width,
        custom_height: value.custom_height,
        min: value.min.as_str().into(),
        max: value.max.as_str().into(),
        note: value.note.as_str().into(),
        error: value.error.as_str().into(),
    }
}

fn core_info(value: &CoreViewModel) -> CoreInfo {
    CoreInfo {
        id: value.id.as_str().into(),
        name: value.name.as_str().into(),
        engine: value.engine.as_str().into(),
        version: value.version.as_str().into(),
        state: value.state.as_str().into(),
        source: value.source.as_str().into(),
        message: value.message.as_str().into(),
        capabilities: strings(&value.capabilities),
        removable: value.removable,
        update_version: value.update_version.as_str().into(),
    }
}

fn translation_form(value: &TranslationFormViewModel) -> TranslationForm {
    TranslationForm {
        provider: value.provider.as_str().into(),
        base_url: value.base_url.as_str().into(),
        openai_api: value.openai_api.as_str().into(),
        model: value.model.as_str().into(),
        language: value.language.as_str().into(),
        timeout_s: value.timeout_s,
        custom_prompt: value.custom_prompt.as_str().into(),
        context_segments: value.context_segments,
        context_chars: value.context_chars,
    }
}

fn excluded(value: &ExcludedViewModel) -> Excluded {
    Excluded {
        id: value.id.as_str().into(),
        title: value.title.as_str().into(),
        location: value.location.as_str().into(),
    }
}

fn engine_action(value: &EngineActionViewModel) -> EngineAction {
    EngineAction {
        control: value.control,
        label: value.label.as_str().into(),
        icon: value.icon.as_str().into(),
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn store_sizes_and_eta_are_readable() {
        assert_eq!(size_text(0), "");
        assert_eq!(byte_text(512), "512 B");
        assert_eq!(byte_text(4_402_341_478), "4.1 GB");
        assert_eq!(byte_text(125_829_120), "120 MB");
        assert_eq!(eta_text(0), "");
        assert_eq!(eta_text(138), "剩余 2 分 18 秒");
        assert_eq!(eta_text(3_900), "剩余 1 小时 5 分");
    }

    #[test]
    fn transfer_without_totals_omits_unknown_facts() {
        let transfer = store_transfer(
            &StoreTransferViewModel {
                state: StoreTransferState::Preparing,
                ..Default::default()
            },
            "",
        );
        assert_eq!(transfer.state, "preparing");
        assert_eq!(transfer.bytes, "");
        assert_eq!(transfer.files, "");
        assert_eq!(transfer.speed, "");
        assert_eq!(transfer.progress, -1.0);
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn script_downloads_show_size_progress_and_verification() {
        let partial = patch_download(&PatchDownloadViewModel {
            name: "fonts.zip".into(),
            completed: 7_759_462,
            total: Some(18_874_368),
            sha256: "4b1d".into(),
            verify: PatchVerify::Pending,
        });
        assert_eq!(partial.bytes, "7.4 MB / 18.0 MB");
        assert!((partial.progress - 0.411).abs() < 0.001);
        assert_eq!(partial.verify, "pending");

        let unknown = patch_download(&PatchDownloadViewModel {
            completed: 2048,
            verify: PatchVerify::Failed,
            ..Default::default()
        });
        assert_eq!(unknown.bytes, "2.0 KB");
        assert_eq!(unknown.progress, -1.0);
        assert_eq!(unknown.verify, "failed");
    }

    #[test]
    fn script_checks_map_to_running_passed_failed() {
        let state = |passed| {
            patch_check(&PatchCheckViewModel {
                passed,
                ..Default::default()
            })
            .state
        };
        assert_eq!(state(None), "running");
        assert_eq!(state(Some(true)), "passed");
        assert_eq!(state(Some(false)), "failed");
    }
}
