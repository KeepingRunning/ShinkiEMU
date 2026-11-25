use std::{path::PathBuf, rc::Rc};

use shinki_emu_manager_ui_slint::{
    App, ManagerViewModel, NoticeViewModel, Notices, SlintManagerAdapter, describe_error, i18n,
    report_error,
};
use slint::{
    ComponentHandle, LogicalSize, Model, PhysicalSize, Rgb8Pixel,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

struct SoftwarePlatform(Rc<MinimalSoftwareWindow>);

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn render(window: &MinimalSoftwareWindow, (width, height): (u32, u32)) -> image::RgbImage {
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width as usize);
    }));
    image::RgbImage::from_raw(
        width,
        height,
        pixels
            .iter()
            .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
            .collect(),
    )
    .unwrap()
}

#[test]
fn inventory_cause_renders_with_three_host_notices_and_clears_on_repair() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform(software.clone()))).unwrap();
    let adapter = SlintManagerAdapter::new().unwrap();
    let window = adapter.window();
    window.show().unwrap();
    let rows = window.global::<Notices>().get_items();
    let host_errors = [
        "SHINKI_EMU_NETWORK_PERMISSION_REQUIRED",
        "SHINKI_EMU_FAMILY_LOAD_LIBRARY",
        "SHINKI_EMU_CORE_INSTALL_COPY",
    ];
    let raw = "SHINKI_EMU_CORE_PENDING_APPLY: not a directory";

    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).unwrap();
    }
    let (languages, sizes): (&[&str], &[(u32, u32)]) = if output.is_some() {
        (
            &["zh-CN", "en", "ja"],
            &[(1280, 800), (360, 780), (640, 360)],
        )
    } else {
        (&["en"], &[(640, 360)])
    };
    for &language in languages {
        let mut model = ManagerViewModel::default();
        model.app.appearance.language = language.into();
        model.app.appearance.reduce_motion = true;
        model.resale_notice.acknowledged = true;
        model.library.prepare_state = "ready".into();
        i18n::apply(language);
        adapter.apply(&model);
        window.global::<App>().set_page("cores".into());
        for raw in host_errors {
            report_error(window, raw);
        }
        let text = describe_error(raw);
        for &(width, height) in sizes {
            let size = (width, height);
            software.set_size(PhysicalSize::new(width, height));
            software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
            software.dispatch_event(WindowEvent::Resized {
                size: LogicalSize::new(width as f32, height as f32),
            });
            let repaired = render(&software, size);
            model.notices.core_inventory_error = Some(NoticeViewModel {
                id: "core-inventory:fixture".into(),
                severity: "error".into(),
                title: text.title.clone(),
                detail: text.detail.clone(),
                code: text.code.clone(),
                ..NoticeViewModel::default()
            });
            adapter.apply(&model);
            let without_cause = render(&software, size);
            model.notices.core_inventory_error.as_mut().unwrap().detail =
                format!("{}\n{raw}", text.detail);
            adapter.apply(&model);
            assert_eq!(rows.row_count(), 4);
            assert_eq!(rows.row_data(2).unwrap().code.as_str(), text.code);
            let blocked = render(&software, size);
            assert!(blocked != without_cause, "the raw cause drew no pixels");

            model.notices.core_inventory_error = None;
            adapter.apply(&model);
            assert_eq!(rows.row_count(), host_errors.len());
            for (row, code) in rows.iter().zip(host_errors.into_iter().rev()) {
                assert_eq!(row.code.as_str(), code);
            }
            assert!(
                repaired == render(&software, size),
                "repair left stale pixels"
            );
            if let Some(dir) = &output {
                for (state, frame) in [("blocked", blocked), ("repaired", repaired)] {
                    frame
                        .save(dir.join(format!("cores-{state}-{language}-{width}x{height}.png")))
                        .unwrap();
                }
            }
        }
    }
}
