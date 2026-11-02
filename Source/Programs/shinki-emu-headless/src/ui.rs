//! Capture the real Manager Slint component with its software renderer, without
//! loading Manager storage, account services, native cores, or an OS window.
use crate::{Error, Result};
use clap::{Args, ValueEnum};
use serde::Deserialize;
use shinki_emu_manager_ui_slint as ui;
use slint::{
    ComponentHandle,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};
use std::{
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Page {
    Library,
    Cores,
    Patches,
    Settings,
}
impl Page {
    fn id(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Cores => "cores",
            Self::Patches => "patches",
            Self::Settings => "settings",
        }
    }
    fn ui_page(self) -> &'static str {
        if matches!(self, Self::Patches) {
            "patch"
        } else {
            self.id()
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Theme {
    Dark,
    Light,
}
#[derive(Debug, Clone, Args)]
pub struct Arguments {
    #[arg(long, value_enum)]
    pub page: Page,
    #[arg(long, default_value = "zh-CN", value_parser = ["zh-CN", "zh-cn", "en", "ja"])]
    pub language: String,
    #[arg(long, value_enum, default_value = "dark")]
    pub theme: Theme,
    #[arg(long, default_value_t = 1600)]
    pub width: u32,
    #[arg(long, default_value_t = 1000)]
    pub height: u32,
    #[arg(long)]
    pub output: PathBuf,
    /// Optional synthetic dataset JSON. No Manager database is opened.
    #[arg(long)]
    pub demo_data: Option<PathBuf>,
    #[arg(long, default_value = "appearance", value_parser = ["appearance", "library-roots", "network", "translation", "input", "updates", "about"])]
    pub settings_category: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoData {
    games: Vec<DemoGame>,
    #[serde(default)]
    roots: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoGame {
    title: String,
    description: String,
}
impl Default for DemoData {
    fn default() -> Self {
        Self {
            games: vec![
                DemoGame {
                    title: "ShinkiEMU UI Sample".into(),
                    description: "Synthetic interface example · no game assets".into(),
                },
                DemoGame {
                    title: "The Paper Observatory".into(),
                    description: "Self-authored demo entry · no game executable".into(),
                },
                DemoGame {
                    title: "A Quiet Afternoon".into(),
                    description: "Self-authored demo entry · no game executable".into(),
                },
            ],
            roots: vec!["C:/ShinkiEMU/Demo".into()],
        }
    }
}

struct SoftwarePlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}
impl Platform for SoftwarePlatform {
    fn create_window_adapter(
        &self,
    ) -> std::result::Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

pub fn run_cli(args: Arguments) -> std::process::ExitCode {
    match run(args) {
        Ok(capture) => {
            println!(
                "{}",
                serde_json::to_string(&capture).expect("serializable capture")
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(error.exit_code())
        }
    }
}

pub fn run(args: Arguments) -> Result<crate::capture::Capture> {
    if !(360..=8192).contains(&args.width) || !(360..=8192).contains(&args.height) {
        return Err(Error::Run(
            "UI dimensions must be in 360..=8192 at DPR 1".into(),
        ));
    }
    let data = args
        .demo_data
        .as_ref()
        .map(|path| -> Result<DemoData> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
        .transpose()?
        .unwrap_or_default();
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
    }))
    .map_err(|error| Error::Run(error.to_string()))?;
    let adapter = ui::SlintManagerAdapter::new().map_err(|error| Error::Run(error.to_string()))?;
    let language = if args.language == "zh-cn" {
        "zh-CN"
    } else {
        &args.language
    };
    ui::i18n::apply(language);
    let mut model = ui::ManagerViewModel::default();
    model.app.version = env!("CARGO_PKG_VERSION").into();
    model.app.build = "Headless UI demo · synthetic data".into();
    model.app.appearance.language = language.into();
    model.app.appearance.theme_mode = match args.theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    }
    .into();
    model.app.appearance.reduce_motion = true;
    model.resale_notice.acknowledged = true;
    model.library.prepare_state = "ready".into();
    model.settings.roots = data.roots;
    for (index, game) in data.games.into_iter().enumerate() {
        let id = format!("demo-{index}");
        model.library.games.push(ui::GameCardViewModel {
            id: id.clone(),
            title: game.title.clone(),
            subtitle: game.description.clone(),
            ready: false,
            problem: "UI demo".into(),
            ..Default::default()
        });
        model.patch.games.push(ui::PatchGameViewModel {
            id,
            title: game.title,
            subtitle: game.description,
            linked: false,
            ..Default::default()
        });
    }
    for (id, name, engine) in [
        ("rfvp", "RFVP", "FVP"),
        ("siglus_rs", "Siglus", "SiglusEngine"),
        ("shinki.emu.musica", "Musica", "Musica"),
        ("shinki.emu.cmvs", "CMVS", "CMVS"),
    ] {
        model.cores.items.push(ui::CoreViewModel {
            id: id.into(),
            name: name.into(),
            engine: engine.into(),
            state: "available".into(),
            source: "online".into(),
            message: "Synthetic catalog preview".into(),
            ..Default::default()
        });
    }
    adapter.apply(&model);
    adapter
        .window()
        .global::<ui::App>()
        .set_page(args.page.ui_page().into());
    adapter
        .window()
        .global::<ui::App>()
        .set_settings_category(args.settings_category.into());
    let size = slint::PhysicalSize::new(args.width, args.height);
    software.set_size(size);
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(args.width as f32, args.height as f32),
    });
    adapter
        .window()
        .show()
        .map_err(|error| Error::Run(error.to_string()))?;
    let mut pixels =
        vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; args.width as usize * args.height as usize];
    // Instantiate lazy page components and settle property bindings before capture.
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, args.width as usize);
        });
    }
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        rgba.extend_from_slice(&[pixel.r, pixel.g, pixel.b, 255]);
    }
    let id = format!("{}-{}", args.page.id(), language);
    let capture = crate::capture::save_rgba(&rgba, args.width, args.height, &id, 0, &args.output)?;
    std::fs::write(
        args.output.join(format!("{id}.source.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "component":"shinki-emu-manager-ui-slint::ManagerWindow", "renderer":"Slint software", "dpr":1,
            "language":language,"theme":model.app.appearance.theme_mode,
            "data":if args.demo_data.is_some() { "Caller-supplied display-only dataset; no image paths, account or user library" } else { "Self-authored synthetic UI entries; no commercial assets, account or user library" },
            "license":if args.demo_data.is_some() { "Caller-supplied; not asserted by the runner" } else { "MIT OR Apache-2.0" },
        }))?,
    )?;
    adapter
        .window()
        .hide()
        .map_err(|error| Error::Run(error.to_string()))?;
    Ok(capture)
}
