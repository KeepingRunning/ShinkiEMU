use super::*;
use crate::{
    eden_save::{EdenEdition, EdenSave, EdenSaveEncoding},
    storage::{SAVE_MAX_SLOTS, SaveCard},
};

impl NativeDirectory {
    fn slot_stem(&self, slot: u32, append_playing: bool) -> SessionResult<String> {
        if slot >= SAVE_MAX_SLOTS {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_SLOT",
                "native save slot is out of range",
            ));
        }
        Ok(match self.title {
            MinoriTitle::EdenEnglish if append_playing => format!("edenPM{slot:04}"),
            MinoriTitle::EdenEnglish => format!("eden{slot:04}"),
            MinoriTitle::Perseus => format!("perseus_{slot:04}"),
        })
    }
    fn slot_save(
        &self,
        slot: u32,
        append_playing: bool,
        encoding: ScriptEncoding,
    ) -> SessionResult<Option<EdenSave>> {
        let stem = self.slot_stem(slot, append_playing)?;
        let Some(bytes) = read_optional(&self.slot_root.join(format!("{stem}.sav")))? else {
            return Ok(None);
        };
        let edition = EdenEdition::for_title(self.title);
        let encoding = EdenSaveEncoding::from_script(encoding);
        let save = EdenSave::decode(&bytes, edition, encoding).map_err(core_error)?;
        if self.title == MinoriTitle::Perseus
            && self.prefix.strict
            && save.prefix != self.prefix.value
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_PREFIX",
                "slot serial code does not match this game",
            ));
        }
        Ok(Some(save))
    }
}

impl NativeFiles {
    pub(crate) fn set_name_resource_identity(
        &mut self,
        identity: Option<Hash256>,
    ) -> SessionResult<()> {
        if let Some(dir) = self.dir.as_mut() {
            dir.slot_root = match identity {
                Some(identity) => dir.root.join("name-resources").join(identity.to_hex()),
                None => dir.root.clone(),
            };
            std::fs::create_dir_all(&dir.slot_root).map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_SAVE_PATH",
                    "native slot directory cannot be created",
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn edition(&self) -> Option<EdenEdition> {
        self.dir
            .as_ref()
            .map(|dir| EdenEdition::for_title(dir.title))
    }
    fn slot_dir(&self) -> SessionResult<&NativeDirectory> {
        self.dir.as_ref().ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_TITLE",
                "native slots require an identified original game executable",
            )
        })
    }
    pub(crate) fn slot(&self, slot: u32) -> SessionResult<Option<EdenSave>> {
        let dir = self.slot_dir()?;
        let append = self
            .globals()
            .get("AppendPlaying")
            .is_some_and(|value| *value != 0);
        dir.slot_save(slot, append, self.encoding)
    }
    pub(crate) fn card(&self, slot: u32) -> SessionResult<Option<SaveCard>> {
        let dir = self.slot_dir()?;
        let append = self
            .globals()
            .get("AppendPlaying")
            .is_some_and(|value| *value != 0);
        let Some(save) = dir.slot_save(slot, append, self.encoding)? else {
            return Ok(None);
        };
        let stem = dir.slot_stem(slot, append)?;
        let png = read_optional(&dir.slot_root.join(format!("{stem}.png")))?.ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_THUMBNAIL",
                "native slot thumbnail is missing",
            )
        })?;
        let mtime = std::fs::metadata(dir.slot_root.join(format!("{stem}.sav")))
            .and_then(|meta| meta.modified())
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_SAVE_READ",
                    "save modification time is unavailable",
                )
            })?;
        SaveCard::from_native(save.edition, save.comment, png, mtime).map(Some)
    }
    pub(crate) fn write_slot(
        &self,
        slot: u32,
        save: &EdenSave,
        thumbnail_png: &[u8],
    ) -> SessionResult<()> {
        let dir = self.slot_dir()?;
        if save.edition != EdenEdition::for_title(dir.title)
            || (dir.title == MinoriTitle::Perseus && save.prefix != dir.prefix.value)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_GAME",
                "slot edition or serial code differs from this game",
            ));
        }
        let stem = dir.slot_stem(
            slot,
            self.globals().get("AppendPlaying").is_some_and(|v| *v != 0),
        )?;
        // Do not replace a malformed existing native slot.
        dir.slot_save(
            slot,
            self.globals().get("AppendPlaying").is_some_and(|v| *v != 0),
            self.encoding,
        )?;
        let bytes = save.encode().map_err(core_error)?;
        // Native writer persists the bitmap first, then the compressed slot.
        replace_file(&dir.slot_root, &format!("{stem}.png"), thumbnail_png)?;
        replace_file(&dir.slot_root, &format!("{stem}.sav"), &bytes)
    }
    pub(crate) fn save_edition(&self) -> SessionResult<(EdenEdition, EdenSaveEncoding, u32)> {
        let dir = self.slot_dir()?;
        Ok((
            EdenEdition::for_title(dir.title),
            EdenSaveEncoding::from_script(self.encoding),
            dir.prefix.value,
        ))
    }
    pub(crate) fn is_perseus(&self) -> SessionResult<bool> {
        Ok(self.slot_dir()?.title == MinoriTitle::Perseus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patched_slot_namespaces_never_read_or_replace_unpatched_slots() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("eden_en.exe"), b"").unwrap();
        let source = b".stage front.png 0 0 back.png 0 0\n.panel 1\n.message 1  Ren Body\n.end\n";
        let mut vm = crate::MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc_with_encoding(
                source,
                &crate::ScOpcodeCatalog::observed_musica(),
                ScriptEncoding::Gbk,
            )
            .unwrap(),
            0,
        )
        .unwrap();
        while vm.state().message.is_none() {
            vm.step(vm.state().fixed_tick + 1).unwrap();
        }
        let checkpoint = vm.native_checkpoint().unwrap();
        let mut files = NativeFiles::open(root.path(), ScriptEncoding::Gbk).unwrap();
        let mut save = EdenSave::from_checkpoint(
            &checkpoint,
            EdenEdition::English,
            EdenSaveEncoding::Gbk,
            0,
            "original".into(),
        )
        .unwrap();
        files.write_slot(0, &save, b"thumbnail").unwrap();
        let patch = Hash256::from_sha256(b"names");
        files.set_name_resource_identity(Some(patch)).unwrap();
        assert!(files.slot(0).unwrap().is_none());
        save.comment = "patched".into();
        files.write_slot(0, &save, b"thumbnail").unwrap();
        assert_eq!(files.slot(0).unwrap().unwrap().comment, "patched");
        files
            .set_name_resource_identity(Some(Hash256::from_sha256(b"other names")))
            .unwrap();
        assert!(files.slot(0).unwrap().is_none());
        files.set_name_resource_identity(None).unwrap();
        assert_eq!(files.slot(0).unwrap().unwrap().comment, "original");
        files.set_name_resource_identity(Some(patch)).unwrap();
        assert_eq!(files.slot(0).unwrap().unwrap().comment, "patched");
    }
}
