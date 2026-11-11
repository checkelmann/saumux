use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

pub fn key_name(key: KeyEvent) -> String {
    let mut name = String::new();
    for (modifier, label) in [
        (KeyModifiers::CONTROL, "Ctrl+"),
        (KeyModifiers::ALT, "Alt+"),
        (KeyModifiers::SHIFT, "Shift+"),
    ] {
        if key.modifiers.contains(modifier) {
            name.push_str(label);
        }
    }
    name.push_str(&match key.code {
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::PageUp => "PgUp".into(),
        KeyCode::PageDown => "PgDn".into(),
        other => format!("{other:?}"),
    });
    name
}

pub fn encode_key(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    match key.code {
        KeyCode::Char(c) => {
            if alt {
                out.push(27);
            }
            if ctrl && c.is_ascii() {
                let byte = c.to_ascii_uppercase() as u8;
                out.push(match byte {
                    b'@'..=b'_' => byte & 31,
                    b' ' | b'2' => 0,
                    b'?' => 127,
                    _ => byte,
                });
            } else {
                out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
        }
        KeyCode::Enter => {
            // A carriage return discards Shift/Control and can submit an agent
            // prompt. CSI-u preserves these keys; retain legacy Alt+Enter.
            if shift || ctrl {
                return format!("\x1b[13;{modifier}u").into_bytes();
            }
            if alt {
                out.push(27);
            }
            out.push(b'\r');
        }
        KeyCode::Tab => {
            if alt {
                out.push(27);
            }
            out.push(b'\t');
        }
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => {
            if alt {
                out.push(27);
            }
            out.push(127);
        }
        KeyCode::Esc => out.push(27),
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Right
        | KeyCode::Left
        | KeyCode::Home
        | KeyCode::End => {
            let final_byte = match key.code {
                KeyCode::Up => 'A',
                KeyCode::Down => 'B',
                KeyCode::Right => 'C',
                KeyCode::Left => 'D',
                KeyCode::Home => 'H',
                _ => 'F',
            };
            let text = if modifier > 1 {
                format!("\x1b[1;{modifier}{final_byte}")
            } else {
                format!(
                    "\x1b{}{final_byte}",
                    if application_cursor { 'O' } else { '[' }
                )
            };
            out.extend_from_slice(text.as_bytes());
        }
        KeyCode::F(n @ 1..=4) => {
            let c = (b'P' + n - 1) as char;
            out.extend_from_slice(
                if modifier == 1 {
                    format!("\x1bO{c}")
                } else {
                    format!("\x1b[1;{modifier}{c}")
                }
                .as_bytes(),
            );
        }
        code => {
            let number = match code {
                KeyCode::Insert => 2,
                KeyCode::Delete => 3,
                KeyCode::PageUp => 5,
                KeyCode::PageDown => 6,
                KeyCode::F(n @ 5..=12) => [15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize],
                _ => return out,
            };
            let text = if modifier == 1 {
                format!("\x1b[{number}~")
            } else {
                format!("\x1b[{number};{modifier}~")
            };
            out.extend_from_slice(text.as_bytes());
        }
    }
    out
}

pub fn encode_mouse(event: MouseEvent, x: u16, y: u16, screen: &vt100::Screen) -> Vec<u8> {
    use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};
    let mode = screen.mouse_protocol_mode();
    if mode == Mode::None {
        return vec![];
    }
    let button = |b| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (mut code, release) = match event.kind {
        MouseEventKind::Down(b) => (button(b), false),
        MouseEventKind::Up(b) if mode != Mode::Press => (button(b), true),
        MouseEventKind::Drag(b) if matches!(mode, Mode::ButtonMotion | Mode::AnyMotion) => {
            (button(b) + 32, false)
        }
        MouseEventKind::Moved if mode == Mode::AnyMotion => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        _ => return vec![],
    };
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }
    match screen.mouse_protocol_encoding() {
        Encoding::Sgr => format!(
            "\x1b[<{code};{};{}{}",
            x + 1,
            y + 1,
            if release { 'm' } else { 'M' }
        )
        .into_bytes(),
        Encoding::Utf8 => {
            let mut out = String::from("\x1b[M");
            for n in [
                if release { 3 } else { code } + 32,
                x as u32 + 33,
                y as u32 + 33,
            ] {
                out.push(char::from_u32(n).unwrap_or(' '));
            }
            out.into_bytes()
        }
        Encoding::Default => {
            if x > 222 || y > 222 {
                return vec![];
            }
            vec![
                27,
                b'[',
                b'M',
                if release { 35 } else { code as u8 + 32 },
                x as u8 + 33,
                y as u8 + 33,
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enter_preserves_newline_modifiers_without_changing_submit_or_alt_enter() {
        for (modifiers, expected) in [
            (KeyModifiers::NONE, "\r"),
            (KeyModifiers::SHIFT, "\x1b[13;2u"),
            (KeyModifiers::CONTROL, "\x1b[13;5u"),
            (KeyModifiers::CONTROL | KeyModifiers::SHIFT, "\x1b[13;6u"),
            (KeyModifiers::ALT, "\x1b\r"),
            (KeyModifiers::ALT | KeyModifiers::SHIFT, "\x1b[13;4u"),
        ] {
            assert_eq!(
                encode_key(KeyEvent::new(KeyCode::Enter, modifiers), false),
                expected.as_bytes()
            );
        }
        assert_eq!(
            encode_key(
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
                false
            ),
            b"\n"
        );
    }
    #[test]
    fn application_keys_and_modifiers() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true),
            b"\x1bOA"
        );
        assert_eq!(
            encode_key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            ),
            [3]
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::ALT), false),
            b"\x1b[21;3~"
        );
    }
    #[test]
    fn sgr_mouse_translates_coordinates_and_release() {
        let mut p = vt100::Parser::default();
        p.process(b"\x1b[?1002h\x1b[?1006h");
        let e = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 80,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(encode_mouse(e, 2, 3, p.screen()), b"\x1b[<0;3;4m");
    }
}
