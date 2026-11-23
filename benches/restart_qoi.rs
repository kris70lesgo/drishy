use drishy::{decode, encode, restart_qoi};
use std::{
    env, fs,
    hint::black_box,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Image {
    path: PathBuf,
    qoi: Vec<u8>,
    restart: Vec<u8>,
    width: u32,
    height: u32,
    channels: u8,
    colorspace: u8,
    pixels: usize,
    pixel_bytes: usize,
}

#[derive(Clone, Copy)]
struct Timing {
    median: Duration,
    fastest: Duration,
    slowest: Duration,
}

#[derive(Clone, Copy)]
enum Decoder {
    Qoi,
    Drishy,
    Restart,
    Rapid,
}

fn prepare(path: &Path, block_pixels: usize) -> Result<Option<Image>, Box<dyn std::error::Error>> {
    let (qoi, expected) = match path.extension().and_then(|ext| ext.to_str()) {
        Some("qoi") => {
            let bytes = fs::read(path)?;
            let (_, pixels) = qoi::decode_to_vec(&bytes)?;
            (bytes, pixels)
        }
        Some("png") => {
            let reader = image::ImageReader::open(path)?.with_guessed_format()?;
            let source = match reader.decode() {
                Ok(source) => source,
                Err(_) => return Ok(None),
            };
            if source.color().has_alpha() {
                let pixels = source.into_rgba8();
                let qoi = encode::encode_rgb_or_rgba(
                    pixels.width(),
                    pixels.height(),
                    4,
                    pixels.as_raw(),
                )?;
                (qoi, pixels.into_raw())
            } else {
                let pixels = source.into_rgb8();
                let qoi = encode::encode_rgb_or_rgba(
                    pixels.width(),
                    pixels.height(),
                    3,
                    pixels.as_raw(),
                )?;
                (qoi, pixels.into_raw())
            }
        }
        _ => return Err(format!("expected .png or .qoi: {}", path.display()).into()),
    };
    let (header, decoded) = decode::decode(&qoi)?;
    ensure_equal(path, "Drishy QOI", &expected, &decoded)?;
    let (_, decoded_by_qoi) = qoi::decode_to_vec(&qoi)?;
    ensure_equal(path, "original qoi", &expected, &decoded_by_qoi)?;
    let (_, decoded_by_rapid_qoi) = rapid_qoi::Qoi::decode_alloc(&qoi)?;
    ensure_equal(path, "rapid-qoi", &expected, &decoded_by_rapid_qoi)?;
    let mut reused = vec![0; expected.len()];
    decode::decode_into(&qoi, &mut reused)?;
    ensure_equal(path, "Drishy reused output", &expected, &reused)?;
    qoi::decode_to_buf(&mut reused, &qoi)?;
    ensure_equal(path, "qoi reused output", &expected, &reused)?;
    rapid_qoi::Qoi::decode(&qoi, &mut reused)?;
    ensure_equal(path, "rapid-qoi reused output", &expected, &reused)?;
    let restart = restart_qoi::encode_with_block_pixels(header, &expected, block_pixels)?;
    Ok(Some(Image {
        path: path.to_path_buf(),
        qoi,
        restart,
        width: header.width,
        height: header.height,
        channels: header.channels,
        colorspace: header.colorspace,
        pixels: header.pixel_count()?,
        pixel_bytes: header.byte_count()?,
    }))
}

fn ensure_equal(path: &Path, decoder: &str, expected: &[u8], actual: &[u8]) -> io::Result<()> {
    if expected == actual {
        return Ok(());
    }
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
        .unwrap_or(expected.len().min(actual.len()));
    Err(io::Error::other(format!(
        "{decoder} differs for {} at byte {offset} ({} vs {} bytes)",
        path.display(),
        expected.len(),
        actual.len()
    )))
}

fn collect_paths(path: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_paths(&entry?.path(), paths)?;
        }
    } else if matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("png" | "qoi")
    ) {
        paths.push(path.to_path_buf());
    }
    Ok(())
}

fn decode(image: &Image, decoder: Decoder) {
    match decoder {
        Decoder::Qoi => {
            black_box(qoi::decode_to_vec(black_box(&image.qoi)).expect("validated QOI"));
        }
        Decoder::Drishy => {
            black_box(decode::decode(black_box(&image.qoi)).expect("validated QOI"));
        }
        Decoder::Restart => {
            black_box(restart_qoi::decode(black_box(&image.restart)).expect("validated restart"));
        }
        Decoder::Rapid => {
            black_box(rapid_qoi::Qoi::decode_alloc(black_box(&image.qoi)).expect("validated QOI"));
        }
    }
}

fn time_corpus_pass(images: &[&Image], decoder: Decoder) -> Duration {
    let start = Instant::now();
    for image in images {
        decode(image, decoder);
    }
    start.elapsed()
}

fn decode_reused(image: &Image, decoder: Decoder, output: &mut [u8]) {
    let output = &mut output[..image.pixel_bytes];
    match decoder {
        Decoder::Qoi => {
            black_box(
                qoi::decode_to_buf(&mut *output, black_box(&image.qoi)).expect("validated QOI"),
            );
        }
        Decoder::Drishy => {
            black_box(
                decode::decode_into(black_box(&image.qoi), &mut *output).expect("validated QOI"),
            );
        }
        Decoder::Restart => {
            black_box(
                restart_qoi::decode_into(black_box(&image.restart), &mut *output)
                    .expect("validated restart"),
            );
        }
        Decoder::Rapid => {
            black_box(
                rapid_qoi::Qoi::decode(black_box(&image.qoi), &mut *output).expect("validated QOI"),
            );
        }
    }
    black_box(output);
}

fn time_reused_pass(images: &[&Image], decoder: Decoder, output: &mut [u8]) -> Duration {
    let start = Instant::now();
    for image in images {
        decode_reused(image, decoder, output);
    }
    start.elapsed()
}

fn summarize(
    samples: [Vec<Duration>; 4],
    paired_speedups: [Vec<f64>; 2],
) -> ([Timing; 4], [[f64; 3]; 2]) {
    let timings = samples.map(|mut samples| {
        samples.sort_by_key(Duration::as_nanos);
        Timing {
            median: samples[2],
            fastest: samples[0],
            slowest: samples[4],
        }
    });
    let speedup_summary = paired_speedups.map(|mut speedups| {
        speedups.sort_by(f64::total_cmp);
        [speedups[2], speedups[0], speedups[4]]
    });
    (timings, speedup_summary)
}

fn measure(
    images: &[&Image],
    decoders: [Decoder; 4],
    target_sample_seconds: f64,
) -> ([Timing; 4], [[f64; 3]; 2]) {
    for decoder in decoders {
        for image in images {
            decode(image, decoder);
        }
    }
    let repeats = decoders.map(|decoder| {
        let pass = time_corpus_pass(images, decoder).as_secs_f64();
        (target_sample_seconds / pass).ceil().max(1.0) as usize
    });
    let orders = [
        [0, 1, 2, 3],
        [1, 2, 3, 0],
        [2, 3, 0, 1],
        [3, 0, 1, 2],
        [0, 2, 1, 3],
    ];
    let mut samples: [Vec<Duration>; 4] = std::array::from_fn(|_| Vec::with_capacity(5));
    let mut paired_speedups: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(5));
    for order in orders {
        let mut round_times = [Duration::ZERO; 4];
        for decoder_index in order {
            let start = Instant::now();
            for _ in 0..repeats[decoder_index] {
                for image in images {
                    decode(image, decoders[decoder_index]);
                }
            }
            let per_pass = start.elapsed().as_secs_f64() / repeats[decoder_index] as f64;
            let normalized = Duration::from_secs_f64(per_pass);
            samples[decoder_index].push(normalized);
            round_times[decoder_index] = normalized;
        }
        paired_speedups[0].push(round_times[0].as_secs_f64() / round_times[2].as_secs_f64());
        paired_speedups[1].push(round_times[3].as_secs_f64() / round_times[2].as_secs_f64());
    }
    summarize(samples, paired_speedups)
}

fn measure_reused(images: &[&Image], decoders: [Decoder; 4]) -> ([Timing; 4], [[f64; 3]; 2]) {
    let output_size = images
        .iter()
        .map(|image| image.pixel_bytes)
        .max()
        .expect("nonempty benchmark corpus");
    let mut output = vec![0; output_size];
    for decoder in decoders {
        for image in images {
            decode_reused(image, decoder, &mut output);
        }
    }

    let target_sample_seconds = 2.0;
    let repeats = decoders.map(|decoder| {
        let pass = time_reused_pass(images, decoder, &mut output).as_secs_f64();
        (target_sample_seconds / pass).ceil().max(1.0) as usize
    });
    let orders = [
        [0, 1, 2, 3],
        [1, 2, 3, 0],
        [2, 3, 0, 1],
        [3, 0, 1, 2],
        [0, 2, 1, 3],
    ];
    let mut samples: [Vec<Duration>; 4] = std::array::from_fn(|_| Vec::with_capacity(5));
    let mut paired_speedups: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(5));
    for order in orders {
        let mut round_times = [Duration::ZERO; 4];
        for decoder_index in order {
            let start = Instant::now();
            for _ in 0..repeats[decoder_index] {
                for image in images {
                    decode_reused(image, decoders[decoder_index], &mut output);
                }
            }
            let per_pass = start.elapsed().as_secs_f64() / repeats[decoder_index] as f64;
            let normalized = Duration::from_secs_f64(per_pass);
            samples[decoder_index].push(normalized);
            round_times[decoder_index] = normalized;
        }
        paired_speedups[0].push(round_times[0].as_secs_f64() / round_times[2].as_secs_f64());
        paired_speedups[1].push(round_times[3].as_secs_f64() / round_times[2].as_secs_f64());
    }
    summarize(samples, paired_speedups)
}

#[derive(Clone, Copy)]
enum Encoder {
    Qoi,
    Drishy,
    Restart,
}

const ENCODERS: [Encoder; 3] = [Encoder::Qoi, Encoder::Drishy, Encoder::Restart];

fn encode_one(
    image: &Image,
    pixels: &[u8],
    encoder: Encoder,
    block_pixels: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let encoded = match encoder {
        Encoder::Qoi => qoi::encode_to_vec(pixels, image.width, image.height)?,
        Encoder::Drishy => {
            encode::encode_rgb_or_rgba(image.width, image.height, image.channels, pixels)?
        }
        Encoder::Restart => restart_qoi::encode_with_block_pixels(
            decode::Header {
                width: image.width,
                height: image.height,
                channels: image.channels,
                colorspace: image.colorspace,
            },
            pixels,
            block_pixels,
        )?,
    };
    Ok(encoded)
}

fn validate_encoders(
    images: &[&Image],
    block_pixels: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    for image in images {
        let (_, expected) = qoi::decode_to_vec(&image.qoi)?;
        for encoder in ENCODERS {
            let encoded = encode_one(image, &expected, encoder, block_pixels)?;
            let actual = match encoder {
                Encoder::Qoi => qoi::decode_to_vec(&encoded)?.1,
                Encoder::Drishy => decode::decode(&encoded)?.1,
                Encoder::Restart => restart_qoi::decode(&encoded)?.1,
            };
            ensure_equal(&image.path, "encoder round trip", &expected, &actual)?;
        }
    }
    Ok(())
}

fn time_encoder_pass(
    images: &[&Image],
    encoder: Encoder,
    block_pixels: usize,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let mut elapsed = Duration::ZERO;
    for image in images {
        let (_, pixels) = qoi::decode_to_vec(black_box(&image.qoi))?;
        let start = Instant::now();
        black_box(encode_one(
            image,
            black_box(&pixels),
            encoder,
            block_pixels,
        )?);
        elapsed += start.elapsed();
    }
    Ok(elapsed)
}

fn measure_encoders(
    images: &[&Image],
    block_pixels: usize,
) -> Result<([Timing; 3], [u64; 3]), Box<dyn std::error::Error>> {
    validate_encoders(images, block_pixels)?;
    let target_sample_seconds = 1.0;
    let mut repeats = [1; 3];
    for (index, encoder) in ENCODERS.into_iter().enumerate() {
        let pass = time_encoder_pass(images, encoder, block_pixels)?.as_secs_f64();
        repeats[index] = (target_sample_seconds / pass).ceil().max(1.0) as usize;
    }
    let mut samples: [Vec<Duration>; 3] = std::array::from_fn(|_| Vec::with_capacity(5));
    let mut sizes = [0_u64; 3];

    // Decode each input once per round, then rotate encoder order per round to reduce order bias.
    for round in 0..5 {
        let mut round_times = [Duration::ZERO; 3];
        for image in images {
            let (_, decoded_pixels) = qoi::decode_to_vec(black_box(&image.qoi))?;
            for offset in 0..3 {
                let encoder_index = (offset + round) % 3;
                let encoder = ENCODERS[encoder_index];
                let start = Instant::now();
                for repeat_index in 0..repeats[encoder_index] {
                    let encoded =
                        encode_one(image, black_box(&decoded_pixels), encoder, block_pixels)?;
                    if round == 0 && repeat_index == 0 {
                        sizes[encoder_index] += encoded.len() as u64;
                    }
                    black_box(encoded);
                }
                round_times[encoder_index] += start.elapsed();
            }
        }
        for encoder_index in 0..3 {
            let normalized = Duration::from_secs_f64(
                round_times[encoder_index].as_secs_f64() / repeats[encoder_index] as f64,
            );
            samples[encoder_index].push(normalized);
        }
    }

    let timings = samples.map(|mut values| {
        values.sort_by_key(Duration::as_nanos);
        Timing {
            median: values[2],
            fastest: values[0],
            slowest: values[4],
        }
    });
    Ok((timings, sizes))
}

fn report_size_bands(images: &[&Image]) {
    let bands = [
        ("small (<65,536 px)", 0, 65_536),
        ("medium (65,536–1,048,575 px)", 65_536, 1_048_576),
        ("large (>=1,048,576 px)", 1_048_576, usize::MAX),
    ];
    let decoders = [
        Decoder::Qoi,
        Decoder::Drishy,
        Decoder::Restart,
        Decoder::Rapid,
    ];
    println!("Decode speed by image size (fresh output allocation):");
    for (label, min_pixels, max_pixels) in bands {
        let group: Vec<&Image> = images
            .iter()
            .copied()
            .filter(|image| image.pixels >= min_pixels && image.pixels < max_pixels)
            .collect();
        if group.is_empty() {
            println!("  {label}: no images");
            continue;
        }
        let group_pixels: u64 = group.iter().map(|image| image.pixels as u64).sum();
        let (times, _) = measure(&group, decoders, 0.75);
        let rates = times.map(|time| group_pixels as f64 / time.median.as_secs_f64() / 1e6);
        println!(
            "  {label}: {} images, {:.1} Mpix; qoi {:.0}, Drishy QOI {:.0}, rapid-qoi {:.0}, DRQ1 {:.0} Mpix/s ({:.2}x vs qoi, {:.2}x vs rapid-qoi)",
            group.len(),
            group_pixels as f64 / 1e6,
            rates[0],
            rates[1],
            rates[3],
            rates[2],
            rates[2] / rates[0],
            rates[2] / rates[3]
        );
    }
}

fn report_slowest_large_images(images: &[&Image]) {
    let mut slow_cases = Vec::new();
    for (image_index, image) in images
        .iter()
        .copied()
        .filter(|image| image.pixels >= 1_048_576)
        .enumerate()
    {
        // Warm both paths before timing; alternate order to reduce systematic bias.
        decode(image, Decoder::Qoi);
        decode(image, Decoder::Restart);
        let qoi_calibration = Instant::now();
        decode(image, Decoder::Qoi);
        let qoi_calibration = qoi_calibration.elapsed().as_secs_f64();
        let restart_calibration = Instant::now();
        decode(image, Decoder::Restart);
        let restart_calibration = restart_calibration.elapsed().as_secs_f64();
        let target_sample_seconds = 0.005;
        let qoi_repeats = (target_sample_seconds / qoi_calibration).ceil().max(1.0) as usize;
        let restart_repeats = (target_sample_seconds / restart_calibration)
            .ceil()
            .max(1.0) as usize;
        let mut qoi_samples = [Duration::ZERO; 3];
        let mut restart_samples = [Duration::ZERO; 3];
        for round in 0..3 {
            let order = if (image_index + round) % 2 == 0 {
                [Decoder::Qoi, Decoder::Restart]
            } else {
                [Decoder::Restart, Decoder::Qoi]
            };
            for decoder in order {
                let start = Instant::now();
                let repeats = match decoder {
                    Decoder::Qoi => qoi_repeats,
                    Decoder::Restart => restart_repeats,
                    Decoder::Drishy | Decoder::Rapid => unreachable!(),
                };
                for _ in 0..repeats {
                    decode(image, decoder);
                }
                let sample =
                    Duration::from_secs_f64(start.elapsed().as_secs_f64() / repeats as f64);
                match decoder {
                    Decoder::Qoi => qoi_samples[round] = sample,
                    Decoder::Restart => restart_samples[round] = sample,
                    Decoder::Drishy | Decoder::Rapid => unreachable!(),
                }
            }
        }
        qoi_samples.sort_by_key(Duration::as_nanos);
        restart_samples.sort_by_key(Duration::as_nanos);
        let qoi_time = qoi_samples[1].as_secs_f64();
        let restart_time = restart_samples[1].as_secs_f64();
        slow_cases.push((
            qoi_time / restart_time,
            image,
            image.pixels as f64 / qoi_time / 1e6,
            image.pixels as f64 / restart_time / 1e6,
        ));
    }
    slow_cases.sort_by(|left, right| left.0.total_cmp(&right.0));
    println!("Five weakest large-image cases (median of three warmed samples, >=5 ms each):");
    for (speedup, image, qoi_rate, restart_rate) in slow_cases.iter().take(5) {
        println!(
            "  {} ({}x{}, {} pixels): qoi {:.0}, DRQ1 {:.0} Mpix/s ({:.2}x)",
            image.path.file_name().unwrap_or_default().to_string_lossy(),
            image.width,
            image.height,
            image.pixels,
            qoi_rate,
            restart_rate,
            speedup,
        );
    }
}

fn report_encoders(
    images: &[&Image],
    pixels: u64,
    block_pixels: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let (times, sizes) = measure_encoders(images, block_pixels)?;
    let rates = times.map(|time| pixels as f64 / time.median.as_secs_f64() / 1e6);
    let bounds = times.map(|time| {
        (
            pixels as f64 / time.slowest.as_secs_f64() / 1e6,
            pixels as f64 / time.fastest.as_secs_f64() / 1e6,
        )
    });
    println!(
        "Encoder speed: original qoi {:.0} [{:.0}–{:.0}], Drishy QOI {:.0} [{:.0}–{:.0}], DRQ1 {:.0} [{:.0}–{:.0}] Mpix/s",
        rates[0],
        bounds[0].0,
        bounds[0].1,
        rates[1],
        bounds[1].0,
        bounds[1].1,
        rates[2],
        bounds[2].0,
        bounds[2].1,
    );
    println!(
        "Encoded output sizes: original qoi {:.2} GB; Drishy QOI {:.2} GB ({:.1}% of original qoi); DRQ1 {:.2} GB ({:.1}% of original qoi)",
        sizes[0] as f64 / 1e9,
        sizes[1] as f64 / 1e9,
        sizes[1] as f64 * 100.0 / sizes[0] as f64,
        sizes[2] as f64 / 1e9,
        sizes[2] as f64 * 100.0 / sizes[0] as f64,
    );
    println!(
        "Encoder timing: five paired rounds, each of at least one second per encoder; PNG/QOI input decoding is excluded."
    );
    Ok(())
}

fn benchmark(
    path: &Path,
    block_pixels: usize,
    encoding_only: bool,
    slow_cases_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    collect_paths(path, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no .png or .qoi files under {}", path.display()).into());
    }

    let mut images = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        if let Some(image) = prepare(path, block_pixels)? {
            images.push(image);
        }
        if (i + 1) % 250 == 0 || i + 1 == paths.len() {
            eprintln!("Prepared {} of {} images", i + 1, paths.len());
        }
    }
    let pixels: u64 = images.iter().map(|image| image.pixels as u64).sum();
    let qoi_size: u64 = images.iter().map(|image| image.qoi.len() as u64).sum();
    let restart_size: u64 = images.iter().map(|image| image.restart.len() as u64).sum();
    let parallel_eligible = images
        .iter()
        .filter(|image| {
            image.pixels >= restart_qoi::PARALLEL_MIN_PIXELS
                && image.pixels.div_ceil(block_pixels) > 1
        })
        .count();
    let parallel_pixels: u64 = images
        .iter()
        .filter(|image| {
            image.pixels >= restart_qoi::PARALLEL_MIN_PIXELS
                && image.pixels.div_ceil(block_pixels) > 1
        })
        .map(|image| image.pixels as u64)
        .sum();
    println!(
        "{} images, {:.3} Gpixels; restart block size = {} pixels",
        images.len(),
        pixels as f64 / 1e9,
        block_pixels
    );
    println!(
        "Parallel path: {} of {} images ({:.1}% of pixels); the remaining pixels use the serial path",
        parallel_eligible,
        images.len(),
        parallel_pixels as f64 * 100.0 / pixels as f64
    );
    println!(
        "Encoded bytes: QOI {:.2} GB; restart {:.2} GB ({:.1}% of QOI size)",
        qoi_size as f64 / 1e9,
        restart_size as f64 / 1e9,
        restart_size as f64 * 100.0 / qoi_size as f64
    );
    let largest_output_bytes = images.iter().map(|image| image.pixel_bytes).max().unwrap();
    println!(
        "Largest single decoded pixel buffer: {:.1} MiB",
        largest_output_bytes as f64 / (1024.0 * 1024.0)
    );

    let image_refs: Vec<&Image> = images.iter().collect();
    if encoding_only {
        return report_encoders(&image_refs, pixels, block_pixels);
    }
    if slow_cases_only {
        report_slowest_large_images(&image_refs);
        return Ok(());
    }

    // Compare first calls on the same image. The restart call includes Rayon pool startup.
    let largest = images.iter().max_by_key(|image| image.pixels).unwrap();
    let qoi_start = Instant::now();
    let (qoi_header, expected) = qoi::decode_to_vec(&largest.qoi)?;
    let qoi_first_time = qoi_start.elapsed();
    let cold_start = Instant::now();
    let (header, actual) = restart_qoi::decode(&largest.restart)?;
    let cold_time = cold_start.elapsed();
    ensure_equal(&largest.path, "first restart decode", &expected, &actual)?;
    println!(
        "First decode of largest image {}x{}: original qoi {:.3} s; restart {:.3} s ({:.2}x, pool startup included)",
        qoi_header.width,
        qoi_header.height,
        qoi_first_time.as_secs_f64(),
        cold_time.as_secs_f64(),
        qoi_first_time.as_secs_f64() / cold_time.as_secs_f64(),
    );
    println!(
        "First restart throughput: {:.0} Mpix/s",
        largest.pixels as f64 / cold_time.as_secs_f64() / 1e6
    );
    if header.width != qoi_header.width
        || header.height != qoi_header.height
        || header.channels != qoi_header.channels.into()
        || header.colorspace != qoi_header.colorspace.into()
    {
        return Err(format!("wrong dimensions for {}", largest.path.display()).into());
    }

    // Cross-check every block-format output against the independent QOI decoder.
    for image in &images {
        let expected = qoi::decode_to_vec(&image.qoi)?.1;
        let (header, actual) = restart_qoi::decode(&image.restart)?;
        if header.width != image.width || header.height != image.height {
            return Err(format!("wrong dimensions for {}", image.path.display()).into());
        }
        ensure_equal(&image.path, "restart decoder", &expected, &actual)?;
        let mut reused = vec![0; image.pixel_bytes];
        restart_qoi::decode_into(&image.restart, &mut reused)?;
        ensure_equal(&image.path, "restart reused output", &expected, &reused)?;
    }

    let decoders = [
        Decoder::Qoi,
        Decoder::Drishy,
        Decoder::Restart,
        Decoder::Rapid,
    ];
    let (times, speedups) = measure(&image_refs, decoders, 2.0);
    let rates = times.map(|time| pixels as f64 / time.median.as_secs_f64() / 1e6);
    let bounds = times.map(|time| {
        (
            pixels as f64 / time.slowest.as_secs_f64() / 1e6,
            pixels as f64 / time.fastest.as_secs_f64() / 1e6,
        )
    });
    println!(
        "Original qoi: {:.0} Mpix/s [{:.0}–{:.0}]",
        rates[0], bounds[0].0, bounds[0].1
    );
    println!(
        "Drishy standard QOI: {:.0} Mpix/s [{:.0}–{:.0}]",
        rates[1], bounds[1].0, bounds[1].1
    );
    println!(
        "Rapid QOI: {:.0} Mpix/s [{:.0}–{:.0}]",
        rates[3], bounds[3].0, bounds[3].1
    );
    println!(
        "Drishy restart blocks: {:.0} Mpix/s [{:.0}–{:.0}]; vs qoi {:.2}x (paired {:.2}x, {:.2}–{:.2}); vs rapid-qoi {:.2}x (paired {:.2}x, {:.2}–{:.2})",
        rates[2],
        bounds[2].0,
        bounds[2].1,
        rates[2] / rates[0],
        speedups[0][0],
        speedups[0][1],
        speedups[0][2],
        rates[2] / rates[3],
        speedups[1][0],
        speedups[1][1],
        speedups[1][2],
    );
    println!(
        "Timing: five paired rounds of at least two seconds per decoder; input bytes are in memory."
    );
    report_size_bands(&image_refs);
    report_slowest_large_images(&image_refs);

    let (reused_times, reused_speedups) = measure_reused(&image_refs, decoders);
    let reused_rates = reused_times.map(|time| pixels as f64 / time.median.as_secs_f64() / 1e6);
    println!(
        "Reused buffers: qoi {:.0}, Drishy standard {:.0}, rapid-qoi {:.0}, restart {:.0} Mpix/s; restart vs qoi {:.2}x (paired {:.2}x, {:.2}–{:.2}); vs rapid-qoi {:.2}x (paired {:.2}x, {:.2}–{:.2})",
        reused_rates[0],
        reused_rates[1],
        reused_rates[3],
        reused_rates[2],
        reused_rates[2] / reused_rates[0],
        reused_speedups[0][0],
        reused_speedups[0][1],
        reused_speedups[0][2],
        reused_rates[2] / reused_rates[3],
        reused_speedups[1][0],
        reused_speedups[1][1],
        reused_speedups[1][2],
    );
    println!(
        "Reusable-buffer timing: five paired rounds of at least two seconds per decoder; one output buffer is reused across all images."
    );
    report_encoders(&image_refs, pixels, block_pixels)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args: Vec<_> = env::args_os().skip(1).collect();
    let encoding_only = raw_args.iter().any(|arg| arg == "--encoding-only");
    let slow_cases_only = raw_args.iter().any(|arg| arg == "--slow-cases-only");
    let args: Vec<_> = raw_args
        .into_iter()
        .filter(|arg| arg != "--bench" && arg != "--encoding-only" && arg != "--slow-cases-only")
        .collect();
    if args.is_empty() {
        eprintln!(
            "Usage: cargo bench --bench restart_qoi -- <image.qoi|image.png|corpus-directory> [block-pixels] [--encoding-only|--slow-cases-only]"
        );
        std::process::exit(2);
    }
    let block_pixels = args
        .get(1)
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(restart_qoi::DEFAULT_BLOCK_PIXELS);
    benchmark(
        &PathBuf::from(&args[0]),
        block_pixels,
        encoding_only,
        slow_cases_only,
    )
}
