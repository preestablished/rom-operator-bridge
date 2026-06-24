pub const SYNTHETIC_FRAME_WIDTH: u32 = 256;
pub const SYNTHETIC_FRAME_HEIGHT: u32 = 224;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_DEFLATE_STORED_BLOCK: usize = 65_535;

pub fn synthetic_frame_png(frame: u64) -> Vec<u8> {
    let width = SYNTHETIC_FRAME_WIDTH as usize;
    let height = SYNTHETIC_FRAME_HEIGHT as usize;
    let mut scanlines = Vec::with_capacity(height * (1 + width * 3));

    for y in 0..height {
        scanlines.push(0);
        for x in 0..width {
            let phase = frame as usize;
            scanlines.push(((x + phase) & 0xff) as u8);
            scanlines.push(((y * 2 + phase) & 0xff) as u8);
            scanlines.push((((x ^ y) + phase) & 0xff) as u8);
        }
    }

    let mut png = Vec::new();
    png.extend_from_slice(PNG_SIGNATURE);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&SYNTHETIC_FRAME_WIDTH.to_be_bytes());
    ihdr.extend_from_slice(&SYNTHETIC_FRAME_HEIGHT.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &ihdr);

    push_chunk(&mut png, b"IDAT", &zlib_stored_stream(&scanlines));
    push_chunk(&mut png, b"IEND", &[]);
    png
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
