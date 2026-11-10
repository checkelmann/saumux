use anyhow::{Result, bail};
use crossterm::event::Event;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const MAX_PACKET: usize = 16 * 1024 * 1024;
#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Hello {
        version: u8,
        cols: u16,
        rows: u16,
        utf8: bool,
        #[serde(default)]
        console: bool,
    },
    Input(Event),
    Control {
        command: String,
        args: Vec<String>,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Render(String),
    Detached,
    Error(String),
    Result(serde_json::Value),
}

pub fn send<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_PACKET {
        bail!("packet too large");
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    Ok(())
}
pub fn receive<T: for<'a> Deserialize<'a>>(reader: &mut impl Read) -> Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_PACKET {
        bail!("packet too large");
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
