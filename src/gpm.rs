//! Linux virtual-console mouse input via gpm's local, native-endian protocol.
//! Wire layout: https://github.com/telmich/gpm/blob/master/src/headers/gpm.h
//! No libgpm linkage or system library is required.
#[cfg(target_os = "linux")]
mod linux {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    pub struct Gpm {
        stream: UnixStream,
        bytes: Vec<u8>,
    }
    impl Gpm {
        pub fn connect() -> Option<Self> {
            if std::env::var("TERM").ok().as_deref() != Some("linux") {
                return None;
            }
            let tty = std::fs::read_link("/proc/self/fd/0").ok()?;
            let vc = tty
                .to_str()?
                .strip_prefix("/dev/tty")?
                .parse::<i32>()
                .ok()?;
            if !(1..=63).contains(&vc) {
                return None;
            }
            let mut stream = UnixStream::connect("/dev/gpmctl").ok()?;
            let mut request = vec![];
            // Request move, drag, press and release. Shift stays with gpm selection.
            for value in [15u16, 0, 0, u16::MAX ^ 1] {
                request.extend_from_slice(&value.to_ne_bytes());
            }
            request.extend_from_slice(&(std::process::id() as i32).to_ne_bytes());
            request.extend_from_slice(&vc.to_ne_bytes());
            stream.write_all(&request).ok()?;
            stream.set_nonblocking(true).ok()?;
            Some(Self {
                stream,
                bytes: vec![],
            })
        }
        pub fn poll(&mut self) -> Vec<MouseEvent> {
            let mut buffer = [0; 1024];
            loop {
                match self.stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => self.bytes.extend_from_slice(&buffer[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let mut events = vec![];
            while self.bytes.len() >= 28 {
                let packet: Vec<_> = self.bytes.drain(..28).collect();
                if let Some(event) = decode(&packet) {
                    events.push(event);
                }
            }
            events
        }
    }
    fn decode(packet: &[u8]) -> Option<MouseEvent> {
        let buttons = packet[0];
        let modifiers = packet[1];
        let short = |i| i16::from_ne_bytes([packet[i], packet[i + 1]]);
        let kind = i32::from_ne_bytes(packet[12..16].try_into().ok()?) & 15;
        let button = if buttons & 4 != 0 {
            MouseButton::Left
        } else if buttons & 2 != 0 {
            MouseButton::Middle
        } else {
            MouseButton::Right
        };
        let kind = if short(26) > 0 || buttons & 16 != 0 {
            MouseEventKind::ScrollUp
        } else if short(26) < 0 || buttons & 32 != 0 {
            MouseEventKind::ScrollDown
        } else {
            match kind {
                1 => MouseEventKind::Moved,
                2 => MouseEventKind::Drag(button),
                4 => MouseEventKind::Down(button),
                8 => MouseEventKind::Up(button),
                _ => return None,
            }
        };
        let mut keys = KeyModifiers::NONE;
        if modifiers & 1 != 0 {
            keys |= KeyModifiers::SHIFT;
        }
        if modifiers & 4 != 0 {
            keys |= KeyModifiers::CONTROL;
        }
        if modifiers & (2 | 8) != 0 {
            keys |= KeyModifiers::ALT;
        }
        Some(MouseEvent {
            kind,
            column: short(8).saturating_sub(1).max(0) as u16,
            row: short(10).saturating_sub(1).max(0) as u16,
            modifiers: keys,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_packet_to_zero_based_mouse() {
            let mut packet = [0; 28];
            packet[0] = 4;
            packet[8..10].copy_from_slice(&21i16.to_ne_bytes());
            packet[10..12].copy_from_slice(&7i16.to_ne_bytes());
            packet[12..16].copy_from_slice(&4i32.to_ne_bytes());
            let event = decode(&packet).unwrap();
            assert_eq!(event.column, 20);
            assert_eq!(event.row, 6);
            assert_eq!(event.kind, MouseEventKind::Down(MouseButton::Left));
        }
    }
}
#[cfg(target_os = "linux")]
pub use linux::Gpm;
#[cfg(not(target_os = "linux"))]
pub struct Gpm;
#[cfg(not(target_os = "linux"))]
impl Gpm {
    pub fn connect() -> Option<Self> {
        None
    }
    pub fn poll(&mut self) -> Vec<crossterm::event::MouseEvent> {
        vec![]
    }
}
