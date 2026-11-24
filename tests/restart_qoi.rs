use drishy::{decode, encode, restart_qoi};

#[test]
fn restart_stream_round_trips_all_qoi_opcode_shapes_and_block_edges() {
    let mut random = 0x243f_6a88_85a3_08d3_u64;
    for (channels, width, height) in [(3, 257_usize, 131_usize), (4, 193, 177)] {
        for pattern in 0..4 {
            let mut pixels = vec![0; width * height * usize::from(channels)];
            for (pixel_number, pixel) in pixels.chunks_exact_mut(usize::from(channels)).enumerate()
            {
                let value = match pattern {
                    0 => [32, 64, 96, 255],
                    1 => [
                        (pixel_number % width) as u8,
                        (pixel_number / width) as u8,
                        (pixel_number % 251) as u8,
                        255,
                    ],
                    2 => [
                        ((pixel_number / 3) % 256) as u8,
                        ((pixel_number / 17) % 256) as u8,
                        ((pixel_number / 61) % 256) as u8,
                        if channels == 4 && pixel_number % 13 == 0 {
                            0
                        } else {
                            255
                        },
                    ],
                    _ => [
                        next_random(&mut random),
                        next_random(&mut random),
                        next_random(&mut random),
                        next_random(&mut random),
                    ],
                };
                pixel.copy_from_slice(&value[..usize::from(channels)]);
            }

            let header = decode::Header {
                width: width as u32,
                height: height as u32,
                channels,
                colorspace: 1,
            };
            let ordinary_qoi = encode::encode(header, &pixels).unwrap();
            assert_eq!(qoi::decode_to_vec(&ordinary_qoi).unwrap().1, pixels);

            for block_pixels in [62, 4096, restart_qoi::DEFAULT_BLOCK_PIXELS] {
                let stream =
                    restart_qoi::encode_with_block_pixels(header, &pixels, block_pixels).unwrap();
                assert_eq!(restart_qoi::read_header(&stream).unwrap(), header);
                assert_eq!(restart_qoi::decode(&stream).unwrap().1, pixels);

                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(4)
                    .build()
                    .unwrap();
                let mut reused = vec![0; pixels.len()];
                assert_eq!(
                    restart_qoi::decode_into_with_pool(&stream, &mut reused, &pool).unwrap(),
                    header
                );
                assert_eq!(reused, pixels);

                assert_restart_payloads_are_valid_qoi(&stream, &pixels, header);
            }
        }
    }
}

#[test]
fn restart_stream_rejects_bad_headers_directories_and_payloads() {
    let stream = restart_qoi::encode_rgb_or_rgba(8, 8, 4, &[12; 8 * 8 * 4]).unwrap();
    let mut output = vec![0; 8 * 8 * 4];

    let mut invalid_magic = stream.clone();
    invalid_magic[0] = b'x';
    assert!(restart_qoi::decode(&invalid_magic).is_err());

    let mut zero_block_size = stream.clone();
    zero_block_size[14..18].copy_from_slice(&0_u32.to_le_bytes());
    assert!(restart_qoi::decode(&zero_block_size).is_err());

    let mut wrong_block_count = stream.clone();
    wrong_block_count[18..22].copy_from_slice(&2_u32.to_le_bytes());
    assert!(restart_qoi::decode(&wrong_block_count).is_err());

    let mut truncated = stream.clone();
    truncated.pop();
    assert!(restart_qoi::decode(&truncated).is_err());

    let mut trailing_byte = stream.clone();
    trailing_byte.push(0);
    assert!(restart_qoi::decode(&trailing_byte).is_err());

    let short_output_len = output.len() - 1;
    assert_eq!(
        restart_qoi::decode_into(&stream, &mut output[..short_output_len]),
        Err(decode::QoiError::PixelBufferSize)
    );
    assert!(
        restart_qoi::encode_with_block_pixels(
            decode::Header {
                width: 8,
                height: 8,
                channels: 4,
                colorspace: 0,
            },
            &[12; 8 * 8 * 4],
            0,
        )
        .is_err()
    );
}

#[test]
fn restart_encoder_parallel_blocks_are_stable_and_pixel_exact() {
    let header = decode::Header {
        width: 1024,
        height: 256,
        channels: 3,
        colorspace: 0,
    };
    let pixels: Vec<u8> = (0..header.byte_count().unwrap())
        .map(|value| value.wrapping_mul(37).wrapping_add(value >> 5) as u8)
        .collect();

    assert!(header.pixel_count().unwrap() >= restart_qoi::PARALLEL_ENCODE_MIN_PIXELS);
    let first = restart_qoi::encode(header, &pixels).unwrap();
    let second = restart_qoi::encode(header, &pixels).unwrap();
    assert_eq!(first, second, "parallel block order must be deterministic");
    assert_eq!(restart_qoi::decode(&first).unwrap().1, pixels);
    assert_restart_payloads_are_valid_qoi(&first, &pixels, header);
}

fn assert_restart_payloads_are_valid_qoi(stream: &[u8], expected: &[u8], header: decode::Header) {
    let block_pixels = u32::from_le_bytes(stream[14..18].try_into().unwrap()) as usize;
    let block_count = u32::from_le_bytes(stream[18..22].try_into().unwrap()) as usize;
    let total_pixels = header.width as usize * header.height as usize;
    let channels = usize::from(header.channels);
    let mut payload_offset = 22 + block_count * 4;

    for block_index in 0..block_count {
        let length_offset = 22 + block_index * 4;
        let payload_len =
            u32::from_le_bytes(stream[length_offset..length_offset + 4].try_into().unwrap())
                as usize;
        let payload_end = payload_offset + payload_len;
        let count = (total_pixels - block_index * block_pixels).min(block_pixels);
        let mut qoi_file = Vec::from(&b"qoif"[..]);
        qoi_file.extend_from_slice(&(count as u32).to_be_bytes());
        qoi_file.extend_from_slice(&1_u32.to_be_bytes());
        qoi_file.extend_from_slice(&[header.channels, header.colorspace]);
        qoi_file.extend_from_slice(&stream[payload_offset..payload_end]);
        qoi_file.extend_from_slice(&decode::END_MARKER);
        assert_eq!(
            qoi::decode_to_vec(&qoi_file).unwrap().1,
            expected[block_index * block_pixels * channels
                ..(block_index * block_pixels + count) * channels]
        );
        payload_offset = payload_end;
    }
    assert_eq!(payload_offset, stream.len());
}

fn next_random(state: &mut u64) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state as u8
}
