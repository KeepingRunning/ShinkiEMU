use super::*;
use shinki_emu_manager_ui_slint::ChoiceViewModel;

impl ShinkiEmuManagerController {
    pub(super) fn engine_label(&self, family_id: &str) -> String {
        self.registry
            .descriptors()
            .chain(
                self.probe_choices
                    .values()
                    .flatten()
                    .map(|choice| &choice.descriptor),
            )
            .find(|descriptor| descriptor.family_id == family_id)
            .map_or_else(|| family_id.to_owned(), |descriptor| descriptor.family_name.clone())
    }

    pub(super) fn emulator_label(&self, descriptor: &FamilyPluginDescriptor) -> String {
        let (version, identity) = self.core_release(descriptor);
        let version = identity.map_or_else(|| version.to_owned(), shinki_emu_metadata::CoreIdentity::display_version);
        format!("{} · {}", descriptor.display_name, version)
    }

    pub(super) fn appearance_view(&self) -> AppearanceViewModel {
        AppearanceViewModel {
            theme_mode: self.appearance.theme_mode.clone(),
            density: self.appearance.density.clone(),
            cover_size: self.appearance.cover_size.clone(),
            view_mode: self.appearance.view_mode.clone(),
            reduce_motion: self.appearance.reduce_motion,
            cover_badges: self.appearance.cover_badges,
            language: self.appearance.language.clone(),
        }
    }

    /// Emulator picker for the library detail: empty unless there is a real choice.
    pub(super) fn emulator_choices(&self, game_id: &str) -> Vec<ChoiceViewModel> {
        match self.probe_selection_for_game(game_id) {
            Some(choices) if choices.len() > 1 => choices
                .iter()
                .map(|choice| ChoiceViewModel {
                    id: choice.report.plugin_id.clone(),
                    label: self.emulator_label(&choice.descriptor),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

pub(super) fn binding_views(
    mapping: &shinki_emu_manager_core::InputMapping,
) -> Vec<GamepadBindingViewModel> {
    GamepadInput::DISPLAY_ORDER
        .iter()
        .map(|button| GamepadBindingViewModel {
            id: button.as_str().into(),
            label: button.label().into(),
            key: mapping.gamepad.get(button).cloned().unwrap_or_default(),
        })
        .collect()
}
