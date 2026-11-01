//! Opt-in acceptance against private user-supplied input copies. No game starts.
use shinki_emu_patch::{PatchMode, PatchRequest, apply, apply_bound};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

fn copy_file(source: &Path, destination: &Path, relative: &Path) {
    let file = fs::canonicalize(source.join(relative)).unwrap();
    let root = fs::canonicalize(source).unwrap();
    assert!(
        file.starts_with(root),
        "private fixture input escapes its game root"
    );
    let target = destination.join(relative);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::copy(file, target).unwrap();
}
fn copy_inputs(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    let fixed = [
        "key.toml",
        "Sakura.hcb",
        "patch.dat",
        "patch.bin",
        "filter.dll",
        "cursor1.ani",
        "cursor2.ani",
        "eden_en.exe",
        "eden_cn.dll",
        "ef_first_AA.exe",
        "ef_first_cn_AA.dll",
        "夏空的英仙座.exe",
        "perseus_chs.mys",
        "yorino.exe",
        "name.acr",
    ];
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let name = entry.file_name().into_string().unwrap();
        let lower = name.to_ascii_lowercase();
        let paz = lower.ends_with(".paz")
            || lower.rsplit_once(".paz").is_some_and(|(_, part)| {
                part.len() == 1 && part.as_bytes()[0].is_ascii_alphabetic()
            });
        if fixed.contains(&name.as_str())
            || paz
            || lower.ends_with(".bin")
            || [".ttf", ".otf", ".ttc"]
                .iter()
                .any(|suffix| lower.ends_with(suffix))
        {
            copy_file(source, destination, Path::new(&name));
        }
    }
    for path in [
        "movie/01.wmv",
        "movie/02.wmv",
        "subtitle/vd1.ass",
        "subtitle/vd2.ass",
        "subtitle/ed1.ass",
        "subtitle/ed2.ass",
        "subtitle/fonts/AaGuiqi.ttf",
        "subtitle/fonts/HanaMinA.ttf",
        "subtitle/fonts/RyuminPro-Light.otf",
        "subtitle/fonts/Yu-MinCho-Light.ttf",
        "game/script_version.txt",
        "game/audio.rpa",
        "game/code.rpa",
        "game/font.rpa",
        "game/gui.rpa",
        "game/image.rpa",
        "game/movie.rpa",
        "game/patch.rpa",
    ] {
        if source.join(path).is_file() {
            copy_file(source, destination, Path::new(path));
        }
    }
    if source.join("key.toml").is_file() {
        let text = fs::read_to_string(source.join("key.toml")).unwrap();
        let key: toml::Value = toml::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        if let Some(overrides) = key.get("texture_overrides").and_then(toml::Value::as_table) {
            for value in overrides.values() {
                copy_file(source, destination, Path::new(value.as_str().unwrap()));
            }
        }
    }
}

#[test]
#[ignore = "requires SHINKI_PATCH_PRIVATE_INPUTS JSON mapping patch IDs to authorized private game roots"]
fn all_six_catalog_and_local_recipes_convert_game_copies() {
    let input =
        PathBuf::from(std::env::var_os("SHINKI_PATCH_PRIVATE_INPUTS").expect("private input map"));
    let inputs: BTreeMap<String, PathBuf> =
        serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
    let metadata = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Metadata");
    let fixture = tempfile::tempdir().unwrap();
    for id in [
        "fvp-sakura-cn",
        "musica-eden-cn",
        "musica-ef-first-cn",
        "musica-perseus-cn",
        "musica-yorino-cn",
        "renpy-himanatsu",
    ] {
        let entry: shinki_emu_metadata::PatchCatalogEntry = serde_json::from_slice(
            &fs::read(metadata.join(format!("patches/entries/{id}.json"))).unwrap(),
        )
        .unwrap();
        entry.validate().unwrap();
        let game = fixture.path().join(id).join("source");
        copy_inputs(&inputs[id], &game);
        for catalog in [false, true] {
            let output = fixture.path().join(id).join(if catalog {
                "catalog-output"
            } else {
                "local-output"
            });
            let request = PatchRequest {
                script: metadata.join("patches").join(&entry.recipe.script),
                source: game.clone(),
                patch: game.clone(),
                mode: PatchMode::NewDirectory {
                    output: output.clone(),
                },
            };
            let cancelled = Arc::new(AtomicBool::new(false));
            let result = if catalog {
                apply_bound(
                    request,
                    &entry.releases,
                    Some(&entry.downloads),
                    cancelled,
                    |_| {},
                )
            } else {
                apply(request, cancelled, |_| {})
            }
            .unwrap_or_else(|error| {
                panic!(
                    "{id} {}: {error}",
                    if catalog { "catalog" } else { "local" }
                )
            });
            assert_eq!(result.profile.plugin_id, entry.emulators[0]);
            assert!(
                !result
                    .profile
                    .configuration
                    .contains_key("translation_table")
            );
            assert!(
                !result
                    .profile
                    .configuration
                    .contains_key("translation_mode")
            );
            assert!(result.output.join("shinki-patch-result.json").is_file());
            assert!(!result.output.join("save").exists());
            assert!(!result.output.join("savedata").exists());
            println!(
                "Private copied input converted: {id}, source={}, release={}",
                if catalog { "catalog" } else { "local" },
                entry.releases[0].release_id
            );
            fs::remove_dir_all(output).unwrap();
        }
    }
}
