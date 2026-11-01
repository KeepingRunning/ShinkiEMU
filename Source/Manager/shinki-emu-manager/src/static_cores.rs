//! The single inventory of Manager-embedded query entrypoints, shared by UI and headless hosts.
#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
pub const STATIC_CORES: &[(&str, shinki_emu_abi::Query)] = &[];
#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
pub const STATIC_CORES: &[(&str, shinki_emu_abi::Query)] = &[
    #[cfg(any(target_os = "android", target_os = "ios"))]
    ("rfvp", shinki_emu_rfvp::shinki_emu_query),
    #[cfg(any(target_os = "android", target_os = "ios"))]
    ("siglus_rs", shinki_emu_siglus_rs::query),
    ("shinki.emu.musica", shinki_emu_musica::musica_query),
    #[cfg(target_os = "android")]
    ("art3m1s-core", shinki_emu_art3m1s_core::artemis_query),
    #[cfg(target_os = "android")]
    ("impacto", shinki_emu_impacto::shinki_emu_query),
    #[cfg(target_os = "android")]
    ("renpy", shinki_emu_renpy::query),
    #[cfg(target_os = "android")]
    ("kirikiroid2-web", shinki_emu_kirikiroid2_web::query),
    #[cfg(target_os = "ios")]
    ("sena-rs", shinki_emu_sena_rs::shinki_emu_query),
    #[cfg(target_os = "ios")]
    ("ethornell-rs", shinki_emu_ethornell_rs::shinki_emu_query),
    #[cfg(target_os = "ios")]
    ("ReMinori", shinki_emu_ReMinori::shinki_emu_query),
];
