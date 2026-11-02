use crate::session::types::{OpenRequest, OpenResponse};
use crate::{
    MusicaVm, ScriptEncoding,
    audio::Audio,
    profile::mount_musica_with_resources,
    scene::{Scene, core_error, error},
    session::MusicaSession,
};
use shinki_emu_musica_keys::MUSICA_KEY_FILE;
use shinki_emu_sdk::runtime::*;
use std::{
    path::{Component, Path},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) struct SessionLease;
impl SessionLease {
    fn acquire() -> SessionResult<Self> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_ACTIVE",
                    "a Musica session is already active",
                )
            })?;
        Ok(Self)
    }
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
#[derive(Default)]
pub(crate) struct MusicaProvider {
    gpu_instance: Option<wgpu::Instance>,
}
pub(crate) const FAMILY_ID: &str = "musica";
pub(crate) const PLUGIN_ID: &str = "shinki.emu.musica";
pub(crate) const SUPPORTED_FORMATS: [&str; 2] = ["musica.paz", "musica.sc"];

/// Family schema field plus the C ABI presentation metadata shown by Manager.
pub(crate) struct MusicaField {
    pub field: ConfigField,
    pub description: &'static str,
    pub choice_labels: Vec<&'static str>,
    pub flags: u32,
    pub step: f64,
    pub unit: &'static str,
}
impl MusicaField {
    pub(crate) fn new(
        id: &str,
        label: &str,
        group: &str,
        kind: ConfigKind,
        default: ConfigValue,
        description: &'static str,
    ) -> Self {
        Self {
            field: ConfigField {
                id: id.into(),
                label: label.into(),
                group: group.into(),
                kind,
                default,
            },
            description,
            choice_labels: Vec::new(),
            flags: 0,
            step: 0.0,
            unit: "",
        }
    }
    fn text(id: &str, label: &str, group: &str, default: &str, description: &'static str) -> Self {
        Self::new(
            id,
            label,
            group,
            ConfigKind::String { max_bytes: 4096 },
            ConfigValue::String(default.into()),
            description,
        )
    }
    pub(crate) fn choice(
        id: &str,
        label: &str,
        group: &str,
        choices: &[(&'static str, &'static str)],
        default: &str,
        description: &'static str,
    ) -> Self {
        let mut field = Self::new(
            id,
            label,
            group,
            ConfigKind::Enum {
                choices: choices.iter().map(|(choice, _)| (*choice).into()).collect(),
            },
            ConfigValue::Enum(default.into()),
            description,
        );
        field.choice_labels = choices.iter().map(|(_, label)| *label).collect();
        field
    }
    pub(crate) fn flags(mut self, flags: u32) -> Self {
        self.flags = flags;
        self
    }
    pub(crate) fn step(mut self, step: f64, unit: &'static str) -> Self {
        self.step = step;
        self.unit = unit;
        self
    }
}
pub(crate) fn musica_fields() -> Vec<MusicaField> {
    use shinki_emu_abi::{
        CONFIG_FLAG_ADVANCED as ADVANCED, CONFIG_FLAG_DIRECTORY_PATH as DIRECTORY,
        CONFIG_FLAG_FILE_PATH as FILE, CONFIG_FLAG_GAME_RELATIVE as GAME_RELATIVE,
    };
    let mut fields = vec![
        MusicaField::new(
            "profile_file",
            "密钥文件（key.toml）",
            "文件",
            ConfigKind::String { max_bytes: 256 },
            ConfigValue::String(MUSICA_KEY_FILE.into()),
            "解密 PAZ 归档所需的私有密钥文件；相对路径按游戏目录解析，也可以填写绝对路径。",
        )
        .flags(FILE | GAME_RELATIVE),
        MusicaField::new(
            "entry_script",
            "入口脚本",
            "脚本",
            ConfigKind::String { max_bytes: 256 },
            ConfigValue::String("test.sc".into()),
            "直接进入脚本时从 scr 归档加载的 .sc 文件名，只能填写单个文件名。",
        )
        .flags(ADVANCED),
        MusicaField::text(
            "scr_archive",
            "脚本归档",
            "文件",
            "",
            "代替 scr.paz 读取的脚本归档，路径相对游戏目录；留空使用 scr.paz。",
        )
        .flags(FILE | GAME_RELATIVE | ADVANCED),
        MusicaField::text(
            "sys_archive",
            "系统归档",
            "文件",
            "",
            "代替 sys.paz 读取的系统归档，路径相对游戏目录；留空使用 sys.paz。",
        )
        .flags(FILE | GAME_RELATIVE | ADVANCED),
        MusicaField::text(
            "script_directory",
            "散装脚本目录",
            "脚本",
            "",
            "目录中的 .sc 文件优先于 scr 归档中的同名脚本，路径相对游戏目录；留空则不使用。",
        )
        .flags(DIRECTORY | GAME_RELATIVE | ADVANCED),
        MusicaField::text(
            "subtitle_map",
            "影片字幕映射",
            "语言",
            "",
            "为过场影片加载字幕的映射文件，路径相对游戏目录；留空则不显示影片字幕。",
        )
        .flags(FILE | GAME_RELATIVE),
        MusicaField::text(
            "font_path",
            "字体文件",
            "显示",
            "",
            "替换正文字体的字体文件，路径相对游戏目录；留空使用默认字体。",
        )
        .flags(FILE | GAME_RELATIVE),
        MusicaField::choice(
            "launch_mode",
            "启动方式",
            "播放",
            &[("direct", "直接进入脚本"), ("title", "标题菜单启动")],
            "title",
            "从标题菜单正常启动，或跳过标题直接运行入口脚本。",
        ),
        MusicaField::new(
            "message_speed_auto_play",
            "自动播放间隔",
            "播放",
            ConfigKind::Integer { min: 0, max: 100 },
            ConfigValue::Integer(50),
            "自动播放时每条消息结束后的等待时间；游戏内尚未保存设置时作为初始值。",
        )
        .step(1.0, "×10 ms"),
        MusicaField::new(
            "progress_in_background",
            "后台继续运行",
            "播放",
            ConfigKind::Bool,
            ConfigValue::Bool(false),
            "窗口失去焦点时是否继续推进剧情和声音；游戏内尚未保存设置时作为初始值。",
        ),
        MusicaField::new(
            "text_shadow",
            "文字阴影",
            "显示",
            ConfigKind::Bool,
            ConfigValue::Bool(true),
            "是否为正文文字绘制阴影；游戏内尚未保存设置时作为初始值。",
        ),
        MusicaField::new(
            "logical_width",
            "逻辑画布宽度",
            "渲染",
            ConfigKind::Integer { min: 1, max: 16384 },
            ConfigValue::Integer(1280),
            "游戏脚本使用的坐标宽度，导入 eden 原版存档时须为 1024；未指定渲染分辨率时按此尺寸渲染。",
        )
        .flags(ADVANCED)
        .step(1.0, "px"),
        MusicaField::new(
            "logical_height",
            "逻辑画布高度",
            "渲染",
            ConfigKind::Integer { min: 1, max: 16384 },
            ConfigValue::Integer(720),
            "游戏脚本使用的坐标高度，导入 eden 原版存档时须为 640；未指定渲染分辨率时按此尺寸渲染。",
        )
        .flags(ADVANCED)
        .step(1.0, "px"),
        MusicaField::choice(
            "script_encoding",
            "脚本编码",
            "脚本",
            &[
                ("shift_jis", "Shift-JIS（日文原版）"),
                ("gbk", "GBK（简体中文）"),
            ],
            "shift_jis",
            "解码脚本文本所用的编码，须与游戏版本一致。",
        ),
    ];
    fields.extend(crate::voice_preferences::VoicePreferences::fields());
    fields.extend(crate::audio::AudioPreferences::fields());
    fields
}
pub(crate) fn musica_descriptor() -> RuntimeDescriptor {
    RuntimeDescriptor {
        family_id: FAMILY_ID.into(),
        plugin_id: PLUGIN_ID.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            RuntimeCapability::CpuFrame,
            RuntimeCapability::PcmAudio,
            RuntimeCapability::NativeSave,
            RuntimeCapability::TextReplacement,
        ],
        supported_formats: SUPPORTED_FORMATS
            .iter()
            .map(|format| (*format).into())
            .collect(),
        configuration: musica_fields()
            .into_iter()
            .map(|field| field.field)
            .collect(),
    }
}
#[cfg(test)]
pub(crate) struct MusicaOpen {
    pub session: Box<MusicaSession>,
}
#[cfg(test)]
impl MusicaProvider {
    pub(crate) fn probe(&self, request: ProbeRequest) -> SessionResult<Option<ProbeReport>> {
        request.validate()?;
        let root = Path::new(request.game_path.as_str());
        if !shinki_emu_musica_keys::REQUIRED_ARCHIVE_ROLES
            .iter()
            .all(|r| root.join(format!("{r}.paz")).is_file())
        {
            return Ok(None);
        }
        let report = ProbeReport {
            family_id: "musica".into(),
            game_id: root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("musica")
                .into(),
            format: "musica.paz".into(),
            confidence_permyriad: 8000,
        };
        report.validate()?;
        Ok(Some(report))
    }
    pub(crate) fn open(&mut self, request: OpenRequest) -> SessionResult<MusicaOpen> {
        let (_, session) = self.open_session(request)?;
        Ok(MusicaOpen {
            session: Box::new(session),
        })
    }
}
impl MusicaProvider {
    #[cfg(test)]
    pub(crate) fn with_gpu_instance(instance: wgpu::Instance) -> Self {
        Self {
            gpu_instance: Some(instance),
        }
    }

    #[cfg(test)]
    pub(crate) fn open_session(
        &mut self,
        request: OpenRequest,
    ) -> SessionResult<(OpenResponse, MusicaSession)> {
        self.open_native_session(request, shinki_emu_abi::Extent::default(), None)
    }

    /// `render_extent` is already checked against the declared render support;
    /// 0x0 renders at the logical canvas size.
    pub(crate) fn open_native_session(
        &mut self,
        request: OpenRequest,
        render_extent: shinki_emu_abi::Extent,
        replacement: Option<crate::native::text::TextBridge>,
    ) -> SessionResult<(OpenResponse, MusicaSession)> {
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let descriptor = musica_descriptor();
        descriptor.validate()?;
        request.validate()?;
        let config = resolve_config(&descriptor.configuration, &request.configuration)?;
        let get = |key: &str| -> SessionResult<String> {
            match &config
                .iter()
                .find(|e| e.id == key)
                .ok_or_else(|| error("SHINKI_EMU_MUSICA_CONFIG", "configuration is missing"))?
                .value
            {
                ConfigValue::String(s) => Ok(s.to_string()),
                _ => Err(error(
                    "SHINKI_EMU_MUSICA_CONFIG",
                    "configuration type is invalid",
                )),
            }
        };
        let profile = get("profile_file")?;
        let entry = get("entry_script")?;
        relative(&entry)?;
        if entry.contains('/') || entry.contains('\\') || !entry.ends_with(".sc") {
            return Err(error(
                "SHINKI_EMU_MUSICA_ENTRY",
                "entry script must name one .sc file in the scr archive",
            ));
        }
        let audio_preferences = crate::audio::AudioPreferences::resolve(&config)?;
        let progress_in_background = match config
            .iter()
            .find(|entry| entry.id == "progress_in_background")
            .map(|entry| &entry.value)
        {
            Some(ConfigValue::Bool(value)) => *value,
            _ => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_CONFIG",
                    "background playback preference is missing or invalid",
                ));
            }
        };
        let text_shadow = match config
            .iter()
            .find(|e| e.id == "text_shadow")
            .map(|e| &e.value)
        {
            Some(ConfigValue::Bool(value)) => *value,
            _ => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_CONFIG",
                    "text shadow preference is missing or invalid",
                ));
            }
        };
        let logical_extent = |key: &str| -> SessionResult<u32> {
            let value = config
                .iter()
                .find(|entry| entry.id == key)
                .map(|entry| &entry.value)
                .ok_or_else(|| {
                    error(
                        "SHINKI_EMU_MUSICA_RENDER_DIMENSIONS",
                        "logical dimension is missing",
                    )
                })?;
            let ConfigValue::Integer(value) = value else {
                return Err(error(
                    "SHINKI_EMU_MUSICA_RENDER_DIMENSIONS",
                    "logical dimension must be a positive integer",
                ));
            };
            u32::try_from(*value).map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_RENDER_DIMENSIONS",
                    "logical dimension is outside the supported integer range",
                )
            })
        };
        let logical_width = logical_extent("logical_width")?;
        let logical_height = logical_extent("logical_height")?;
        let (raster_width, raster_height) = if render_extent == shinki_emu_abi::Extent::default() {
            (logical_width, logical_height)
        } else {
            (render_extent.width, render_extent.height)
        };
        let encoding = match config
            .iter()
            .find(|e| e.id == "script_encoding")
            .map(|e| &e.value)
        {
            Some(ConfigValue::Enum(value)) if value == "shift_jis" => ScriptEncoding::ShiftJis,
            Some(ConfigValue::Enum(value)) if value == "gbk" => ScriptEncoding::Gbk,
            _ => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_CONFIG",
                    "unsupported script encoding",
                ));
            }
        };
        let title_launch = match config
            .iter()
            .find(|e| e.id == "launch_mode")
            .map(|e| &e.value)
        {
            Some(ConfigValue::Enum(value)) if value == "title" => true,
            Some(ConfigValue::Enum(value)) if value == "direct" => false,
            _ => return Err(error("SHINKI_EMU_MUSICA_CONFIG", "invalid launch mode")),
        };
        let focused = request.initial_window.focused;
        let visible = request.initial_window.visible;
        let lease = SessionLease::acquire()?;
        let root = Path::new(request.game_path.as_str());
        let resources = crate::MusicaResourcePaths {
            scr_archive: get("scr_archive")?,
            sys_archive: get("sys_archive")?,
            script_directory: get("script_directory")?,
        };
        let (archive, texture_overrides) =
            mount_musica_with_resources(root, Path::new(&profile), &resources)
                .map_err(core_error)?;
        let configured_font =
            crate::resources::load_font(root, &get("font_path")?).map_err(core_error)?;
        let subtitle_catalog = crate::subtitles::SubtitleCatalog::load(root, &get("subtitle_map")?)
            .map_err(core_error)?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let archive = Arc::new(archive);
        let primary_encoding = encoding;
        let uri = format!("musica:/scr/{entry}");
        let loaded = crate::script_loader::load_script(&archive, &uri, primary_encoding)?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let encoding = loaded.script.encoding;
        let mut vm = MusicaVm::new(uri, loaded.hash, loaded.script, 0)
            .map_err(|_| error("SHINKI_EMU_MUSICA_VM", "entry script cannot be initialized"))?;
        vm.set_name_resources(archive.name_resources.clone());
        vm.set_voice_preferences(crate::voice_preferences::VoicePreferences::resolve(
            &config,
        )?);
        let auto_delay = config
            .iter()
            .find(|entry| entry.id == "message_speed_auto_play")
            .ok_or_else(|| error("SHINKI_EMU_MUSICA_CONFIG", "auto delay is missing"))?;
        let ConfigValue::Integer(auto_delay) = auto_delay.value else {
            return Err(error(
                "SHINKI_EMU_MUSICA_CONFIG",
                "auto delay type is invalid",
            ));
        };
        vm.set_auto_delay_units(
            u8::try_from(auto_delay)
                .map_err(|_| error("SHINKI_EMU_MUSICA_CONFIG", "auto delay is out of bounds"))?,
        )
        .map_err(|_| error("SHINKI_EMU_MUSICA_CONFIG", "auto delay is out of bounds"))?;
        let game = crate::resources::game_identity(
            &archive,
            configured_font.as_ref().map(|font| font.hash),
            subtitle_catalog.as_ref().map(|catalog| catalog.identity()),
        )
        .map_err(core_error)?;
        let mut native = crate::native_directory::NativeFiles::open(root, primary_encoding)?;
        native.set_name_resource_identity(
            archive.name_resources.as_ref().and_then(|names| names.resource_identity),
        )?;
        native.import_legacy_progress(root, game)?;
        native.import_legacy_settings(root, game)?;
        let mut settings = vm.config().clone();
        [
            settings.bgm_volume,
            settings.voice_volume,
            settings.se_volume,
        ] = audio_preferences.volume;
        [settings.bgm_muted, settings.voice_muted, settings.se_muted] = audio_preferences.muted;
        settings.progress_in_background = progress_in_background;
        settings.text_shadow = text_shadow;
        let settings = native.configuration(settings);
        let progress_in_background = settings.progress_in_background;
        let text_shadow = settings.text_shadow;
        let audio_preferences = settings.audio_preferences();
        vm.set_config(settings).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_CONFIG",
                "native configuration is invalid",
            )
        })?;
        vm.replace_global_variables(native.globals());
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        if self.gpu_instance.is_none() {
            self.gpu_instance = Some(roukaku_platform_common::native_wgpu_instance().map_err(
                |cause| {
                    error(
                        "SHINKI_EMU_MUSICA_RENDERER",
                        &format!(
                            "GPU instance creation failed ({:?} during {})",
                            cause.code, cause.operation
                        ),
                    )
                },
            )?);
        }
        let mut scene = Scene::new_scaled(
            self.gpu_instance
                .as_ref()
                .expect("GPU instance initialized"),
            archive.clone(),
            logical_width,
            logical_height,
            raster_width,
            raster_height,
            texture_overrides,
            encoding,
        )?;
        scene.set_configured_font(configured_font)?;
        #[cfg(feature = "ffmpeg-vcpkg")]
        scene.set_subtitle_catalog(subtitle_catalog);
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        scene.set_text_shadow(text_shadow);
        if title_launch {
            vm.begin_title_launch()
                .map_err(|_| error("SHINKI_EMU_MUSICA_TITLE", "title session cannot start"))?;
            scene.render_title(vm.title_variant(), None)?;
        } else {
            scene.render(vm.state(), None, None)?;
        }
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let audio = Audio::start(archive.clone(), request.audio_output)?;
        audio.set_preferences(audio_preferences)?;
        audio.suspend(!visible || (!focused && !progress_in_background))?;
        if let Some(service) = &replacement {
            service.check_cancelled()?;
        }
        let info = FrameInfo {
            width: raster_width,
            height: raster_height,
            logical_width,
            logical_height,
            stride: raster_width
                .checked_mul(4)
                .ok_or_else(|| error("SHINKI_EMU_MUSICA_FRAME_SIZE", "frame stride overflows"))?,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Ignored,
            },
            rows: FrameRowOrder::TopToBottom,
        };
        let response = OpenResponse {
            frame: info,
            audio_format: Some(crate::audio::FORMAT),
        };
        response.validate()?;
        let mut session = MusicaSession::new(
            info,
            request.session_epoch,
            archive,
            vm,
            scene,
            audio,
            replacement,
            lease,
            focused,
            visible,
            progress_in_background,
            primary_encoding,
            native,
        );
        session.sync_read_flags()?;
        tracing::info!(event = "shinki.emu.musica.session.open");
        Ok((response, session))
    }
}
fn relative(value: &str) -> SessionResult<()> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || Path::new(value)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        Err(error(
            "SHINKI_EMU_MUSICA_CONFIG_PATH",
            "configuration path must be a safe relative file",
        ))
    } else {
        Ok(())
    }
}
