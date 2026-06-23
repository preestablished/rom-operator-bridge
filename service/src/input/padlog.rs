use crate::input::{AppliedInputFrame, PAD_MASK, PadWord};
use thiserror::Error;

pub const PADLOG_VERSION_HEADER: &str = "padlog v1";
pub const MAX_PADLOG_FRAMES: u64 = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PadLog {
    rom_blake3: Option<[u8; 32]>,
    frames: Vec<PadWord>,
}

impl PadLog {
    pub fn new(frames: Vec<PadWord>) -> Self {
        Self {
            rom_blake3: None,
            frames,
        }
    }

    pub fn with_rom_blake3(mut self, rom_blake3: [u8; 32]) -> Self {
        self.rom_blake3 = Some(rom_blake3);
        self
    }

    pub fn from_raw_frames(frames: impl IntoIterator<Item = u16>) -> Result<Self, PadLogError> {
        let mut validated = Vec::new();
        for (index, raw) in frames.into_iter().enumerate() {
            let pad_word = PadWord::new(raw)
                .map_err(|_| PadLogError::ReservedBitsInFrames { index, word: raw })?;
            validated.push(pad_word);
        }

        Ok(Self::new(validated))
    }

    pub fn from_applied_frames(
        frames: impl IntoIterator<Item = AppliedInputFrame>,
    ) -> Result<Self, PadLogError> {
        // Padlogs are dense frame scripts. This preserves iterator order and
        // intentionally does not expand gaps between absolute assigned frames.
        Self::from_raw_frames(frames.into_iter().map(|frame| frame.pad_word))
    }

    pub fn parse(text: &str) -> Result<Self, PadLogError> {
        let mut log = Self::default();
        let mut header_seen = false;

        for (idx, raw) in text.lines().enumerate() {
            let line_no = idx + 1;
            let line = match raw.find('#') {
                Some(pos) => &raw[..pos],
                None => raw,
            }
            .trim();

            if line.is_empty() {
                continue;
            }

            if !header_seen {
                parse_header(line, line_no, &mut log)?;
                header_seen = true;
                continue;
            }

            let (count, word) = parse_frame_line(line, line_no)?;
            let total_frames = (log.frames.len() as u64)
                .checked_add(count)
                .ok_or(PadLogError::TooManyFrames { line: line_no })?;
            if total_frames > MAX_PADLOG_FRAMES {
                return Err(PadLogError::TooManyFrames { line: line_no });
            }

            let pad_word = PadWord::new(word).map_err(|_| PadLogError::ReservedBitsSet {
                line: line_no,
                word,
            })?;
            log.frames
                .extend(std::iter::repeat_n(pad_word, count as usize));
        }

        if !header_seen {
            return Err(PadLogError::BadHeader {
                line: text.lines().count() + 1,
            });
        }

        Ok(log)
    }

    pub fn write_canonical(&self) -> String {
        let mut out = String::from(PADLOG_VERSION_HEADER);
        if let Some(hash) = &self.rom_blake3 {
            out.push_str(" rom=");
            for byte in hash {
                out.push_str(&format!("{byte:02x}"));
            }
        }
        out.push('\n');

        let mut index = 0;
        while index < self.frames.len() {
            let word = self.frames[index].raw();
            let mut run = 1usize;
            while index + run < self.frames.len() && self.frames[index + run].raw() == word {
                run += 1;
            }

            if run > 1 {
                out.push_str(&format!("{run}x{word:04x}\n"));
            } else {
                out.push_str(&format!("{word:04x}\n"));
            }

            index += run;
        }

        out
    }

    pub fn frames(&self) -> &[PadWord] {
        &self.frames
    }

    pub fn rom_blake3(&self) -> Option<[u8; 32]> {
        self.rom_blake3
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PadLogError {
    #[error("line {line}: expected `padlog v1` header")]
    BadHeader { line: usize },
    #[error("line {line}: unsupported padlog version `{version}`")]
    UnsupportedVersion { line: usize, version: String },
    #[error("line {line}: rom= value must be 64 hex characters")]
    BadRomHash { line: usize },
    #[error("line {line}: cannot parse frame line `{text}`")]
    BadFrameLine { line: usize, text: String },
    #[error("line {line}: pad word {word:#06x} sets reserved bits 12-15")]
    ReservedBitsSet { line: usize, word: u16 },
    #[error("line {line}: run-length count must be >= 1")]
    ZeroRun { line: usize },
    #[error("line {line}: total frame count exceeds the {MAX_PADLOG_FRAMES}-frame limit")]
    TooManyFrames { line: usize },
    #[error("frame {index}: pad word {word:#06x} sets reserved bits 12-15")]
    ReservedBitsInFrames { index: usize, word: u16 },
}

fn parse_header(line: &str, line_no: usize, log: &mut PadLog) -> Result<(), PadLogError> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("padlog") {
        return Err(PadLogError::BadHeader { line: line_no });
    }

    match parts.next() {
        Some("v1") => {}
        Some(version) => {
            return Err(PadLogError::UnsupportedVersion {
                line: line_no,
                version: version.to_string(),
            });
        }
        None => return Err(PadLogError::BadHeader { line: line_no }),
    }

    for field in parts {
        if let Some(hex) = field.strip_prefix("rom=") {
            log.rom_blake3 =
                Some(parse_hex32(hex).ok_or(PadLogError::BadRomHash { line: line_no })?);
        } else {
            return Err(PadLogError::BadHeader { line: line_no });
        }
    }

    Ok(())
}

fn parse_frame_line(line: &str, line_no: usize) -> Result<(u64, u16), PadLogError> {
    let bad = || PadLogError::BadFrameLine {
        line: line_no,
        text: line.to_string(),
    };
    let (count_str, word_str) = match line.split_once(['x', 'X']) {
        Some((count, word)) => (Some(count), word),
        None => (None, line),
    };

    if word_str.len() != 4 || !word_str.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let word = u16::from_str_radix(word_str, 16).map_err(|_| bad())?;
    if word & !PAD_MASK != 0 {
        return Err(PadLogError::ReservedBitsSet {
            line: line_no,
            word,
        });
    }

    let count = match count_str {
        Some(count) => {
            if count.is_empty() || !count.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(bad());
            }

            let count = count.parse::<u64>().map_err(|_| bad())?;
            if count == 0 {
                return Err(PadLogError::ZeroRun { line: line_no });
            }
            count
        }
        None => 1,
    };

    Ok((count, word))
}

fn parse_hex32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }

    let mut out = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let hex_byte = std::str::from_utf8(chunk).ok()?;
        out[index] = u8::from_str_radix(hex_byte, 16).ok()?;
    }

    Some(out)
}
