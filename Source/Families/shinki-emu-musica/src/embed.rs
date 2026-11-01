use crate::{
    CoreError, MusicaTranslationTable, ScLineKind, ScOpcodeCatalog, ScriptEncoding,
    parse_sc_with_encoding,
};
use std::fmt::Write;

/// Rewrites display operands in native SC data. Control flow, voice operands,
/// language guards and untranslated lines retain their original bytes.
/// Translation lookup happens here, never in a playback callback.
pub fn embed_sc_localization(
    bytes: &[u8],
    table: &MusicaTranslationTable,
    primary: ScriptEncoding,
) -> Result<(Vec<u8>, usize), CoreError> {
    if bytes.len() > crate::script_loader::MAX_SCRIPT_BYTES {
        return Err(invalid("source SC exceeds the native script bound"));
    }
    let encoding = ScriptEncoding::detect(bytes, primary);
    let script = parse_sc_with_encoding(bytes, &ScOpcodeCatalog::observed_musica(), encoding)
        .map_err(|_| invalid("source script cannot be parsed"))?;
    let mut output = Vec::with_capacity(bytes.len());
    let mut replaced = 0;
    for line in script.lines {
        let ScLineKind::Command { command } = &line.kind else {
            append(&mut output, &line.raw)?;
            continue;
        };
        let raw = &command.raw_operands;
        let replacement = if let Some((body_hash, speaker_hash)) = command.message_display_hashes()
        {
            let body = if table.has_dialogue() {
                table.lookup(body_hash)
            } else {
                None
            };
            let speaker = speaker_hash.and_then(|hash| table.lookup(hash));
            if body.is_none() && speaker.is_none() {
                None
            } else {
                let mut found = raw
                    .iter()
                    .enumerate()
                    .filter_map(|(at, byte)| matches!(byte, b' ' | b'\t').then_some(at));
                let mut separators = [0; 3];
                for separator in &mut separators {
                    *separator = found
                        .next()
                        .ok_or_else(|| invalid("message has no four-operand display endpoint"))?;
                }
                let mut operands = Vec::new();
                operands.extend_from_slice(&raw[..=separators[1]]);
                if let Some((text, _)) = speaker {
                    operands.extend_from_slice(
                        literal(text.strip_prefix(['@', '#']).unwrap_or(text)).as_bytes(),
                    );
                    replaced += 1;
                } else {
                    operands.extend_from_slice(&raw[separators[1] + 1..separators[2]]);
                }
                operands.push(raw[separators[2]]);
                if let Some((text, _)) = body {
                    let original = command
                        .tokens()
                        .map_err(|_| invalid("invalid message operands"))?;
                    let markup = crate::parse_musica_message_markup(&original[3..].join(" "))
                        .map_err(|_| invalid("invalid source message controls"))?;
                    operands.extend_from_slice(literal(text).as_bytes());
                    // Controls are already executed before the display callback in the old
                    // path. Keep their effects, without interpreting translation text.
                    for control in markup.controls {
                        use crate::MusicaMessageControl::*;
                        let encoded = match control {
                            WaitForVoice { .. } => "\\v".into(),
                            AutoAdvance { .. } => "\\a".into(),
                            LoadCharacter {
                                delay_ms,
                                slot_id,
                                resource,
                                transition_ms,
                                opacity_255,
                                ..
                            } => format!(
                                "\\x{{load,{delay_ms},{slot_id},{resource},{transition_ms},{opacity_255}}}"
                            ),
                        };
                        operands.extend_from_slice(encoded.as_bytes());
                    }
                    replaced += 1;
                } else {
                    operands.extend_from_slice(&raw[separators[2] + 1..]);
                }
                Some(operands)
            }
        } else if table.has_dialogue() && command.opcode == "select" {
            let mut changed = false;
            let mut operands = Vec::new();
            for token in raw.split_inclusive(|byte| matches!(byte, b' ' | b'\t')) {
                let end = token
                    .last()
                    .is_some_and(|byte| matches!(byte, b' ' | b'\t'));
                let value = &token[..token.len() - usize::from(end)];
                if let Some(colon) = value.iter().position(|byte| *byte == b':') {
                    let hash = crate::musica_display_cstr_hash(&value[..colon]);
                    if let Some((text, _)) = table.lookup(hash) {
                        operands.extend_from_slice(literal(text).as_bytes());
                        operands.extend_from_slice(&value[colon..]);
                        changed = true;
                        replaced += 1;
                    } else {
                        operands.extend_from_slice(value);
                    }
                } else {
                    operands.extend_from_slice(value);
                }
                if end {
                    operands.push(token[token.len() - 1]);
                }
            }
            changed.then_some(operands)
        } else {
            None
        };
        if let Some(operands) = replacement {
            let logical_end = line.raw.len()
                - if line.raw.ends_with(b"\r\n") {
                    2
                } else {
                    usize::from(line.raw.ends_with(b"\n"))
                };
            let start = logical_end
                .checked_sub(raw.len())
                .ok_or_else(|| invalid("source operand span is invalid"))?;
            if &line.raw[start..logical_end] != raw {
                return Err(invalid("source operand span does not match native parser"));
            }
            append(&mut output, &line.raw[..start])?;
            append(&mut output, &operands)?;
            append(&mut output, &line.raw[logical_end..])?;
        } else {
            append(&mut output, &line.raw)?;
        }
    }
    parse_sc_with_encoding(&output, &ScOpcodeCatalog::observed_musica(), encoding)
        .map_err(|_| invalid("embedded script cannot be parsed"))?;
    Ok((output, replaced))
}

fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), CoreError> {
    if output
        .len()
        .checked_add(bytes.len())
        .is_none_or(|size| size > crate::script_loader::MAX_SCRIPT_BYTES)
    {
        return Err(invalid("embedded SC exceeds the native script bound"));
    }
    output.extend_from_slice(bytes);
    Ok(())
}

/// ASCII scalar escapes let native SC carry literal Unicode, whitespace and
/// backslashes without changing its byte encoding or creating VM markup.
fn literal(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for character in text.chars() {
        write!(&mut encoded, "\\u{{{:x}}}", u32::from(character)).expect("writing to String");
    }
    encoded
}

pub(crate) fn decode_sc_literal(text: &str) -> Result<String, CoreError> {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("\\u{") {
        decoded.push_str(&rest[..start]);
        rest = &rest[start + 3..];
        let end = rest
            .find('}')
            .ok_or_else(|| invalid("truncated native scalar escape"))?;
        if end == 0 || end > 6 {
            return Err(invalid("invalid native scalar escape"));
        }
        let scalar = u32::from_str_radix(&rest[..end], 16)
            .ok()
            .and_then(char::from_u32)
            .filter(|character| *character != '\0')
            .ok_or_else(|| invalid("invalid native scalar escape"))?;
        decoded.push(scalar);
        rest = &rest[end + 1..];
    }
    decoded.push_str(rest);
    Ok(decoded)
}

fn invalid(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_MUSICA_NATIVE_LOCALIZATION", message)
}
