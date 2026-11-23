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
    assert_eq!(parallel_header, header);
    assert_eq!(parallel, serial);
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
