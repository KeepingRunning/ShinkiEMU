use crate::{CoreError, ScLineKind, ScOpcodeCatalog, ScriptEncoding};
use roukaku_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const RESOURCE: &str = "shinki-names.sc";
const MAX_BYTES: u64 = 16 * 1024;

/// The executable's two distinct name-window constants. A resource identity
/// distinguishes patched slots even when its literals equal the originals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaNameResources {
    pub(crate) unknown: String,
    pub(crate) history_unknown: String,
    pub(crate) resource_identity: Option<Hash256>,
}

fn invalid() -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_MUSICA_NAME_RESOURCE",
        "native name resource is malformed",
    )
}

impl MusicaNameResources {
    pub(crate) fn load(root: &Path, directory: &str) -> Result<Option<Self>, CoreError> {
        if !directory.is_empty() {
            let relative = format!("{directory}/{RESOURCE}");
            match std::fs::symlink_metadata(root.join(&relative)) {
                Ok(_) => {
                    return Self::parse(&crate::resources::read_resource(
                        root, &relative, MAX_BYTES,
                    )?)
                    .map(Some);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(invalid()),
            }
        }
        match std::fs::symlink_metadata(root.join("yorino.exe")) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(invalid()),
        }
        let path = crate::resources::resource_file(root, "yorino.exe")?;
        let mut exe = std::fs::File::open(path).map_err(|_| invalid())?;
        Self::from_exe(&mut exe).map(Some)
    }

    fn parse(bytes: &[u8]) -> Result<Self, CoreError> {
        let script = crate::parse_sc_with_encoding(
            bytes,
            &ScOpcodeCatalog::observed_musica(),
            ScriptEncoding::Gbk,
        )
        .map_err(|_| invalid())?;
        let mut unknown = None;
        let mut history_unknown = None;
        for line in &script.lines {
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            if command.opcode != "pragma" || line.language_guard.is_some() {
                return Err(invalid());
            }
            let tokens = command.tokens().map_err(|_| invalid())?;
            let [key, literal] = tokens.as_slice() else {
                return Err(invalid());
            };
            if !literal.starts_with("\\u{") {
                return Err(invalid());
            }
            let value = crate::embed::decode_sc_literal(literal)?;
            if value.is_empty() || value.contains(['\0', '\r', '\n']) {
                return Err(invalid());
            }
            let target = match key.as_str() {
                "shinki_name_unknown" => &mut unknown,
                "shinki_history_name_unknown" => &mut history_unknown,
                _ => return Err(invalid()),
            };
            if target.replace(value).is_some() {
                return Err(invalid());
            }
        }
        Ok(Self {
            unknown: unknown.ok_or_else(invalid)?,
            history_unknown: history_unknown.ok_or_else(invalid)?,
            resource_identity: Some(Hash256::from_sha256(bytes)),
        })
    }

    fn from_exe(exe: &mut (impl Read + Seek)) -> Result<Self, CoreError> {
        fn constant<const N: usize>(
            exe: &mut (impl Read + Seek),
            offset: u64,
        ) -> Result<String, CoreError> {
            let mut bytes = [0; N];
            exe.seek(SeekFrom::Start(offset)).map_err(|_| invalid())?;
            exe.read_exact(&mut bytes).map_err(|_| invalid())?;
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(invalid)?;
            let value = ScriptEncoding::ShiftJis
                .decode(&bytes[..end])
                .ok_or_else(invalid)?;
            if value.is_empty() {
                return Err(invalid());
            }
            Ok(value.into_owned())
        }
        Ok(Self {
            unknown: constant::<7>(exe, 0x112098)?,
            history_unknown: constant::<11>(exe, 0x121f18)?,
            resource_identity: None,
        })
    }

    /// Yorino's GetNameType is based solely on the authored first byte.
    pub(crate) fn live_name(
        &self,
        token: &str,
        encoding: ScriptEncoding,
    ) -> Result<Option<String>, crate::MusicaRuntimeError> {
        let mut token = crate::embed::decode_sc_literal(token)
            .map_err(|_| crate::MusicaRuntimeError::Operand)?;
        match token.as_bytes().first() {
            None | Some(b'#') => Ok(None),
            Some(b'@') => Ok(Some(self.unknown.clone())),
            _ => {
                if let Some(at) = token.rfind('=') {
                    token.drain(..=at);
                }
                let encoded = encoding
                    .encode(&token)
                    .ok_or(crate::MusicaRuntimeError::Operand)?;
                if encoded.len() > 6 {
                    // Native rendering takes six source bytes, not six characters.
                    // Keep the largest complete code-page prefix within that limit.
                    let name = (0..=6)
                        .rev()
                        .find_map(|end| encoding.decode(&encoded[..end]))
                        .ok_or(crate::MusicaRuntimeError::Operand)?;
                    return Ok((!name.is_empty()).then(|| name.into_owned()));
                }
                drop(encoded);
                Ok((!token.is_empty()).then_some(token))
            }
        }
    }

    pub(crate) fn history_name(
        &self,
        token: &str,
        encoding: ScriptEncoding,
    ) -> Result<Option<String>, crate::MusicaRuntimeError> {
        let token = crate::embed::decode_sc_literal(token)
            .map_err(|_| crate::MusicaRuntimeError::Operand)?;
        match token.as_bytes().first() {
            None => Ok(None),
            Some(b'@' | b'#') => Ok(Some(self.history_unknown.clone())),
            _ => {
                // Use the script code page, not the executable's CP932 bytes.
                let mut name = String::with_capacity(token.len() + 6);
                name.push('【');
                name.push_str(&token);
                name.push('】');
                encoding
                    .encode(&name)
                    .ok_or(crate::MusicaRuntimeError::Operand)?;
                Ok(Some(name))
            }
        }
    }
}

#[cfg(test)]
mod tests;
