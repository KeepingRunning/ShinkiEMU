use crate::{Error, Result};
use roukaku_headless_protocol::{ButtonState, InputMessage, ObservationPredicate, PhysicalInput};
use shinki_emu_manager_core::model::*;
use std::path::Path;

pub(crate) fn load(path: Option<&Path>) -> Result<Vec<InputMessage>> {
    let Some(path) = path else {
        return Ok(vec![]);
    };
    parse(&std::fs::read(path)?)
}
fn parse(bytes: &[u8]) -> Result<Vec<InputMessage>> {
    let messages: Vec<InputMessage> = serde_json::from_slice(bytes)?;
    let mut sequence = 0;
    let mut tick = 0;
    let mut session = None;
    let mut checkpoints = std::collections::BTreeSet::new();
    for message in &messages {
        message
            .validate()
            .map_err(|error| Error::Run(error.to_string()))?;
        if message.sequence <= sequence
            || message.tick < tick
            || session.is_some_and(|id| id != &message.session)
        {
            return Err(Error::Run("script sequence must increase, ticks must not decrease, and session must be constant".into()));
        }
        sequence = message.sequence;
        tick = message.tick;
        session = Some(&message.session);
        match &message.event {
            PhysicalInput::Checkpoint { id } => {
                if id.contains('/') || id.starts_with("frame-") || !checkpoints.insert(id) {
                    return Err(Error::Run(
                        "checkpoint IDs must be unique filenames and cannot start with frame-"
                            .into(),
                    ));
                }
            }
            PhysicalInput::Await {
                observation,
                continue_at_match,
                ..
            } => {
                validate_observation(observation)?;
                if *continue_at_match {
                    return Err(Error::Run(
                        "continue_at_match is unavailable for native-owned game clocks".into(),
                    ));
                }
            }
            PhysicalInput::Resume
            | PhysicalInput::Focus { .. }
            | PhysicalInput::Keyboard { .. }
            | PhysicalInput::ImeCommit { .. }
            | PhysicalInput::PointerMove { .. }
            | PhysicalInput::PointerButton { .. }
            | PhysicalInput::Wheel { .. }
            | PhysicalInput::AdvanceTicks { .. } => {}
            _ => {
                return Err(Error::Run(
                    "this physical input is not supported by the native ABI headless adapter"
                        .into(),
                ));
            }
        }
    }
    Ok(messages)
}
fn validate_observation(observation: &ObservationPredicate) -> Result<()> {
    match observation {
        ObservationPredicate::Exists { key } if key == "frame" => Ok(()),
        ObservationPredicate::Equals { key, .. } if key == "frame.sha256" => Ok(()),
        _ => Err(Error::Run(
            "native observations support Exists(frame) and Equals(frame.sha256)".into(),
        )),
    }
}
pub(crate) fn end_tick(messages: &[InputMessage]) -> Result<u64> {
    let mut tick = 0u64;
    for message in messages {
        tick = tick.max(message.tick);
        if let PhysicalInput::AdvanceTicks { count } = message.event {
            tick = tick
                .checked_add(u64::from(count))
                .ok_or_else(|| Error::Run("script timeline overflow".into()))?;
        }
    }
    tick.checked_mul(roukaku_headless_protocol::TICK_DURATION_NS)
        .ok_or_else(|| Error::Run("script timeline overflow".into()))?;
    Ok(tick)
}
pub(crate) fn matches(frame: &FrameLease, observation: &ObservationPredicate) -> Result<bool> {
    validate_observation(observation)?;
    match observation {
        ObservationPredicate::Exists { .. } => Ok(true),
        ObservationPredicate::Equals { value_hash, .. } => {
            Ok(crate::capture::pixel_hash(frame)? == *value_hash)
        }
    }
}
fn state(value: ButtonState, repeat: bool) -> KeyState {
    match (value, repeat) {
        (ButtonState::Released, _) => KeyState::Released,
        (_, true) => KeyState::Repeated,
        _ => KeyState::Pressed,
    }
}
fn key(name: &str) -> Result<KeyCode> {
    use KeyCode::*;
    const KEYS: &[KeyCode] = &[
        Unknown,
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
        I,
        J,
        K,
        L,
        M,
        N,
        O,
        P,
        Q,
        R,
        S,
        T,
        U,
        V,
        W,
        X,
        Y,
        Z,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Escape,
        Enter,
        Tab,
        Backspace,
        Space,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowLeft,
        ArrowRight,
        ArrowUp,
        ArrowDown,
        ControlLeft,
        ControlRight,
        ShiftLeft,
        ShiftRight,
        AltLeft,
        AltRight,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadEnter,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        Minus,
        Equals,
        Comma,
        Period,
        Slash,
        Semicolon,
        Apostrophe,
        BracketLeft,
        BracketRight,
        Backslash,
        Grave,
    ];
    KEYS.iter()
        .copied()
        .find(|key| key.physical_name() == name && *key != Unknown)
        .ok_or_else(|| Error::Run(format!("unsupported physical key {name}")))
}
pub(crate) fn events(event: &PhysicalInput, frame: &FrameLease) -> Result<Vec<FamilyEvent>> {
    Ok(match event {
        PhysicalInput::Resume => vec![FamilyEvent::WindowFocused { focused: true }],
        PhysicalInput::Focus { focused } => vec![FamilyEvent::WindowFocused { focused: *focused }],
        PhysicalInput::Keyboard {
            physical_key,
            logical_key,
            state: value,
            repeat,
        } => vec![FamilyEvent::Key {
            code: key(physical_key)?,
            state: state(*value, *repeat),
            modifiers: KeyModifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
            logical: logical_key.clone(),
        }],
        PhysicalInput::ImeCommit { text } => vec![FamilyEvent::TextInput { text: text.clone() }],
        PhysicalInput::PointerMove { x, y } => vec![FamilyEvent::PointerMove {
            x: f32::from(*x) * frame.info.logical_width.saturating_sub(1) as f32 / 65535.0,
            y: f32::from(*y) * frame.info.logical_height.saturating_sub(1) as f32 / 65535.0,
            displayed: Some(frame.identity),
        }],
        PhysicalInput::PointerButton {
            button,
            state: value,
        } => {
            use roukaku_headless_protocol::PointerButton as P;
            let button = match button {
                P::Primary => PointerButton::Primary,
                P::Secondary => PointerButton::Secondary,
                P::Middle => PointerButton::Middle,
                P::Back => PointerButton::Back,
                P::Forward => PointerButton::Forward,
                P::Other => {
                    return Err(Error::Run(
                        "native ABI does not support Other pointer button".into(),
                    ));
                }
            };
            vec![FamilyEvent::PointerButton {
                button,
                state: state(*value, false),
                displayed: Some(frame.identity),
            }]
        }
        PhysicalInput::Wheel { delta_x, delta_y } => vec![FamilyEvent::Wheel {
            delta_x: *delta_x as f32,
            delta_y: *delta_y as f32,
        }],
        _ => return Err(Error::Run("unsupported physical input".into())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(sequence: u64, tick: u64, event: PhysicalInput) -> InputMessage {
        InputMessage {
            schema: roukaku_headless_protocol::USER_INPUT_SEQUENCE_SCHEMA.into(),
            session: "game".into(),
            sequence,
            tick,
            event,
        }
    }
    #[test]
    fn parses_replay_and_rejects_out_of_order_unknown_and_duplicate_steps() {
        let messages = vec![
            input(1, 0, PhysicalInput::AdvanceTicks { count: 2 }),
            input(2, 2, PhysicalInput::Checkpoint { id: "title".into() }),
        ];
        assert_eq!(
            parse(&serde_json::to_vec(&messages).unwrap())
                .unwrap()
                .len(),
            2
        );
        let mut invalid = messages.clone();
        invalid[1].sequence = 1;
        assert!(parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        invalid = messages.clone();
        invalid[1].schema = "unknown".into();
        assert!(parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        invalid = vec![
            input(1, 0, PhysicalInput::Checkpoint { id: "title".into() }),
            input(2, 1, PhysicalInput::Checkpoint { id: "title".into() }),
        ];
        assert!(parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        assert_eq!(end_tick(&messages).unwrap(), 2);
    }
}
