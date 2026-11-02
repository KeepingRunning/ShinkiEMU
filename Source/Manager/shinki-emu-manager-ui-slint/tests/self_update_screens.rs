//! Renders the real `ManagerWindow` through the software renderer in every
//! self-update state and checks the pixels: each check status draws a
//! different 更新 page, the banner changes the library page, and applying the
//! same model again draws the same frame.
//!
//! Set `SHINKI_UI_SCREENSHOT_DIR` to keep PNGs of every state in zh-CN, en
//! and ja at 1280x800 and 360x780; without it zh-CN is rendered at 360x780
//! only. All text is synthetic.

use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use shinki_emu_manager_ui_slint::{
    App, ManagerViewModel, SelfUpdateViewModel, SlintManagerAdapter, i18n,
};
use slint::{
    ComponentHandle,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

struct SoftwarePlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

const NOTES: &str = "新增设置页“更新”分组与启动时的版本检查。\n修复窄屏下设置页按钮被截断的问题。\n改进日志目录的打开方式。";

/// (file id, page, settings category, state)
fn states() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    SelfUpdateViewModel,
)> {
    let base = SelfUpdateViewModel {
        current_version: "0.1.0-beta.7".into(),
        include_prerelease: true,
        status: "idle".into(),
        ..Default::default()
    };
    let available = SelfUpdateViewModel {
        status: "available".into(),
        latest_version: "0.1.0-beta.8".into(),
        notes_summary: NOTES.into(),
        download_url: "https://github.com/Project-Shinkirou/ShinkiEMU/releases".into(),
        last_checked: "2026-11-02 08:30 UTC".into(),
        ..base.clone()
    };
    vec![
        ("settings-idle", "settings", "updates", base.clone()),
        (
            "settings-checking",
            "settings",
            "updates",
            SelfUpdateViewModel {
                status: "checking".into(),
                ..base.clone()
            },
        ),
        (
            "settings-up-to-date",
            "settings",
            "updates",
            SelfUpdateViewModel {
                status: "up-to-date".into(),
                last_checked: "2026-11-02 08:30 UTC".into(),
                include_prerelease: false,
                ..base.clone()
            },
        ),
        (
            "settings-available",
            "settings",
            "updates",
            available.clone(),
        ),
        (
            "settings-ignored",
            "settings",
            "updates",
            SelfUpdateViewModel {
                ignored_version: "0.1.0-beta.8".into(),
                ..available.clone()
            },
        ),
        (
            "settings-failed",
            "settings",
            "updates",
            SelfUpdateViewModel {
                status: "failed".into(),
                failure_reason: i18n_rate_limit(),
                last_checked: "2026-11-02 08:30 UTC".into(),
                ..base.clone()
            },
        ),
        (
            "settings-offline",
            "settings",
            "updates",
            SelfUpdateViewModel {
                offline: true,
                ..base.clone()
            },
        ),
        (
            "banner-library",
            "library",
            "appearance",
            SelfUpdateViewModel {
                banner_visible: true,
                ..available.clone()
            },
        ),
        (
            "banner-settings",
            "settings",
            "network",
            SelfUpdateViewModel {
                banner_visible: true,
                ..available
            },
        ),
    ]
}

/// Backend-provided reason, rendered in the current language.
fn i18n_rate_limit() -> String {
    shinki_emu_manager_ui_slint::tr!("GitHub 请求已达速率限制，请稍后再试").to_owned()
}

fn render(software: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    software.set_size(slint::PhysicalSize::new(width, height));
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(width as f32, height as f32),
    });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; width as usize * height as usize];
    // Instantiate lazy page components, settle bindings and let finite
    // animations (the Material switch) finish before capture. Indeterminate
    // progress never stops, so it ends at the cap.
    for frame in 0..120 {
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
        if frame >= 2 && !software.window().has_active_animations() {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect()
}

/// Applies `state` on `page`/`category` and renders one frame.
fn show(
    adapter: &SlintManagerAdapter,
    software: &MinimalSoftwareWindow,
    language: &str,
    page: &str,
    category: &str,
    state: &SelfUpdateViewModel,
    (width, height): (u32, u32),
) -> Vec<u8> {
    let mut model = ManagerViewModel::default();
    model.app.version = state.current_version.clone();
    model.app.appearance.language = language.into();
    model.app.appearance.theme_mode = "dark".into();
    model.app.appearance.reduce_motion = true;
    model.resale_notice.acknowledged = true;
    model.library.prepare_state = "ready".into();
    model.network.allowed = !state.offline;
    model.self_update = state.clone();
    adapter.apply(&model);
    let app = adapter.window().global::<App>();
    app.set_page(page.into());
    app.set_settings_category(category.into());
    render(software, width, height)
}

#[test]
fn self_update_states_render_distinct_stable_frames() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
    }))
    .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");

    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    let (languages, sizes): (&[&str], &[(u32, u32)]) = if output.is_some() {
        (&["zh-CN", "en", "ja"], &[(1280, 800), (360, 780)])
    } else {
        (&["zh-CN"], &[(360, 780)])
    };
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).expect("screenshot dir");
    }
    adapter.window().show().expect("show");
    for &language in languages {
        i18n::apply(language);
        for &size in sizes {
            let (width, height) = size;
            let mut frames = HashMap::new();
            for (id, page, category, state) in states() {
                let rgba = show(&adapter, &software, language, page, category, &state, size);
                // Indeterminate progress animates, so only static states must repeat.
                if state.status != "checking" {
                    let again = show(&adapter, &software, language, page, category, &state, size);
                    assert!(
                        rgba == again,
                        "{id} {language} {width}x{height}: same model drew a different frame"
                    );
                }
                if id == "banner-library" {
                    let hidden = SelfUpdateViewModel {
                        banner_visible: false,
                        ..state
                    };
                    let plain = show(&adapter, &software, language, page, category, &hidden, size);
                    assert!(
                        rgba != plain,
                        "{language} {width}x{height}: banner did not change the library page"
                    );
                }
                if let Some(dir) = &output {
                    let path = dir.join(format!("{id}-{language}-{width}x{height}.png"));
                    image::RgbaImage::from_raw(width, height, rgba.clone())
                        .expect("frame size")
                        .save(&path)
                        .expect("write png");
                }
                frames.insert(id, rgba);
                // Leave the page so the next state starts from a fresh settings view.
                adapter.window().global::<App>().set_page("library".into());
            }
            let distinct = [
                "settings-idle",
                "settings-checking",
                "settings-up-to-date",
                "settings-available",
                "settings-ignored",
                "settings-failed",
                "settings-offline",
            ];
            for (i, a) in distinct.iter().enumerate() {
                for b in &distinct[i + 1..] {
                    assert!(
                        frames[a] != frames[b],
                        "{language} {width}x{height}: {a} and {b} drew the same frame"
                    );
                }
            }
        }
    }
    adapter.window().hide().expect("hide");
}
