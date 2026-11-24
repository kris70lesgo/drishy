#![no_main]

use drishy::{decode, restart_qoi};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 16 * 1024;
const MAX_DECODE_BYTES: usize = 16 * 1024;

fuzz_target!(|input: &[u8]| {
    if input.len() > MAX_INPUT_BYTES {
        return;
    }

    // Check arbitrary files while limiting any allocation to a small image.
    if let Ok(header) = restart_qoi::read_header(input) {
        if header
            .byte_count()
            .is_ok_and(|size| size <= MAX_DECODE_BYTES)
        {
            let _ = restart_qoi::decode(input);
            let mut output = vec![0; header.byte_count().expect("bounded header")];
            let _ = restart_qoi::decode_into(input, &mut output);
        }
    }

    // Synthesize a small valid stream from fuzz bytes, then vary its payload
    // without changing its directory so opcode decoding is reached consistently.
    let width = u32::from(input.first().copied().unwrap_or(0) % 32) + 1;
    let height = u32::from(input.get(1).copied().unwrap_or(0) % 32) + 1;
    let channels = if input.get(2).copied().unwrap_or(0) & 1 == 0 {
        3
    } else {
        4
    };
    let header = decode::Header {
        width,
        height,
        channels,
        colorspace: input.get(3).copied().unwrap_or(0) & 1,
    };
    let mut pixels = vec![0; header.byte_count().expect("bounded dimensions")];
    if !input.is_empty() {
        for (offset, byte) in pixels.iter_mut().enumerate() {
            *byte = input[offset % input.len()];
        }
    }
    let block_pixels = [16, 64, 256][usize::from(input.get(4).copied().unwrap_or(0) % 3)];
    let Ok(stream) = restart_qoi::encode_with_block_pixels(header, &pixels, block_pixels) else {
        return;
    };
    let _ = restart_qoi::decode(&stream);

    let block_count = u32::from_le_bytes(stream[18..22].try_into().expect("restart header"));
    let payload_start = 22 + block_count as usize * 4;
    if payload_start >= stream.len() {
        return;
    }
    let mut changed_payload = stream.clone();
    for (position, byte) in input.iter().enumerate().skip(5) {
        let offset = payload_start + position % (changed_payload.len() - payload_start);
        changed_payload[offset] ^= byte;
    }
    let _ = restart_qoi::decode(&changed_payload);

    // Also alter one header/directory byte to explore structural validation.
    if input.len() > 5 {
        let mut changed_directory = stream;
        let offset = 14 + usize::from(input[5]) % (payload_start - 14);
        changed_directory[offset] ^= input[5];
        let _ = restart_qoi::read_header(&changed_directory);
        let _ = restart_qoi::decode(&changed_directory);
    }
});
