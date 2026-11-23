use drishy::{decode, encode, index::DecodeIndex, parallel};

#[test]
fn round_trips_rgb_and_rgba_pixels() {
    for (channels, pixels) in [
        (3, vec![255, 0, 0, 255, 0, 0, 255, 255, 255, 0, 255, 0]),
        (
            4,
            vec![255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0, 0, 255, 0, 64],
        ),
    ] {
        let qoi = encode::encode_rgb_or_rgba(2, 2, channels, &pixels).unwrap();
        let (header, decoded) = decode::decode(&qoi).unwrap();
        assert_eq!(header.width, 2);
        assert_eq!(header.height, 2);
        assert_eq!(header.channels, channels);
        assert_eq!(decoded, pixels);
    }
}

#[test]
fn randomized_images_match_independent_qoi_decoders() {
    let mut random = 0x6a09_e667_f3bc_c909_u64;

    for case in 0..96_usize {
        let width = usize::from(next_random(&mut random) % 96) + 1;
        let height = usize::from(next_random(&mut random) % 80) + 1;
        let channels = if case % 2 == 0 { 3 } else { 4 };
        let mut pixels = vec![0; width * height * usize::from(channels)];

        for y in 0..height {
            for x in 0..width {
                let offset = (y * width + x) * usize::from(channels);
                let pixel = match case % 4 {
                    0 => [32, 64, 96, 255],
                    1 => [x as u8, y as u8, (x + y) as u8, 255],
                    2 => [
                        ((x / 8) % 32) as u8,
                        ((y / 8) % 32) as u8,
                        ((x + y) / 8) as u8,
                        if (x + y) % 7 == 0 { 128 } else { 255 },
                    ],
                    _ => [
                        next_random(&mut random),
                        next_random(&mut random),
                        next_random(&mut random),
                        next_random(&mut random),
                    ],
                };
                pixels[offset..offset + usize::from(channels)]
                    .copy_from_slice(&pixel[..usize::from(channels)]);
            }
        }

        let qoi =
            encode::encode_rgb_or_rgba(width as u32, height as u32, channels, &pixels).unwrap();
        let (header, decoded) = decode::decode(&qoi).unwrap();
        assert_eq!(header.width, width as u32);
        assert_eq!(header.height, height as u32);
        assert_eq!(decoded, pixels, "Drishy round trip, case {case}");
        assert_eq!(
            qoi::decode_to_vec(&qoi).unwrap().1,
            pixels,
            "qoi crate, case {case}"
        );
        assert_eq!(
            rapid_qoi::Qoi::decode_alloc(&qoi).unwrap().1,
            pixels,
            "rapid-qoi, case {case}"
        );
    }
}

#[test]
fn encoder_does_not_index_pixels_seen_only_in_a_run() {
    let pixels = [0, 0, 0, 255, 45, 45, 45, 255, 0, 0, 0, 255];
    let qoi = encode::encode_rgb_or_rgba(3, 1, 4, &pixels).unwrap();
    assert_eq!(&qoi[14..23], &[0xc0, 0xfe, 45, 45, 45, 0xfe, 0, 0, 0]);
    assert_eq!(decode::decode(&qoi).unwrap().1, pixels);
}

#[test]
fn decoder_indexes_the_initial_pixel_run() {
    let mut qoi = Vec::from(&b"qoif"[..]);
    qoi.extend_from_slice(&3_u32.to_be_bytes());
    qoi.extend_from_slice(&1_u32.to_be_bytes());
    qoi.extend_from_slice(&[4, 0]);
    qoi.extend_from_slice(&[0xc0, 0xfe, 45, 45, 45, 0x35]);
    qoi.extend_from_slice(&decode::END_MARKER);

    let pixels = [0, 0, 0, 255, 45, 45, 45, 255, 0, 0, 0, 255];
    assert_eq!(decode::decode(&qoi).unwrap().1, pixels);
}

#[test]
fn rejects_truncated_bad_and_inconsistent_files() {
    assert!(decode::read_header(b"qoif").is_err());
    let qoi = encode::encode_rgb_or_rgba(1, 1, 3, &[1, 2, 3]).unwrap();
    let mut invalid_magic = qoi.clone();
    invalid_magic[0] = b'x';
    assert!(decode::decode(&invalid_magic).is_err());
    let mut invalid_end = qoi.clone();
    *invalid_end.last_mut().unwrap() = 0;
    assert!(decode::decode(&invalid_end).is_err());
    assert!(encode::encode_rgb_or_rgba(1, 1, 3, &[1, 2]).is_err());
}

#[test]
fn indexed_parallel_decode_matches_serial_decode() {
    let pixels: Vec<u8> = (0..96 * 64 * 4)
        .map(|position| ((position * 37 + position / 11) % 256) as u8)
        .collect();
    let qoi = encode::encode_rgb_or_rgba(96, 64, 4, &pixels).unwrap();
    let index = DecodeIndex::build(&qoi, 6).unwrap();
    assert!(index.checkpoint_count() > 1);

    let (header, serial) = decode::decode(&qoi).unwrap();
    let (parallel_header, parallel) = parallel::decode_indexed(&qoi, &index).unwrap();
    let bound = parallel::IndexedDecoder::new(&qoi, &index).unwrap();
    let (bound_header, bound_pixels) = bound.decode().unwrap();
    let built = parallel::IndexedDecoder::build(&qoi, 6).unwrap();
    let (built_header, built_pixels) = built.decode().unwrap();
    assert_eq!(parallel_header, header);
    assert_eq!(parallel, serial);
    assert_eq!(bound_header, header);
    assert_eq!(bound_pixels, serial);
    assert_eq!(built_header, header);
    assert_eq!(built_pixels, serial);
}

#[test]
fn rejects_an_index_for_a_different_image() {
    let qoi = encode::encode_rgb_or_rgba(1, 1, 3, &[1, 2, 3]).unwrap();
    let index = DecodeIndex::build(&qoi, 2).unwrap();
    let mut other = qoi.clone();
    other[14] ^= 1;
    assert!(parallel::decode_indexed(&other, &index).is_err());
}

#[test]
fn index_sidecar_round_trips() {
    let pixels: Vec<u8> = (0..40 * 40 * 3).map(|value| (value % 251) as u8).collect();
    let qoi = encode::encode_rgb_or_rgba(40, 40, 3, &pixels).unwrap();
    let index = DecodeIndex::build(&qoi, 4).unwrap();
    let mut serialized = Vec::new();
    index.write_to(&mut serialized).unwrap();
    let restored = DecodeIndex::read_from(serialized.as_slice()).unwrap();
    assert_eq!(parallel::decode_indexed(&qoi, &restored).unwrap().1, pixels);
}

fn next_random(state: &mut u64) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state as u8
}
