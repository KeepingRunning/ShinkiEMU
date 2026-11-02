mod configuration;
pub use configuration::*;
mod audio;
mod movie;
mod native_directory;
mod provider;
mod scene;
mod session;
mod storage;
mod voice_preferences;

mod archive;
mod embed;
mod image_container;
mod localization;
pub use embed::embed_sc_localization;
mod message;
mod name_resources;
pub use name_resources::MusicaNameResources;
mod subtitles;
pub use localization::{MusicaTranslationTable, musica_display_cstr_hash, musica_display_hash};
mod paz;
mod profile;
mod resources;
pub use resources::MusicaResourcePaths;
mod runtime;
pub use message::*;
mod script;
mod script_loader;
pub use script_loader::{ScriptValidation, read_expanded_script, validate_musica_script};
mod text_renderer;

pub use archive::*;
pub use image_container::*;
pub use paz::*;
pub use profile::*;
pub use runtime::*;
pub use script::*;
pub use shinki_emu_sdk::CoreError;
pub use text_renderer::MusicaTextRenderer;

pub const MUSICA_READER_ID: &str = "shinki.emu.musica.paz.v1";
pub const MUSICA_DECRYPT_PROVIDER_ID: &str = "shinki.emu.musica.paz.decrypt.v1";
pub const MUSICA_DECRYPT_DESCRIPTOR_SCHEMA: &str = "shinki.emu.musica.paz.decrypt_descriptor.v1";
pub const MUSICA_SCRIPT_IR_SCHEMA: &str = "shinki.emu.musica.script_ir.v3";

mod native;
pub use native::musica_query;

#[cfg(test)]
pub(crate) use provider::{MusicaProvider, musica_descriptor};

#[cfg(test)]
mod test_fixture;

pub mod eden_save;
pub mod minori_data;
