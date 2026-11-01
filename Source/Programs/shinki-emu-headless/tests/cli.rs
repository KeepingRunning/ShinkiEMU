use std::process::Command;

#[test]
fn actual_png_comparison_controls_process_exit_and_reports_pixel_difference() {
    let temp = tempfile::tempdir().unwrap();
    let actual = temp.path().join("actual.png");
    let baseline = temp.path().join("baseline.png");
    image::save_buffer(&actual, &[20, 40, 60, 255], 1, 1, image::ColorType::Rgba8).unwrap();
    image::save_buffer(&baseline, &[22, 40, 60, 255], 1, 1, image::ColorType::Rgba8).unwrap();
    let compare = |tolerance: &str| {
        Command::new(env!("CARGO_BIN_EXE_shinki-emu-headless"))
            .arg("compare")
            .arg("--actual")
            .arg(&actual)
            .arg("--baseline")
            .arg(&baseline)
            .arg("--max-channel-delta")
            .arg(tolerance)
            .output()
            .unwrap()
    };
    let mismatch = compare("0");
    assert_eq!(mismatch.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    assert_eq!(report["changed_pixel_ratio"], 1.0);
    assert_eq!(report["max_channel_delta"], 2);
    let tolerant = compare("2");
    assert!(tolerant.status.success());
    image::save_buffer(&baseline, &[20, 40, 60, 255], 1, 1, image::ColorType::Rgba8).unwrap();
    assert!(compare("0").status.success());
}

#[test]
fn malformed_replay_fails_before_game_open_and_writes_failure_report() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("script.json");
    std::fs::write(&script,br#"[{"schema":"wrong","session":"game","sequence":1,"tick":0,"event":{"type":"advance_ticks","count":1}}]"#).unwrap();
    let output = temp.path().join("output");
    let game = temp.path().join("game");
    std::fs::create_dir(&game).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_shinki-emu-headless"))
        .args(["run", "--core", "nonexistent", "--game-dir"])
        .arg(&game)
        .arg("--output")
        .arg(&output)
        .arg("--script")
        .arg(script)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(!output.join("run.json").exists());
    let failure: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("failure.json")).unwrap()).unwrap();
    assert!(failure["error"].as_str().unwrap().contains("schema"));
}

#[test]
fn rejects_output_below_source_game_without_writing_anything() {
    let game = tempfile::tempdir().unwrap();
    let output = game.path().join("artifacts");
    let result = Command::new(env!("CARGO_BIN_EXE_shinki-emu-headless"))
        .args(["run", "--core", "nonexistent", "--game-dir"])
        .arg(game.path())
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(game.path()).unwrap().count(), 0);
}

#[cfg(feature = "manager-ui")]
#[test]
fn software_ui_renders_real_pages_translations_and_light_dark_themes() {
    let temp = tempfile::tempdir().unwrap();
    let render = |language: &str, theme: &str| {
        let output = temp.path().join(format!("{language}-{theme}"));
        let result = Command::new(env!("CARGO_BIN_EXE_shinki-emu-headless"))
            .args([
                "ui",
                "--page",
                "settings",
                "--language",
                language,
                "--theme",
                theme,
                "--width",
                "640",
                "--height",
                "480",
                "--output",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let image = image::open(output.join(format!("settings-{language}.png")))
            .unwrap()
            .to_rgba8();
        assert_eq!(image.dimensions(), (640, 480));
        image
    };
    let english = render("en", "light");
    let japanese = render("ja", "light");
    let dark = render("en", "dark");
    assert_ne!(english.as_raw(), japanese.as_raw());
    let light_sum: u64 = english
        .pixels()
        .map(|pixel| pixel.0[..3].iter().map(|v| u64::from(*v)).sum::<u64>())
        .sum();
    let dark_sum: u64 = dark
        .pixels()
        .map(|pixel| pixel.0[..3].iter().map(|v| u64::from(*v)).sum::<u64>())
        .sum();
    assert!(light_sum > dark_sum);
}
