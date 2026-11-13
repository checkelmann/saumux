use crate::render::Grid;
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_READ: usize = 1024 * 1024; // 1 MB
const MAX_LINES: usize = 500;
const HEX_BYTES: usize = 256;

pub(super) enum PreviewContent {
    Text(Vec<String>),
    Image {
        format: &'static str,
        width: u32,
        height: u32,
        file_size: u64,
    },
    Binary(Vec<u8>),
    Error(String),
}

pub(super) struct PreviewOverlay {
    pub path: PathBuf,
    pub content: PreviewContent,
    pub scroll: usize,
}

impl PreviewOverlay {
    pub fn load(path: &Path) -> Self {
        let content = match load_content(path) {
            Ok(c) => c,
            Err(e) => PreviewContent::Error(e.to_string()),
        };
        Self {
            path: path.to_owned(),
            content,
            scroll: 0,
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, visible: usize) {
        let max = match &self.content {
            PreviewContent::Text(lines) => lines.len().saturating_sub(visible),
            _ => 0,
        };
        if self.scroll < max {
            self.scroll += 1;
        }
    }

    pub fn draw(&self, grid: &mut Grid, term_width: u16, term_height: u16) {
        let theme = crate::theme::current();
        let w = term_width.saturating_sub(4).clamp(10, 80);
        let h = term_height.saturating_sub(4).max(4);
        let x = (term_width.saturating_sub(w)) / 2;
        let y = 2u16;

        grid.fill(x, y, w, h, " ", theme.dialog.clone());

        // Title bar
        let name = self.path.file_name().unwrap_or_default().to_string_lossy();
        let title = format!(" {name} ");
        grid.fill(x, y, w, 1, " ", theme.title.clone());
        grid.text(x, y, &title, w, theme.title.clone());

        let inner_h = h.saturating_sub(2) as usize;
        let inner_w = w.saturating_sub(2);

        match &self.content {
            PreviewContent::Text(lines) => {
                for (i, line) in lines.iter().skip(self.scroll).take(inner_h).enumerate() {
                    let truncated: String = line.chars().take(inner_w as usize).collect();
                    grid.text(
                        x + 1,
                        y + 1 + i as u16,
                        &truncated,
                        inner_w,
                        theme.dialog.clone(),
                    );
                }
                // scroll indicator in footer
                let total = lines.len();
                let foot = format!(" Esc closes  line {}/{total}", self.scroll + 1);
                grid.fill(x, y + h - 1, w, 1, " ", theme.status.clone());
                grid.text(x, y + h - 1, &foot, w, theme.status.clone());
            }
            PreviewContent::Image {
                format,
                width,
                height,
                file_size,
            } => {
                let info = [
                    format!("  Format : {format}"),
                    format!("  Size   : {width}×{height} pixels"),
                    format!("  File   : {}", super::format_size(*file_size)),
                ];
                for (i, line) in info.iter().enumerate() {
                    if i < inner_h {
                        grid.text(x + 1, y + 1 + i as u16, line, inner_w, theme.dialog.clone());
                    }
                }
                grid.fill(x, y + h - 1, w, 1, " ", theme.status.clone());
                grid.text(x, y + h - 1, " Esc closes", w, theme.status.clone());
            }
            PreviewContent::Binary(bytes) => {
                for (i, chunk) in bytes.chunks(16).take(inner_h).enumerate() {
                    let hex: String = chunk.iter().map(|b| format!("{b:02x} ")).collect();
                    let ascii: String = chunk
                        .iter()
                        .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
                        .collect();
                    let line = format!("{:04x}  {:<48}{}", i * 16, hex, ascii);
                    grid.text(
                        x + 1,
                        y + 1 + i as u16,
                        &line,
                        inner_w,
                        theme.dialog.clone(),
                    );
                }
                grid.fill(x, y + h - 1, w, 1, " ", theme.status.clone());
                grid.text(
                    x,
                    y + h - 1,
                    " Esc closes  (binary)",
                    w,
                    theme.status.clone(),
                );
            }
            PreviewContent::Error(msg) => {
                grid.text(x + 1, y + 1, msg, inner_w, theme.warning.clone());
                grid.fill(x, y + h - 1, w, 1, " ", theme.status.clone());
                grid.text(x, y + h - 1, " Esc closes", w, theme.status.clone());
            }
        }
    }
}

fn load_content(path: &Path) -> anyhow::Result<PreviewContent> {
    let meta = fs::metadata(path)?;
    let file_size = meta.len();

    // Read up to MAX_READ bytes
    let bytes = {
        use std::io::Read;
        let f = fs::File::open(path)?;
        let cap = (file_size as usize).min(MAX_READ);
        let mut buf = Vec::with_capacity(cap);
        f.take(MAX_READ as u64).read_to_end(&mut buf)?;
        buf
    };

    // Image detection first
    if let Some((format, w, h)) = detect_image(&bytes) {
        return Ok(PreviewContent::Image {
            format,
            width: w,
            height: h,
            file_size,
        });
    }

    // Binary check: null bytes in the first 1 KB
    let probe = bytes.len().min(1024);
    if bytes[..probe].contains(&0) {
        return Ok(PreviewContent::Binary(
            bytes[..bytes.len().min(HEX_BYTES)].to_vec(),
        ));
    }

    // UTF-8 text
    match std::str::from_utf8(&bytes) {
        Ok(text) => {
            let lines: Vec<String> = text.lines().take(MAX_LINES).map(str::to_owned).collect();
            Ok(PreviewContent::Text(lines))
        }
        Err(_) => Ok(PreviewContent::Binary(
            bytes[..bytes.len().min(HEX_BYTES)].to_vec(),
        )),
    }
}

fn detect_image(data: &[u8]) -> Option<(&'static str, u32, u32)> {
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some(("PNG", w, h));
    }
    if data.len() >= 2 && data.starts_with(b"\xff\xd8") {
        let (w, h) = jpeg_dimensions(data).unwrap_or((0, 0));
        return Some(("JPEG", w, h));
    }
    if data.len() >= 10 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([data[6], data[7]]) as u32;
        let h = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some(("GIF", w, h));
    }
    if data.len() >= 26 && data.starts_with(b"BM") {
        let w = u32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let h = u32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        return Some(("BMP", w, h));
    }
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        let (w, h) = webp_dimensions(data).unwrap_or((0, 0));
        return Some(("WebP", w, h));
    }
    None
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 3 < data.len() {
        if data[i] != 0xff {
            break;
        }
        let marker = data[i + 1];
        if matches!(marker, 0xc0..=0xc2) && i + 9 < data.len() {
            let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Some((w, h));
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if len < 2 {
            break;
        }
        i += 2 + len;
    }
    None
}

fn webp_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 30 {
        return None;
    }
    if &data[12..16] == b"VP8 " {
        let w = (u16::from_le_bytes([data[26], data[27]]) & 0x3fff) as u32;
        let h = (u16::from_le_bytes([data[28], data[29]]) & 0x3fff) as u32;
        return Some((w + 1, h + 1));
    }
    if &data[12..16] == b"VP8L" && data.len() >= 25 {
        let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
        let w = (bits & 0x3fff) + 1;
        let h = ((bits >> 14) & 0x3fff) + 1;
        return Some((w, h));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_image_formats() {
        // PNG
        let mut png = vec![0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&[0, 0, 0, 0x0d]); // chunk length
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&100u32.to_be_bytes()); // width at [16]
        png.extend_from_slice(&200u32.to_be_bytes()); // height at [20]
        assert_eq!(detect_image(&png), Some(("PNG", 100, 200)));

        // GIF
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        assert_eq!(detect_image(&gif), Some(("GIF", 320, 240)));

        // BMP (DIB header: width at offset 18, height at 22)
        let mut bmp = vec![b'B', b'M'];
        bmp.extend(std::iter::repeat_n(0u8, 16)); // fill to offset 18
        bmp.extend_from_slice(&640u32.to_le_bytes());
        bmp.extend_from_slice(&480u32.to_le_bytes());
        assert_eq!(detect_image(&bmp), Some(("BMP", 640, 480)));

        // Not an image
        assert_eq!(detect_image(b"hello world"), None);
    }
}
