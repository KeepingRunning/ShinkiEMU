use super::*;
use crate::{MusicaVm, ScOpcodeCatalog, mount_musica, parse_sc};
use roukaku_core::Hash256;
use shinki_emu_musica_keys::MUSICA_KEY_FILE;

#[test]
#[ignore = "requires a hardware GPU"]
fn native_stage_gpu_restore_preserves_position_and_rejects_invalid_sequences() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.png -4 2 * 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    vm.step(1).unwrap();
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive.clone(),
        32,
        32,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let pixel = |x: usize, y: usize| &scene.pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4];
    assert_eq!(pixel(0, 0), &[0, 0, 0, 255]);
    assert_ne!(pixel(0, 2), &[0, 0, 0, 255]);
    assert_eq!(pixel(12, 2), &[0, 0, 0, 255]);
    let expected = scene.pixels.clone();
    let mut restored_vm = make_vm();
    restored_vm
        .install_test_state(&MusicaVm::checked_test_state(vm.state()).unwrap(), 1)
        .unwrap();
    let mut restored_scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        32,
        32,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    restored_scene
        .render(restored_vm.state(), None, None)
        .unwrap();
    assert_eq!(restored_scene.pixels, expected);
    let mut unsupported = restored_vm.state().clone();
    unsupported.stage.as_mut().unwrap().foreground.main = Some("musica:/bg/Light_ov.png".into());
    assert_eq!(
        restored_scene
            .render(&unsupported, None, None)
            .unwrap_err()
            .code(),
        "SHINKI_EMU_MUSICA_STAGE_BLEND"
    );
    assert_eq!(restored_scene.pixels, expected);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn dual_stage_screen_prefix_orders_layers_and_restores_without_partial_frames() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage Light_SC.PNG:BG.png 4 2 BG.png 0 0 Stand.png 8,0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let png = |width, height, color| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    crate::test_fixture::stand(root.path(), &png(8, 8, [64, 128, 192, 255]));
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[
            ("BG.png", &png(16, 16, [25, 100, 220, 255])),
            ("Light_SC.PNG", &png(16, 16, [128, 0, 0, 128])),
        ],
    );
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let make_scene = || {
        Scene::new(
            crate::test_fixture::gpu_instance(),
            archive.clone(),
            16,
            16,
            crate::ScriptEncoding::ShiftJis,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    vm.step(1).unwrap();
    let mut scene = make_scene();
    scene.render(vm.state(), None, None).unwrap();
    let pixel = |scene: &Scene, x: usize, y: usize| {
        <[u8; 4]>::try_from(&scene.pixels[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]).unwrap()
    };
    assert_eq!(pixel(&scene, 0, 0), [83, 100, 220, 255]);
    assert_eq!(pixel(&scene, 5, 10), [83, 100, 220, 255]);
    let mut restored_vm = make_vm();
    restored_vm
        .install_test_state(&MusicaVm::checked_test_state(vm.state()).unwrap(), 1)
        .unwrap();
    let mut restored = make_scene();
    restored.render(restored_vm.state(), None, None).unwrap();
    assert_eq!(restored.pixels, scene.pixels);
    let mut state = vm.state().clone();
    state.stage.as_mut().unwrap().foreground.main = None;
    scene.render(&state, None, None).unwrap();
    assert_eq!(pixel(&scene, 5, 10), [112, 128, 192, 255]);
    let valid = scene.pixels.clone();
    state.stage.as_mut().unwrap().foreground.prefix = Some("musica:/bg/Light_ov.png".into());
    assert_eq!(
        scene.render(&state, None, None).unwrap_err().code(),
        "SHINKI_EMU_MUSICA_STAGE_BLEND"
    );
    assert_eq!(scene.pixels, valid);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn panel_modes_draw_at_native_positions_and_clear_after_restore() {
    let root = tempfile::tempdir().unwrap();
    let source = b".panel 1 * custom.png\r\n.panel 3\r\n.panel 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let png = |color| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 80, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    crate::test_fixture::assets(
        root.path(),
        "sys",
        &[
            ("custom.png", &png([255, 0, 0, 255])),
            ("fullPanel.png", &png([0, 255, 0, 255])),
        ],
    );
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        80,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    let mut vm = make_vm();
    for tick in 1..=3 {
        vm.step(tick).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        let expected = scene.pixels.clone();
        let top = &expected[..4];
        let bottom = &expected[79 * 16 * 4..79 * 16 * 4 + 4];
        match tick {
            1 => {
                assert_eq!(top, [0, 0, 0, 255]);
                assert_eq!(bottom, [255, 0, 0, 255]);
            }
            2 => {
                assert_eq!(top, [0, 255, 0, 255]);
                assert_eq!(bottom, top);
            }
            3 => {
                assert_eq!(top, [0, 0, 0, 255]);
                assert_eq!(bottom, top);
            }
            _ => unreachable!(),
        }
        let mut restored = make_vm();
        restored
            .install_test_state(&MusicaVm::checked_test_state(vm.state()).unwrap(), 1)
            .unwrap();
        scene.render(restored.state(), None, None).unwrap();
        assert_eq!(scene.pixels, expected);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn screen_shake_gpu_preserves_composed_clipping_and_restores_mid_interval() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.png 0 0 * 0 0\r\n.shakescreen V 4 30\r\n.transition 0 * 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        16,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    vm.step(1).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let original = scene.pixels.clone();
    vm.step(2).unwrap();
    vm.advance_screen_shake_clock(30_000_000).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let shifted = scene.pixels.clone();
    for y in 0..16 {
        for x in 0..16 {
            let index = (y * 16 + x) * 4;
            let expected = if y < 12 {
                &original[((y + 4) * 16 + x) * 4..((y + 4) * 16 + x) * 4 + 4]
            } else {
                &[0, 0, 0, 255]
            };
            assert_eq!(&shifted[index..index + 4], expected);
        }
    }
    vm.advance_screen_shake_clock(29_000_000).unwrap();
    let mut restored = make_vm();
    restored
        .install_test_state(&MusicaVm::checked_test_state(vm.state()).unwrap(), 3)
        .unwrap();
    vm.advance_screen_shake_clock(1_000_000).unwrap();
    restored.advance_screen_shake_clock(1_000_000).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let expected = scene.pixels.clone();
    scene.render(restored.state(), None, None).unwrap();
    assert_eq!(scene.pixels, expected);
    vm.step(3).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    assert_eq!(scene.pixels, original);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn crossfade_transition_and_sqz_animated_foreground_render_native_layers() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage * BLACK.png 0 0\r\n.transition 0 * 10\r\n.stage *:*,*,loop.png 0 0 BLACK.png 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    use std::io::Write as _;
    let png = |color| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    let frame = |color: [u8; 3]| {
        let mut bgra = Vec::with_capacity(16 * 16 * 4);
        for _ in 0..16 * 16 {
            bgra.extend_from_slice(&[color[2], color[1], color[0], 255]);
        }
        let mut compressed = Vec::new();
        flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::default())
            .write_all(&bgra)
            .unwrap();
        compressed
    };
    let red = frame([255, 0, 0]);
    let blue = frame([0, 0, 255]);
    let frames_start = 0x14 + 2 * 8;
    let mut sqz = Vec::new();
    sqz.extend_from_slice(b"SQZ1");
    sqz.extend_from_slice(&32u32.to_le_bytes());
    sqz.extend_from_slice(&16u32.to_le_bytes());
    sqz.extend_from_slice(&16u32.to_le_bytes());
    sqz.extend_from_slice(&1u32.to_le_bytes());
    sqz.extend_from_slice(&(frames_start as u32).to_le_bytes());
    sqz.extend_from_slice(&(red.len() as u32).to_le_bytes());
    sqz.extend_from_slice(&((frames_start + red.len()) as u32).to_le_bytes());
    sqz.extend_from_slice(&(blue.len() as u32).to_le_bytes());
    sqz.extend_from_slice(&red);
    sqz.extend_from_slice(&blue);
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[
            ("BLACK.png", &png([0, 0, 0, 255])),
            ("WHITE.png", &png([255, 255, 255, 255])),
            ("loop.png", &png([255, 0, 0, 255])),
            ("loop.sqz", &sqz),
        ],
    );
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut machine = make_vm();
    machine.step(1).unwrap();
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        16,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    let pixel = |scene: &Scene, x: usize, y: usize| {
        <[u8; 4]>::try_from(&scene.pixels[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]).unwrap()
    };
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [0, 0, 0, 255]);
    // Second step submits the comma-slot stage under an armed transition.
    machine.step(2).unwrap();
    let duration_ns = machine
        .state()
        .transition_active
        .as_ref()
        .unwrap()
        .duration_ns;
    assert_eq!(duration_ns, 25_600_000_000 / 10);
    let pair_count = crate::MusicaSqzArchive::header_pair_count(&sqz).unwrap();
    let mut animations = std::collections::BTreeMap::new();
    animations.insert(
        "musica:/bg/loop.png".to_owned(),
        crate::MusicaImageAnimationState {
            frame_index: 0,
            frame_count: pair_count * 2,
            interval_ns: 1_000_000_000 / u64::from(pair_count),
            elapsed_ns: 0,
        },
    );
    machine.attach_image_animations(animations).unwrap();
    // At progress zero the replaced stage stays fully opaque.
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [0, 0, 0, 255]);
    // Half-way: the animated red frame fades in over the old black stage.
    assert!(machine.advance_transition_clock(duration_ns / 2).unwrap());
    scene.render(machine.state(), None, None).unwrap();
    let halfway = pixel(&scene, 8, 8);
    // Half-intensity red is 127.5 in RGBA8; accept either adjacent integer.
    assert!(
        (127..=128).contains(&halfway[0]),
        "unexpected half-way pixel: {halfway:?}"
    );
    assert_eq!(&halfway[1..], &[0, 0, 255]);
    // Completed: the new stage renders fully, animated frame 0 is opaque red.
    assert!(!machine.advance_transition_clock(duration_ns).unwrap());
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [255, 0, 0, 255]);
    // One native interval later the SQZ advances to the blue frame and loops.
    assert!(
        machine
            .advance_image_animation_clock(1_000_000_000)
            .unwrap()
    );
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [0, 0, 255, 255]);
    assert!(
        machine
            .advance_image_animation_clock(1_000_000_000)
            .unwrap()
    );
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [255, 0, 0, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn masked_transition_reveals_new_stage_through_the_rule_image() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage * BLACK.png 0 0\r\n.transition 1 rule.png 10\r\n.stage * WHITE.png 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 255, 255, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let white = png.into_inner();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 0, 0, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let black = png.into_inner();
    // Rule image: the left half is dark (reveals first after the native
    // load-time inversion), the right half is bright.
    let mut rule = image::RgbaImage::new(16, 16);
    for (x, _, pixel) in rule.enumerate_pixels_mut() {
        *pixel = image::Rgba(if x < 8 {
            [0, 0, 0, 255]
        } else {
            [255, 255, 255, 255]
        });
    }
    let mut png = std::io::Cursor::new(Vec::new());
    rule.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let rule = png.into_inner();
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[("BLACK.png", &black), ("WHITE.png", &white)],
    );
    crate::test_fixture::asset(root.path(), "sys", "rule.png", &rule);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let mut machine = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    machine.step(1).unwrap();
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        16,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    let pixel = |scene: &Scene, x: usize, y: usize| {
        <[u8; 4]>::try_from(&scene.pixels[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]).unwrap()
    };
    machine.step(2).unwrap();
    let duration_ns = machine
        .state()
        .transition_active
        .as_ref()
        .unwrap()
        .duration_ns;
    // Phase zero: nothing revealed, the old black stage shows.
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 4, 8), [0, 0, 0, 255]);
    assert_eq!(pixel(&scene, 12, 8), [0, 0, 0, 255]);
    // Halfway (phase 256): the inverted-bright left half is fully white,
    // the right half still black.
    assert!(machine.advance_transition_clock(duration_ns / 2).unwrap());
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 4, 8), [255, 255, 255, 255]);
    assert_eq!(pixel(&scene, 12, 8), [0, 0, 0, 255]);
    // Completed: the new stage renders normally.
    assert!(!machine.advance_transition_clock(duration_ns).unwrap());
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 12, 8), [255, 255, 255, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn ani_patch_selection_draws_matching_eye_and_mouth_rows() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage * BASE.png 0 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 0, 0, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let base = png.into_inner();
    // Synthetic ANI: three eye rows and three mouth rows for one character.
    // The patch decoder only reads metadata names; pixel data comes from the
    // container's raw frames, so build them with matching dimensions.
    let labels: [(&str, [u8; 4]); 6] = [
        ("恋 開き目", [255, 0, 0, 255]),
        ("恋 中目", [0, 255, 0, 255]),
        ("恋 閉じ目", [0, 0, 255, 255]),
        ("恋 閉じ口", [255, 255, 0, 255]),
        ("恋 中口", [0, 255, 255, 255]),
        ("恋 開き口", [255, 0, 255, 255]),
    ];
    let encoded: Vec<(Vec<u8>, [u8; 4])> = labels
        .iter()
        .map(|(label, color)| (encoding_rs::SHIFT_JIS.encode(label).0.into_owned(), *color))
        .collect();
    let frame_bytes = 16 * 16 * 4;
    let mut ani = Vec::new();
    ani.extend_from_slice(&0x0100u16.to_le_bytes());
    ani.extend_from_slice(&(encoded.len() as u16).to_le_bytes());
    ani.extend_from_slice(&0u32.to_le_bytes());
    // The container interleaves each frame's name, 10-byte header and raw
    // pixels; record the pixel start per frame while appending.
    let mut offsets = Vec::new();
    for (bytes, _) in &encoded {
        ani.extend_from_slice(bytes);
        ani.push(0);
        ani.extend_from_slice(&16u16.to_le_bytes());
        ani.extend_from_slice(&16u16.to_le_bytes());
        ani.extend_from_slice(&32u16.to_le_bytes());
        ani.extend_from_slice(&(4i16).to_le_bytes());
        ani.extend_from_slice(&(4i16).to_le_bytes());
        offsets.push(ani.len());
        ani.extend_from_slice(&vec![0u8; frame_bytes]);
    }
    for (index, (_, color)) in encoded.iter().enumerate() {
        let start = offsets[index];
        for pixel in ani[start..start + frame_bytes].as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[color[2], color[1], color[0], 255]);
        }
    }
    crate::test_fixture::assets(
        root.path(),
        "bg",
        &[("BASE.png", &base), ("BASE.ani", &ani)],
    );
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let mut machine = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    machine.step(1).unwrap();
    let mut patches = std::collections::BTreeMap::new();
    patches.insert(
        "musica:/bg/BASE.png".to_owned(),
        (vec!["恋".to_owned()], 0x2545F4914F6CDD1Du64),
    );
    machine.attach_ani_patches(patches).unwrap();
    // Eye opens at (0,10): arm and display 開き目 (red patch at 4,4).
    machine
        .advance_ani_patch_clock(10_000_000, Some("恋"), None)
        .unwrap();
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        16,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    let pixel = |scene: &Scene, x: usize, y: usize| {
        <[u8; 4]>::try_from(&scene.pixels[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]).unwrap()
    };
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [255, 0, 0, 255]);
    assert_eq!(pixel(&scene, 1, 1), [0, 0, 0, 255]);
    machine
        .advance_ani_patch_clock(10_000_000, Some("恋"), Some((true, 800)))
        .unwrap();
    scene.render(machine.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [0, 255, 255, 255]);
    let saved = MusicaVm::checked_test_state(machine.state()).unwrap();
    let mut restored = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    restored.install_test_state(&saved, 2).unwrap();
    scene.render(restored.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [0, 255, 255, 255]);
    for vm in [&mut machine, &mut restored] {
        vm.advance_ani_patch_clock(10_000_000, Some("恋"), Some((true, 900)))
            .unwrap();
    }
    scene.render(restored.state(), None, None).unwrap();
    assert_eq!(pixel(&scene, 8, 8), [255, 0, 255, 255]);
    assert_eq!(restored.state(), machine.state());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn scrollxf_gpu_crops_translates_and_accepts_empty_extent() {
    let root = tempfile::tempdir().unwrap();
    let source=b".stage * BG.png 0 0\r\n.scrollxf 16 16 8 8 0 0 8 8 100 0\r\n.scrollxf 0 0 0 0 0 0 0 0 100 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let mut vm = make_vm();
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        16,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    vm.step(1).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let original = scene.pixels.clone();
    vm.step(2).unwrap();
    vm.advance_scroll_xf_clock(50_000_000).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    let cropped = scene.pixels.clone();
    for y in 0..16 {
        for x in 0..16 {
            let index = (y * 16 + x) * 4;
            let expected = if x < 12 && y < 12 {
                &original[((y + 4) * 16 + x + 4) * 4..((y + 4) * 16 + x + 4) * 4 + 4]
            } else {
                &[0, 0, 0, 255]
            };
            assert_eq!(&cropped[index..index + 4], expected);
        }
    }
    let mut restored = make_vm();
    restored
        .install_test_state(&MusicaVm::checked_test_state(vm.state()).unwrap(), 3)
        .unwrap();
    scene.render(restored.state(), None, None).unwrap();
    assert_eq!(scene.pixels, cropped);
    vm.step(3).unwrap();
    scene.render(vm.state(), None, None).unwrap();
    assert!(
        scene
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel == &[0, 0, 0, 255])
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_gpu_scaled_ani_offset_scroll_clip_and_shake_preserve_logical_geometry() {
    let root = tempfile::tempdir().unwrap();
    let source = b".stage BG.ani 0 0 * 0 0\r\n.scrollxf 16 16 12 12 0 0 2 1 100 0\r\n.shakescreen V 2 30\r\n.transition 0 * 0\r\n.end\r\n";
    crate::test_fixture::game(root.path(), source);

    // The test-only ANI contains four opaque quadrants and a non-zero native
    // origin. It is deliberately small and synthetic; the Scene loader and
    // archive path remain the same as a native family session.
    let mut ani = vec![0x00, 0x01, 0x01, 0x00, 0, 0, 0, 0];
    ani.extend_from_slice(b"scaled\0");
    ani.extend_from_slice(&16u16.to_le_bytes());
    ani.extend_from_slice(&16u16.to_le_bytes());
    ani.extend_from_slice(&32u16.to_le_bytes());
    ani.extend_from_slice(&2i16.to_le_bytes());
    ani.extend_from_slice(&1i16.to_le_bytes());
    for y in 0..16u16 {
        for x in 0..16u16 {
            let rgba = match (x < 8, y < 8) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255, 255, 0, 255],
            };
            // ANI stores BGRA while the Scene renderer consumes RGBA.
            ani.extend_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
        }
    }
    crate::test_fixture::asset(root.path(), "bg", "BG.ani", &ani);

    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let make_vm = || {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    };
    let scales = [(16u32, 1.0f32), (24, 1.5), (32, 2.0), (48, 3.0)];
    let mut baseline_by_scale = Vec::new();
    let mut transformed_by_scale = Vec::new();

    for (raster_size, scale) in scales {
        let mut vm = make_vm();
        vm.step(1).unwrap();
        let mut scene = Scene::new_scaled(
            crate::test_fixture::gpu_instance(),
            archive.clone(),
            16,
            16,
            raster_size,
            raster_size,
            std::collections::BTreeMap::new(),
            crate::ScriptEncoding::ShiftJis,
        )
        .unwrap();
        assert_ne!(scene.renderer.identity().device_type, "cpu");
        scene.render(vm.state(), None, None).unwrap();
        let baseline = scene.pixels.clone();

        // The ANI origin is [2, 1], so the first red quadrant begins at that
        // logical point while the preceding logical border remains clear.
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 2, 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 1, 1),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 12, 1),
            [0, 255, 0, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 2, 9),
            [0, 0, 255, 255]
        );
        assert_eq!(
            sample_logical(&baseline, raster_size, scale, 12, 9),
            [255, 255, 0, 255]
        );

        vm.step(2).unwrap();
        vm.advance_scroll_xf_clock(50_000_000).unwrap();
        let mut cropped_state = vm.state().clone();
        cropped_state.screen_shake = None;
        scene.render(&cropped_state, None, None).unwrap();
        let cropped = scene.pixels.clone();
        // The half-way scroll moves the source by one logical pixel and clips
        // to a 14x14 visible extent. This checks source crop and clip through
        // the actual Musica Scene::render path.
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 2, 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 13, 1),
            [0, 255, 0, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 14, 1),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 2, 13),
            [0, 0, 255, 255]
        );
        assert_eq!(
            sample_logical(&cropped, raster_size, scale, 2, 14),
            [0, 0, 0, 255]
        );

        vm.step(3).unwrap();
        vm.advance_screen_shake_clock(30_000_000).unwrap();
        scene.render(vm.state(), None, None).unwrap();
        let transformed = scene.pixels.clone();
        // Vertical shake is -2 logical pixels on its first update. The
        // compositor's outer clip must keep the shifted frame inside the
        // logical viewport while preserving the inner scroll clip.
        assert_eq!(
            sample_logical(&transformed, raster_size, scale, 2, 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&transformed, raster_size, scale, 2, 13),
            [0, 0, 0, 255]
        );
        assert_eq!(
            sample_logical(&transformed, raster_size, scale, 14, 1),
            [0, 0, 0, 255]
        );

        baseline_by_scale.push((raster_size, baseline));
        transformed_by_scale.push((raster_size, transformed));
    }

    // The same logical scene must agree at every supported output density;
    // compare representative interior logical pixels rather than raster
    // bytes so the assertion covers the coordinate contract.
    let baseline_reference = &baseline_by_scale[0].1;
    let transformed_reference = &transformed_by_scale[0].1;
    for ((raster_size, baseline), (_, transformed)) in
        baseline_by_scale.iter().zip(&transformed_by_scale)
    {
        let scale = *raster_size as f32 / 16.0;
        for (x, y) in [(2, 1), (12, 1), (2, 9), (12, 9), (2, 13)] {
            assert_eq!(
                sample_logical(baseline, *raster_size, scale, x, y),
                sample_logical(baseline_reference, 16, 1.0, x, y)
            );
            assert_eq!(
                sample_logical(transformed, *raster_size, scale, x, y),
                sample_logical(transformed_reference, 16, 1.0, x, y)
            );
        }
    }
}

fn sample_logical(pixels: &[u8], raster_size: u32, scale: f32, x: u32, y: u32) -> [u8; 4] {
    let raster_x = (((x as f32) + 0.5) * scale).floor() as u32;
    let raster_y = (((y as f32) + 0.5) * scale).floor() as u32;
    let offset = ((raster_y * raster_size + raster_x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

#[test]
#[ignore = "requires a hardware GPU"]
fn moving_shared_asset_reuses_gpu_pixels_and_reappears_after_release() {
    let root = tempfile::tempdir().unwrap();
    crate::test_fixture::game(root.path(), b".end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(512, 512, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    crate::test_fixture::assets(root.path(), "bg", &[("BG.png", png.get_ref())]);
    let archive =
        Arc::new(mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE)).unwrap());
    let mut scene = Scene::new(
        crate::test_fixture::gpu_instance(),
        archive,
        16,
        16,
        crate::ScriptEncoding::ShiftJis,
    )
    .unwrap();
    for x in 0..8 {
        let mut commands = Vec::new();
        scene
            .layer(&mut commands, "musica:/bg/BG.png", x, 0, 1.0)
            .unwrap();
        // Two logical draws share the same immutable native asset.
        scene
            .layer(&mut commands, "musica:/bg/BG.png", x + 1, 0, 1.0)
            .unwrap();
        scene.submit(commands).unwrap();
        for column in 0..16 {
            assert_eq!(
                &scene.pixels[(column * 4) as usize..(column * 4 + 4) as usize],
                if column < x {
                    &[0, 0, 0, 255]
                } else {
                    &[255, 0, 0, 255]
                }
            );
        }
        if x > 0 {
            // Counters include vertex uploads. Re-uploading this 1 MiB asset
            // cannot fit below its decoded size, even after atlas packing.
            assert!(scene.renderer.performance_counters().upload_bytes < 512 * 512 * 4);
        }
    }
    scene.submit(Vec::new()).unwrap();
    assert!(
        scene
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255])
    );
    let mut commands = Vec::new();
    scene
        .layer(&mut commands, "musica:/bg/BG.png", 0, 0, 1.0)
        .unwrap();
    scene.submit(commands).unwrap();
    assert!(
        scene
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255])
    );
}
