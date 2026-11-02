use super::*;
use shinki_emu_manager_ui_slint::tr;
use shinki_emu_manager_ui_slint::{
    AppViewModel, BangumiViewModel, ChoiceViewModel, CoresViewModel, EngineActionViewModel,
    ExcludedViewModel, FiltersViewModel, GameCardViewModel, GameSettingsViewModel,
    LibraryViewModel, MatchViewModel, NoticeViewModel, NoticesViewModel, OnboardingViewModel,
    PatchGameViewModel, PatchViewModel, PlaySessionViewModel, ReleaseViewModel,
    SelectedGameViewModel, SelectionViewModel, SessionViewModel, SettingsViewModel, describe_error,
};

impl ShinkiEmuManagerController {
    pub(super) fn model(&mut self) -> Result<ManagerViewModel, String> {
        let now_ms = unix_time_ms()?;
        let query = self.search_query.trim().to_ascii_lowercase();
        let patch_query = self.patch_query.trim().to_ascii_lowercase();
        let shinki_emu_manager_core::LibrarySnapshot {
            games,
            play_stats,
            identities,
            metadata: metadata_snapshots,
        } = self
            .library
            .library_snapshot()
            .map_err(|error| error.to_string())?;
        let mut selected = None;
        let mut metadata = None;
        let mut rows = Vec::new();
        let mut patch_games = Vec::new();
        let mut patch = PatchViewModel {
            query: self.patch_query.clone(),
            state: self.patch_state_view(),
            ..PatchViewModel::default()
        };
        let versions = self.version_labels(&games, &identities);
        for mut game in games {
            let linked = identities
                .get(&game.game_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let record = Self::metadata_from_snapshot(&game.game_id, linked, &metadata_snapshots)?;
            let has_metadata = record.is_some();
            if self.selected_case_id.as_deref() == Some(game.game_id.as_str()) {
                if let Some(record) = record {
                    game.title.clone_from(&record.title);
                    metadata = Some(record);
                }
                selected = Some(game.clone());
            } else if let Some(record) = record {
                game.title = record.title;
            }
            let cover_uri = if has_metadata {
                self.data_dir
                    .join("covers")
                    .join(&game.game_id)
                    .to_str()
                    .ok_or("SHINKI_EMU_METADATA_COVER_PATH_UTF8")?
                    .to_owned()
            } else {
                String::new()
            };
            let matches = |query: &str| {
                query.is_empty()
                    || game.display_title().to_ascii_lowercase().contains(query)
                    || game.game_id.to_ascii_lowercase().contains(query)
            };
            let patch_selected = self.patch_game_id.as_deref() == Some(game.game_id.as_str());
            let patch_listed = matches(&patch_query);
            if patch_selected || patch_listed {
                let row = PatchGameViewModel {
                    id: game.game_id.clone(),
                    title: game.display_title().into(),
                    subtitle: self.engine_and_emulator(&game),
                    cover_uri: cover_uri.clone(),
                    linked: Self::vndb_id(linked).is_some(),
                };
                if patch_selected {
                    (patch.recipes, patch.hint) = self.patch_choices(&game.game_id, linked)?;
                    patch.game = row.clone();
                }
                if patch_listed {
                    patch_games.push(row);
                }
            }
            if !matches(&query) {
                continue;
            }
            let stats = play_stats
                .get(&game.game_id)
                .ok_or("SHINKI_EMU_LIBRARY_SNAPSHOT_STATS")?;
            let compatibility_status = self.compatibility_status(&game.game_id, linked);
            if self.compatibility_filter != "all"
                && (self.compatibility_filter == "unknown" && !compatibility_status.is_empty()
                    || self.compatibility_filter != "unknown"
                        && self.compatibility_filter != compatibility_status)
            {
                continue;
            }
            rows.push((
                game,
                stats.total_duration_ms,
                stats.last_played_unix_ms,
                cover_uri,
                compatibility_status,
            ));
        }
        match self.library_sort.as_str() {
            "recent" => rows.sort_by(|left, right| {
                right
                    .2
                    .unwrap_or(i64::MIN)
                    .cmp(&left.2.unwrap_or(i64::MIN))
                    .then_with(|| {
                        left.0
                            .display_title()
                            .to_ascii_lowercase()
                            .cmp(&right.0.display_title().to_ascii_lowercase())
                    })
            }),
            "play_time" => rows.sort_by(|left, right| {
                right
                    .1
                    .cmp(&left.1)
                    .then_with(|| left.0.game_id.cmp(&right.0.game_id))
            }),
            _ => rows.sort_by(|left, right| {
                left.0
                    .display_title()
                    .to_ascii_lowercase()
                    .cmp(&right.0.display_title().to_ascii_lowercase())
                    .then_with(|| left.0.game_id.cmp(&right.0.game_id))
            }),
        }
        if self.selected_case_id.is_some() && selected.is_none() {
            return Err("SHINKI_EMU_GAME_NOT_FOUND".into());
        }
        let cards = rows
            .into_iter()
            .map(
                |(game, duration_ms, last_played_ms, cover_uri, compatibility_status)| {
                    let problem = self.game_problem(&game);
                    GameCardViewModel {
                        subtitle: self.engine_and_emulator(&game),
                        version: versions
                            .get(&game.game_id)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_default(),
                        title: game.display_title().into(),
                        id: game.game_id,
                        cover_uri,
                        ready: problem.is_empty(),
                        problem,
                        play_time: human_duration(duration_ms),
                        last_played: last_played_ms
                            .map(|value| human_relative(value, now_ms))
                            .unwrap_or_default(),
                        compat: compatibility_status,
                    }
                },
            )
            .collect();
        let selection = match &selected {
            Some(game) => {
                let linked = identities
                    .get(&game.game_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let stats = play_stats
                    .get(&game.game_id)
                    .ok_or("SHINKI_EMU_LIBRARY_SNAPSHOT_STATS")?;
                let mut view =
                    self.selection_view(game, metadata.as_ref(), linked, stats, now_ms)?;
                if let Some((vn_id, _)) = versions.get(&game.game_id) {
                    view.other_versions = versions
                        .iter()
                        .filter(|(id, (other, _))| other == vn_id && *id != &game.game_id)
                        .map(|(id, (_, label))| ChoiceViewModel {
                            id: id.clone(),
                            label: label.clone(),
                        })
                        .collect();
                }
                view
            }
            None => SelectionViewModel::default(),
        };
        let game_settings = match &selected {
            Some(game) => self.game_settings_view(game)?,
            None => GameSettingsViewModel::default(),
        };
        patch_games.sort_by(|left, right| {
            left.title
                .to_ascii_lowercase()
                .cmp(&right.title.to_ascii_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        patch.games = patch_games;
        let (prepare_state, prepare_label, prepare_progress, can_cancel_prepare) =
            self.prepare_view();
        let roots: Vec<String> = self
            .library
            .library_roots()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|root| root.path)
            .collect();
        let profile = self.profile()?;
        Ok(ManagerViewModel {
            app: AppViewModel {
                version: shinki_emu_manager::version().into(),
                build: option_env!("SHINKI_EMU_MANAGER_RUSTC_FINGERPRINT")
                    .unwrap_or("development")
                    .into(),
                appearance: self.appearance_view(),
            },
            network: self.network_view(now_ms),
            self_update: self.self_update_view(),
            library: LibraryViewModel {
                games: cards,
                query: self.search_query.clone(),
                sort: self.library_sort.clone(),
                compat_filter: self.compatibility_filter.clone(),
                prepare_state: prepare_state.into(),
                prepare_label: prepare_label.into(),
                prepare_detail: self.startup_status(),
                prepare_progress,
                can_cancel_prepare,
                selected_id: self.selected_case_id.clone().unwrap_or_default(),
            },
            selection,
            game_settings,
            patch,
            cores: self.cores_model()?,
            notices: NoticesViewModel {
                items: self.notice_views(),
                snackbar: self.snackbar.view(),
            },
            settings: SettingsViewModel {
                translation: translation::profile_form(profile.as_ref()),
                translation_enabled: self.translation_enabled,
                secret_saved: profile.is_some(),
                models: self.translation_models.clone(),
                translation_status: self.translation_status.clone(),
                sensitive_covers: self.sensitive_covers,
                bangumi_token_saved: self
                    .metadata_tokens
                    .contains_key(MetadataProviderId::Bangumi.as_str()),
                compat_summary: self.compatibility_summary(),
                gamepad_enabled: self.input_mapping.gamepad_enabled,
                deadzone: self.input_mapping.deadzone.as_str().into(),
                bindings: view_fields::binding_views(&self.input_mapping),
                roots: roots.clone(),
                excluded: self
                    .library
                    .excluded_games()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(|game| ExcludedViewModel {
                        id: game.game_id,
                        title: game.title,
                        location: game.location,
                    })
                    .collect(),
            },
            filters: FiltersViewModel {
                preset: self.filter_preset_id().into(),
                fields: self.filter_fields()?,
            },
            session: self.session_view()?,
            onboarding: OnboardingViewModel { roots },
            resale_notice: self.notice_reading.view(),
            store: self.store.view(),
        })
    }

    fn engine_and_emulator(&self, game: &GameRecord) -> String {
        let engine = game
            .family_id
            .as_deref()
            .map(|id| self.engine_label(id))
            .unwrap_or_else(|| tr!("未识别引擎").to_owned());
        match self.launch_candidate_for_game(game) {
            Some(candidate) => format!("{engine} · {}", candidate.descriptor.display_name),
            None => engine,
        }
    }

    /// `(vID, label)` for every installed copy whose VNDB work is linked by
    /// another copy too: the pinned release title, or else the install
    /// directory name. Copies of a work installed once have no entry.
    fn version_labels(
        &self,
        games: &[GameRecord],
        identities: &BTreeMap<String, Vec<shinki_emu_manager_core::ExternalIdentityRecord>>,
    ) -> BTreeMap<String, (String, String)> {
        let mut works: BTreeMap<&str, Vec<&GameRecord>> = BTreeMap::new();
        for game in games {
            if let Some(vn_id) = identities
                .get(&game.game_id)
                .and_then(|linked| Self::vndb_id(linked))
            {
                works.entry(vn_id).or_default().push(game);
            }
        }
        let mut labels = BTreeMap::new();
        for (vn_id, copies) in works.into_iter().filter(|(_, copies)| copies.len() > 1) {
            for game in copies {
                let label = match self.pinned_releases.get(&game.game_id) {
                    Some(release) if !release.title.is_empty() => release.title.clone(),
                    _ => Path::new(&game.location).file_name().map_or_else(
                        || game.location.clone(),
                        |name| name.to_string_lossy().into_owned(),
                    ),
                };
                labels.insert(game.game_id.clone(), (vn_id.to_owned(), label));
            }
        }
        labels
    }

    fn selection_view(
        &self,
        game: &GameRecord,
        metadata: Option<&MetadataRecord>,
        linked: &[shinki_emu_manager_core::ExternalIdentityRecord],
        stats: &shinki_emu_manager_core::PlayStats,
        now_ms: i64,
    ) -> Result<SelectionViewModel, String> {
        let history = self
            .library
            .session_history(&game.game_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|session| PlaySessionViewModel {
                started: local_time_label(session.start_unix_ms),
                duration: human_duration(session.duration_ms),
                ended_by: match session.ended_by {
                    Some(PlaySessionEndReason::Leave) => tr!("返回游戏库").into(),
                    Some(PlaySessionEndReason::Finished) => tr!("游戏已结束").into(),
                    Some(PlaySessionEndReason::Shutdown) => tr!("关闭程序").into(),
                    Some(PlaySessionEndReason::Crash) => tr!("异常结束").into(),
                    None => tr!("进行中").into(),
                },
            })
            .collect();
        let candidate = self.launch_candidate_for_game(game);
        let joined = |values: &[String]| values.join(", ");
        Ok(SelectionViewModel {
            game: SelectedGameViewModel {
                id: game.game_id.clone(),
                title: game.display_title().into(),
                engine: game
                    .family_id
                    .as_deref()
                    .map(|id| self.engine_label(id))
                    .or_else(|| metadata.and_then(|record| record.engine.clone()))
                    .unwrap_or_default(),
                emulator: candidate
                    .map(|candidate| self.emulator_label(&candidate.descriptor))
                    .unwrap_or_default(),
                emulator_id: self
                    .candidates
                    .get(&game.game_id)
                    .map(|candidate| candidate.report.plugin_id.clone())
                    .unwrap_or_default(),
                launchable: !self.patch_busy()
                    && !self.startup_busy()
                    && !shinki_emu_patch::is_staging_path(Path::new(&game.location))
                    && candidate.is_some(),
                problem: self.game_problem(game),
                play_time: human_duration(stats.total_duration_ms),
                last_played: stats
                    .last_played_unix_ms
                    .map(|value| human_relative(value, now_ms))
                    .unwrap_or_default(),
                developer: metadata
                    .map(|record| joined(&record.developers))
                    .unwrap_or_default(),
                release_date: metadata
                    .and_then(|record| record.release_date.clone())
                    .unwrap_or_default(),
                platforms: metadata
                    .map(|record| joined(&record.platforms))
                    .unwrap_or_default(),
                description: metadata
                    .and_then(|record| record.description.clone())
                    .unwrap_or_default(),
                tags: metadata
                    .map(|record| joined(&record.tags))
                    .unwrap_or_default(),
                aliases: metadata
                    .map(|record| joined(&record.alternate_titles))
                    .unwrap_or_default(),
            },
            emulator_choices: self.emulator_choices(&game.game_id),
            compat: self.compat_view(&game.game_id, linked, now_ms),
            releases: self.release_views(&game.game_id),
            pinned_release: self
                .pinned_releases
                .get(&game.game_id)
                .map(|release| release.release_id.clone())
                .unwrap_or_default(),
            other_versions: Vec::new(),
            history,
            bangumi: BangumiViewModel {
                status: self.bangumi_play_status.clone(),
                rating: self.bangumi_rating,
                note: self.bangumi_note.clone(),
            },
            bangumi_summary: tr!("同步需手动点击，不会自动上传。").into(),
            metadata_busy: self
                .pending_metadata
                .values()
                .any(|pending| pending.game_id.as_deref() == Some(game.game_id.as_str())),
            metadata_status: self
                .metadata_status
                .get(&game.game_id)
                .map(|status| status.text().to_owned())
                .or_else(|| {
                    metadata.map(|record| {
                        tr!(
                            "已关联 {provider}：{title}",
                            provider = record.provider.as_str(),
                            title = record.title
                        )
                    })
                })
                .unwrap_or_default(),
            metadata_error: matches!(
                self.metadata_status.get(&game.game_id),
                Some(MetadataStatus::Error(_))
            ),
            matches: self
                .match_candidates
                .iter()
                .filter(|(_, (game_id, _))| *game_id == game.game_id)
                .map(|(candidate_id, (_, record))| MatchViewModel {
                    id: candidate_id.clone(),
                    provider: record.provider.as_str().into(),
                    title: record.title.clone(),
                    aliases: record.alternate_titles.join(" / "),
                    release_date: record.release_date.clone().unwrap_or_default(),
                    developer: record.developers.join(" / "),
                })
                .collect(),
        })
    }

    fn release_views(&self, game_id: &str) -> Vec<ReleaseViewModel> {
        let mut releases: Vec<_> = self
            .releases
            .get(game_id)
            .into_iter()
            .flatten()
            .map(|release| ReleaseViewModel {
                release_id: release.release_id.clone(),
                title: release.title.clone().unwrap_or_default(),
                platforms: release.platforms.clone(),
                released: release.released.clone(),
            })
            .collect();
        // Older persisted pins contain only an id and, since schema 7, a title.
        if let Some(pinned) = self.pinned_releases.get(game_id)
            && !releases
                .iter()
                .any(|release| release.release_id == pinned.release_id)
        {
            releases.push(ReleaseViewModel {
                release_id: pinned.release_id.clone(),
                title: pinned.title.clone(),
                ..Default::default()
            });
        }
        releases
    }

    fn game_settings_view(&mut self, game: &GameRecord) -> Result<GameSettingsViewModel, String> {
        let game_id = game.game_id.as_str();
        let override_mapping = self
            .library
            .game_settings(game_id)
            .map_err(|error| error.to_string())?
            .and_then(|settings| settings.input_mapping);
        let recommended = self
            .recommended_preset(game_id)?
            .map(|preset| preset.summary.clone());
        Ok(GameSettingsViewModel {
            game_id: game_id.into(),
            title: game.display_title().into(),
            emulator_choices: self
                .probe_selection_for_game(game_id)
                .unwrap_or_default()
                .iter()
                .map(|choice| ChoiceViewModel {
                    id: choice.report.plugin_id.clone(),
                    label: self.emulator_label(&choice.descriptor),
                })
                .collect(),
            emulator_id: self
                .candidates
                .get(game_id)
                .map(|candidate| candidate.report.plugin_id.clone())
                .unwrap_or_default(),
            render: self.game_render_view(game_id)?,
            fields: self.game_config_fields(game_id)?,
            recommended_available: recommended.is_some(),
            recommended_summary: recommended.unwrap_or_default(),
            input_override: override_mapping.is_some(),
            bindings: override_mapping
                .as_ref()
                .map(view_fields::binding_views)
                .unwrap_or_default(),
        })
    }

    fn cores_model(&mut self) -> Result<CoresViewModel, String> {
        let mut cores = self.cores_view()?;
        if let Some(plugin) = self.defaults_plugin.clone()
            && self.registry.descriptor(&plugin).is_some()
        {
            cores.default_plugin.clone_from(&plugin);
            cores.default_fields = self.emulator_default_fields(&plugin)?;
            cores.default_render = self.emulator_render_view(&plugin)?;
        }
        Ok(cores)
    }

    fn notice_views(&self) -> Vec<NoticeViewModel> {
        let mut notices = Vec::new();
        for (file, raw) in &self.plugin_errors {
            let text = describe_error(raw);
            notices.push(NoticeViewModel {
                id: format!("core-load:{file}"),
                severity: "error".into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: file.clone(),
            });
        }
        if self.registry.is_ready() && self.registry.descriptors().next().is_none() {
            notices.push(NoticeViewModel {
                id: "no-cores".into(),
                severity: "warning".into(),
                title: tr!("没有可用的核心").into(),
                detail: tr!("请在“核心”页安装核心，或使用包含 cores 目录的完整运行包。").into(),
                code: String::new(),
                core: String::new(),
            });
        }
        if !self.diagnostic.is_empty() {
            let text = describe_error(&self.diagnostic);
            notices.push(NoticeViewModel {
                id: format!("diagnostic:{}", &sha256_id(&self.diagnostic)[..12]),
                severity: self.diagnostic_severity.into(),
                title: text.title,
                detail: text.detail,
                code: text.code,
                core: String::new(),
            });
        }
        notices
    }

    fn session_view(&self) -> Result<SessionViewModel, String> {
        let active = self
            .active
            .as_ref()
            .filter(|active| active.closing.is_none());
        let launch_title = match active {
            Some(active) => self.game(&active.game_id)?.display_title().into(),
            None => String::new(),
        };
        Ok(SessionViewModel {
            phase: if active.is_some_and(|active| active.opened) {
                "running"
            } else {
                "opening"
            }
            .into(),
            launch_title,
            launch_game_id: active
                .map(|active| active.game_id.clone())
                .unwrap_or_default(),
            actions: self.session_actions(),
            action_message: self.action_message.clone(),
            action_serial: self.action_serial,
        })
    }

    fn game_problem(&self, game: &GameRecord) -> String {
        if shinki_emu_patch::is_staging_path(Path::new(&game.location)) {
            return tr!("这是尚未发布的临时目录，不能启动").into();
        }
        if self.patch_busy() {
            return tr!("文件补丁转换或导入尚未结束，暂不能启动游戏").into();
        }
        if self.launch_candidate_for_game(game).is_some() {
            return String::new();
        }
        match self.startup_state {
            background::StartupState::Waiting => tr!("需要先准备资料库").into(),
            background::StartupState::Loading | background::StartupState::Scanning(_) => {
                tr!("正在准备资料库").into()
            }
            background::StartupState::Failed(_) => tr!("准备失败，请查看核心诊断").into(),
            background::StartupState::Ready if self.probe_choices.contains_key(&game.game_id) => {
                tr!("需要选择模拟器").into()
            }
            background::StartupState::Ready => tr!("未识别到可用的模拟器").into(),
        }
    }

    fn session_actions(&self) -> Vec<EngineActionViewModel> {
        let Some(active) = self
            .active
            .as_ref()
            .filter(|active| active.closing.is_none())
        else {
            return Vec::new();
        };
        let Some(plugin_id) = self
            .candidates
            .get(&active.game_id)
            .map(|candidate| candidate.descriptor.plugin_id.as_str())
        else {
            return Vec::new();
        };
        self.registry
            .core_metadata()
            .find(|meta| meta.descriptor.plugin_id == plugin_id)
            .map(|meta| {
                meta.menu
                    .iter()
                    .map(|item| EngineActionViewModel {
                        control: item.control as i32,
                        label: item.label.clone(),
                        icon: menu_icon_name(item.icon).into(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn menu_icon_name(icon: u32) -> &'static str {
    match icon {
        shinki_emu_abi::MENU_ICON_QUICK_SAVE => "quick-save",
        shinki_emu_abi::MENU_ICON_QUICK_LOAD => "quick-load",
        shinki_emu_abi::MENU_ICON_SAVE => "save",
        shinki_emu_abi::MENU_ICON_LOAD => "load",
        shinki_emu_abi::MENU_ICON_AUTO => "auto",
        shinki_emu_abi::MENU_ICON_SKIP => "skip",
        shinki_emu_abi::MENU_ICON_BACKLOG => "backlog",
        shinki_emu_abi::MENU_ICON_SETTINGS => "settings",
        _ => "generic",
    }
}
