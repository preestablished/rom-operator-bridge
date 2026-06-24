pub const SYNTHETIC_FRAME_WIDTH: u32 = 256;
pub const SYNTHETIC_FRAME_HEIGHT: u32 = 224;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_DEFLATE_STORED_BLOCK: usize = 65_535;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFramebufferFormat {
    Xrgb8888,
}

#[derive(Debug, Clone, Copy)]
pub struct RawFramebuffer<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: RawFramebufferFormat,
    pub pixels: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferConvertError;

pub fn synthetic_frame_png(frame: u64) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((SYNTHETIC_FRAME_WIDTH * SYNTHETIC_FRAME_HEIGHT * 3) as usize);

    for y in 0..SYNTHETIC_FRAME_HEIGHT {
        for x in 0..SYNTHETIC_FRAME_WIDTH {
            let phase = frame as u8;
            rgb.push((x as u8).wrapping_add(phase));
            rgb.push((y as u8).wrapping_mul(2).wrapping_add(phase));
            rgb.push(((x ^ y) as u8).wrapping_add(phase));
        }
    }

    rgb8_png(SYNTHETIC_FRAME_WIDTH, SYNTHETIC_FRAME_HEIGHT, &rgb)
        .expect("synthetic frame dimensions match generated RGB bytes")
}

pub fn framebuffer_png(raw: RawFramebuffer<'_>) -> Result<Vec<u8>, FramebufferConvertError> {
    if raw.width != SYNTHETIC_FRAME_WIDTH || raw.height != SYNTHETIC_FRAME_HEIGHT {
        return Err(FramebufferConvertError);
    }
    let rgb = xrgb8888_to_rgb8(raw)?;
    rgb8_png(raw.width, raw.height, &rgb)
}

pub fn xrgb8888_to_rgb8(raw: RawFramebuffer<'_>) -> Result<Vec<u8>, FramebufferConvertError> {
    if raw.format != RawFramebufferFormat::Xrgb8888 || raw.width == 0 || raw.height == 0 {
        return Err(FramebufferConvertError);
    }

    let width = usize::try_from(raw.width).map_err(|_| FramebufferConvertError)?;
    let height = usize::try_from(raw.height).map_err(|_| FramebufferConvertError)?;
    let stride = usize::try_from(raw.stride).map_err(|_| FramebufferConvertError)?;
    let row_bytes = width.checked_mul(4).ok_or(FramebufferConvertError)?;
    if stride < row_bytes {
        return Err(FramebufferConvertError);
    }
    let expected_len = stride.checked_mul(height).ok_or(FramebufferConvertError)?;
    if raw.pixels.len() != expected_len {
        return Err(FramebufferConvertError);
    }

    let rgb_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or(FramebufferConvertError)?;
    let mut rgb = Vec::with_capacity(rgb_len);
    for row in raw.pixels.chunks_exact(stride) {
        for pixel in row[..row_bytes].chunks_exact(4) {
            rgb.push(pixel[2]);
            rgb.push(pixel[1]);
            rgb.push(pixel[0]);
        }
    }
    Ok(rgb)
}

pub fn rgb8_png(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, FramebufferConvertError> {
    if width == 0 || height == 0 {
        return Err(FramebufferConvertError);
    }
    let width_usize = usize::try_from(width).map_err(|_| FramebufferConvertError)?;
    let height_usize = usize::try_from(height).map_err(|_| FramebufferConvertError)?;
    let expected_len = width_usize
        .checked_mul(height_usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or(FramebufferConvertError)?;
    if rgb.len() != expected_len {
        return Err(FramebufferConvertError);
    }

    let scanline_len = width_usize
        .checked_mul(3)
        .and_then(|row| row.checked_add(1))
        .ok_or(FramebufferConvertError)?;
    let scanlines_len = scanline_len
        .checked_mul(height_usize)
        .ok_or(FramebufferConvertError)?;
    let mut scanlines = Vec::with_capacity(scanlines_len);
    for row in rgb.chunks_exact(width_usize * 3) {
        scanlines.push(0);
        scanlines.extend_from_slice(row);
    }

    let mut png = Vec::new();
    png.extend_from_slice(PNG_SIGNATURE);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &ihdr);

    push_chunk(&mut png, b"IDAT", &zlib_stored_stream(&scanlines));
    push_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

fn zlib_stored_stream(data: &[u8]) -> Vec<u8> {
    let mut stream = Vec::with_capacity(data.len() + data.len() / MAX_DEFLATE_STORED_BLOCK * 5 + 8);
    stream.extend_from_slice(&[0x78, 0x01]);

    for (index, chunk) in data.chunks(MAX_DEFLATE_STORED_BLOCK).enumerate() {
        let final_block = index == data.len().saturating_sub(1) / MAX_DEFLATE_STORED_BLOCK;
        stream.push(if final_block { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        stream.extend_from_slice(&len.to_le_bytes());
        stream.extend_from_slice(&(!len).to_le_bytes());
        stream.extend_from_slice(chunk);
    }

    stream.extend_from_slice(&adler32(data).to_be_bytes());
    stream
}

fn push_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);

    let mut crc_input = Vec::with_capacity(kind.len() + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    png.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
