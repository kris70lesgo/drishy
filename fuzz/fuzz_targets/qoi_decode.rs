#![no_main]

use drishy::{decode, index::DecodeIndex, parallel};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

const MAX_INPUT_BYTES: usize = 16 * 1024;
const MAX_DECODE_BYTES: usize = 16 * 1024;

fuzz_target!(|input: &[u8]| {
    if input.len() > MAX_INPUT_BYTES {
        return;
    }

    // Exercise the public API with unmodified input, but do not let a crafted
    // header request a large pixel buffer during fuzzing.
    let raw_header = decode::read_header(input);
    if let Ok(header) = raw_header {
        if header
            .byte_count()
            .is_ok_and(|size| size <= MAX_DECODE_BYTES)
        {
            let _ = decode::decode(input);
            let mut output = vec![0; header.byte_count().expect("bounded header")];
            let _ = decode::decode_into(input, &mut output);
        }
    }
    if input.starts_with(b"DRSHYIDX") {
        let _ = DecodeIndex::read_from(Cursor::new(input));
    }

    // Keep the QOI header valid while fuzzing opcode bytes, so mutations can
    // reach the stateful decoder without discovering a four-byte magic first.
    let width = u32::from(input.first().copied().unwrap_or(0) % 64) + 1;
    let height = u32::from(input.get(1).copied().unwrap_or(0) % 64) + 1;
    let channels = if input.get(2).copied().unwrap_or(0) & 1 == 0 {
        3
    } else {
        4
    };
    let colorspace = input.get(3).copied().unwrap_or(0) & 1;
    let header = decode::Header {
        width,
        height,
        channels,
        colorspace,
    };
    let mut qoi = Vec::with_capacity(14 + input.len() + decode::END_MARKER.len());
    qoi.extend_from_slice(b"qoif");
    qoi.extend_from_slice(&header.width.to_be_bytes());
    qoi.extend_from_slice(&header.height.to_be_bytes());
    qoi.push(header.channels);
    qoi.push(header.colorspace);
    qoi.extend_from_slice(input.get(4..).unwrap_or_default());
    qoi.extend_from_slice(&decode::END_MARKER);

    let mut output = vec![0; header.byte_count().expect("bounded dimensions")];
    let _ = decode::decode_into(&qoi, &mut output);
    if let Ok(index) = DecodeIndex::build(&qoi, 2) {
        let mut serialized = Vec::new();
        if index.write_to(&mut serialized).is_ok() {
            let _ = DecodeIndex::read_from(Cursor::new(&serialized));
            let _ = parallel::decode_indexed(&qoi, &index);

            // Mutate the serialized sidecar fields while preserving its size.
            // The count limit keeps malformed counts from causing large allocations.
            if input.len() >= 4 {
                let mut altered = serialized;
                let offset = (usize::from(input[0]) % altered.len()).max(28);
                let offset = offset.min(altered.len() - 1);
                altered[offset] ^= input[1];
                let _ = DecodeIndex::read_from(Cursor::new(altered));
            }
        }
    }
});
