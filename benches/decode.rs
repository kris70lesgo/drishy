use drishy::{
    decode, encode,
    index::DecodeIndex,
    parallel::{self, IndexedDecoder},
};
use std::{
    env, fs,
    hint::black_box,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct BenchImage {
    path: PathBuf,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    opcodes: [u64; 6],
    indexes: Vec<(usize, DecodeIndex)>,
}

fn opcode_counts(bytes: &[u8]) -> [u64; 6] {
    let mut counts = [0; 6];
    let mut offset = decode::HEADER_SIZE;
    let end = bytes.len().saturating_sub(decode::END_MARKER.len());
    while offset < end {
        let tag = bytes[offset];
        offset += 1;
        match tag {
            0xfe => {
                counts[4] += 1;
                offset += 3;
            }
            0xff => {
                counts[5] += 1;
                offset += 4;
            }
            _ => match tag >> 6 {
                0 => counts[0] += 1,
                1 => counts[1] += 1,
                2 => {
                    counts[2] += 1;
                    offset += 1;
                }
                _ => counts[3] += 1,
            },
        }
    }
    counts
}

fn ensure_equal(
    path: &Path,
    decoder: &str,
    expected: &[u8],
    actual: &[u8],
) -> Result<(), io::Error> {
    if expected == actual {
        return Ok(());
    }
    let mismatch = expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
        .unwrap_or(expected.len().min(actual.len()));
    Err(io::Error::other(format!(
        "{decoder} output differs for {} at byte {mismatch} ({} vs {} bytes)",
        path.display(),
        expected.len(),
        actual.len()
    )))
}

fn prepare(path: &Path) -> Result<Option<BenchImage>, Box<dyn std::error::Error>> {
    let (bytes, expected) = match path.extension().and_then(|extension| extension.to_str()) {
        Some("qoi") => {
            let bytes = fs::read(path)?;
            let (_, pixels) = decode::decode(&bytes)?;
            (bytes, pixels)
        }
        Some("png") => {
            let reader = image::ImageReader::open(path)?.with_guessed_format()?;
            let source = match reader.decode() {
                Ok(source) => source,
                Err(_) => return Ok(None),
            };
            let (bytes, expected) = if source.color().has_alpha() {
                let pixels = source.into_rgba8();
                let bytes = encode::encode_rgb_or_rgba(
                    pixels.width(),
                    pixels.height(),
                    4,
                    pixels.as_raw(),
                )?;
                (bytes, pixels.into_raw())
            } else {
                let pixels = source.into_rgb8();
                let bytes = encode::encode_rgb_or_rgba(
                    pixels.width(),
                    pixels.height(),
                    3,
                    pixels.as_raw(),
                )?;
                (bytes, pixels.into_raw())
            };
            (bytes, expected)
        }
        _ => return Err(format!("expected a .png or .qoi file: {}", path.display()).into()),
    };

    let (header, pixels) = decode::decode(&bytes)?;
    ensure_equal(path, "Drishy", &expected, &pixels)?;
    let (_, qoi_pixels) = qoi::decode_to_vec(&bytes)?;
    ensure_equal(path, "qoi", &expected, &qoi_pixels)?;
    let (_, rapid_pixels) = rapid_qoi::Qoi::decode_alloc(&bytes)?;
    ensure_equal(path, "rapid-qoi", &expected, &rapid_pixels)?;

    Ok(Some(BenchImage {
        path: path.to_path_buf(),
        opcodes: opcode_counts(&bytes),
        bytes,
        width: header.width,
        height: header.height,
        indexes: Vec::new(),
    }))
}

fn measure(mut decode: impl FnMut()) -> Duration {
    for _ in 0..3 {
        decode();
    }

    let calibration_start = Instant::now();
    let mut iterations = 0;
    while calibration_start.elapsed() < Duration::from_millis(100) {
        decode();
        iterations += 1;
    }

    let mut samples = Vec::with_capacity(5);
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..iterations {
            decode();
        }
        samples.push(start.elapsed().as_secs_f64() / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    Duration::from_secs_f64(samples[2])
}

fn measure_corpus(images: &[BenchImage], mut decode: impl FnMut(&[u8])) -> Duration {
    for image in images {
        decode(&image.bytes);
    }

    let mut samples = Vec::with_capacity(3);
    for _ in 0..3 {
        let start = Instant::now();
        for image in images {
            decode(&image.bytes);
        }
        samples.push(start.elapsed().as_secs_f64());
    }
    samples.sort_by(f64::total_cmp);
    Duration::from_secs_f64(samples[1])
}

fn measure_indexed_corpus(images: &[BenchImage], workers: usize) -> Duration {
    let decode_all = |image: &BenchImage| {
        let index = image
            .indexes
            .iter()
            .find(|(count, _)| *count == workers)
            .map(|(_, index)| index)
            .expect("index prepared for worker count");
        black_box(
            parallel::decode_indexed(black_box(&image.bytes), black_box(index))
                .expect("validated indexed benchmark image"),
        );
    };
    for image in images {
        decode_all(image);
    }

    let mut samples = Vec::with_capacity(3);
    for _ in 0..3 {
        let start = Instant::now();
        for image in images {
            decode_all(image);
        }
        samples.push(start.elapsed().as_secs_f64());
    }
    samples.sort_by(f64::total_cmp);
    Duration::from_secs_f64(samples[1])
}

fn bind_indexed_corpus<'a>(
    images: &'a [BenchImage],
    workers: usize,
) -> Result<(Vec<IndexedDecoder<'a>>, Duration), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let decoders = images
        .iter()
        .map(|image| {
            let index = image
                .indexes
                .iter()
                .find(|(count, _)| *count == workers)
                .map(|(_, index)| index)
                .expect("index prepared for worker count");
            IndexedDecoder::new(&image.bytes, index)
        })
        .collect::<Result<Vec<_>, drishy::decode::QoiError>>()?;
    Ok((decoders, start.elapsed()))
}

fn build_bound_indexed_corpus<'a>(
    images: &'a [BenchImage],
    workers: usize,
) -> Result<(Vec<IndexedDecoder<'a>>, Duration), Box<dyn std::error::Error>> {
    let mut build_time = Duration::ZERO;
    let mut decoders = Vec::with_capacity(images.len());
    for image in images {
        let start = Instant::now();
        let decoder = IndexedDecoder::build(&image.bytes, workers)?;
        build_time += start.elapsed();
        let (_, indexed_pixels) = decoder.decode()?;
        let (_, serial_pixels) = decode::decode(&image.bytes)?;
        ensure_equal(
            &image.path,
            "built indexed Drishy",
            &serial_pixels,
            &indexed_pixels,
        )?;
        decoders.push(decoder);
    }
    Ok((decoders, build_time))
}

fn measure_bound_indexed_corpus(decoders: &[IndexedDecoder<'_>]) -> Duration {
    let decode_all = |decoder: &IndexedDecoder<'_>| {
        black_box(decoder.decode().expect("validated indexed benchmark image"));
    };
    for decoder in decoders {
        decode_all(decoder);
    }

    let mut samples = Vec::with_capacity(3);
    for _ in 0..3 {
        let start = Instant::now();
        for decoder in decoders {
            decode_all(decoder);
        }
        samples.push(start.elapsed().as_secs_f64());
    }
    samples.sort_by(f64::total_cmp);
    Duration::from_secs_f64(samples[1])
}

fn print_results(
    name: &str,
    megapixels: f64,
    drishy_time: Duration,
    qoi_time: Duration,
    rapid_time: Duration,
) {
    println!(
        "{name}: Drishy {:.0} Mp/s, qoi {:.0} Mp/s ({:.2}x), rapid-qoi {:.0} Mp/s ({:.2}x)",
        megapixels / drishy_time.as_secs_f64(),
        megapixels / qoi_time.as_secs_f64(),
        drishy_time.as_secs_f64() / qoi_time.as_secs_f64(),
        megapixels / rapid_time.as_secs_f64(),
        drishy_time.as_secs_f64() / rapid_time.as_secs_f64(),
    );
}

fn benchmark_one(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let image = prepare(path)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("PNG could not be decoded: {}", path.display()),
        )
    })?;
    let drishy_time = measure(|| {
        black_box(decode::decode(black_box(&image.bytes)).expect("validated benchmark image"));
    });
    let qoi_time = measure(|| {
        black_box(qoi::decode_to_vec(black_box(&image.bytes)).expect("validated benchmark image"));
    });
    let rapid_time = measure(|| {
        black_box(
            rapid_qoi::Qoi::decode_alloc(black_box(&image.bytes))
                .expect("validated benchmark image"),
        );
    });
    let megapixels = f64::from(image.width) * f64::from(image.height) / 1_000_000.0;
    print_results(
        &image.path.display().to_string(),
        megapixels,
        drishy_time,
        qoi_time,
        rapid_time,
    );
    let workers = std::thread::available_parallelism().map_or(1, |count| count.get());
    let build_start = Instant::now();
    let indexed = IndexedDecoder::build(&image.bytes, workers)?;
    let build_time = build_start.elapsed();
    let indexed_pixels = indexed.decode()?.1;
    let (_, serial_pixels) = decode::decode(&image.bytes)?;
    ensure_equal(
        &image.path,
        "built indexed Drishy",
        &serial_pixels,
        &indexed_pixels,
    )?;
    let indexed_time = measure(|| {
        black_box(indexed.decode().expect("validated indexed benchmark image"));
    });
    println!(
        "Indexed Drishy ({workers} segments): {:.0} Mp/s bound repeated; {:.0} Mp/s including build + one decode ({:.3}s build)",
        megapixels / indexed_time.as_secs_f64(),
        megapixels / (indexed_time + build_time).as_secs_f64(),
        build_time.as_secs_f64(),
    );
    Ok(())
}

fn collect_paths(path: &Path, paths: &mut Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_paths(&entry?.path(), paths)?;
        }
    } else if matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("png" | "qoi")
    ) {
        paths.push(path.to_path_buf());
    }
    Ok(())
}

fn benchmark_corpus(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    collect_paths(root, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no .png or .qoi images found under {}", root.display()).into());
    }

    let mut images = Vec::with_capacity(paths.len());
    let mut skipped = 0;
    for (position, path) in paths.iter().enumerate() {
        match prepare(path)? {
            Some(image) => images.push(image),
            None => {
                eprintln!("Skipping unreadable PNG: {}", path.display());
                skipped += 1;
            }
        }
        if (position + 1) % 250 == 0 || position + 1 == paths.len() {
            eprintln!("Validated {} of {} images", position + 1, paths.len());
        }
    }

    let max_workers = std::thread::available_parallelism().map_or(1, |count| count.get());
    let mut worker_counts = vec![2, 4, 8, max_workers];
    worker_counts.sort_unstable();
    worker_counts.dedup();
    let mut index_build_times = Vec::with_capacity(worker_counts.len());
    for workers in worker_counts {
        let mut build_time = Duration::ZERO;
        for image in &mut images {
            let start = Instant::now();
            let index = DecodeIndex::build(&image.bytes, workers)?;
            build_time += start.elapsed();
            let (_, indexed_pixels) = parallel::decode_indexed(&image.bytes, &index)?;
            let (_, serial_pixels) = decode::decode(&image.bytes)?;
            ensure_equal(
                &image.path,
                "indexed Drishy",
                &serial_pixels,
                &indexed_pixels,
            )?;
            image.indexes.push((workers, index));
        }
        index_build_times.push((workers, build_time));
    }

    let total_pixels: u64 = images
        .iter()
        .map(|image| u64::from(image.width) * u64::from(image.height))
        .sum();
    let total_megapixels = total_pixels as f64 / 1_000_000.0;
    let qoi_bytes: u64 = images.iter().map(|image| image.bytes.len() as u64).sum();
    let mut opcodes = [0_u64; 6];
    for image in &images {
        for (total, count) in opcodes.iter_mut().zip(image.opcodes) {
            *total += count;
        }
    }
    let total_chunks: u64 = opcodes.iter().sum();
    println!(
        "Validated {} images ({} skipped), {:.2} Gpixels, {:.2} GB of QOI data",
        images.len(),
        skipped,
        total_pixels as f64 / 1_000_000_000.0,
        qoi_bytes as f64 / 1_000_000_000.0,
    );
    println!(
        "Chunk mix: INDEX {:.1}%, DIFF {:.1}%, LUMA {:.1}%, RUN {:.1}%, RGB {:.1}%, RGBA {:.1}%",
        opcodes[0] as f64 * 100.0 / total_chunks as f64,
        opcodes[1] as f64 * 100.0 / total_chunks as f64,
        opcodes[2] as f64 * 100.0 / total_chunks as f64,
        opcodes[3] as f64 * 100.0 / total_chunks as f64,
        opcodes[4] as f64 * 100.0 / total_chunks as f64,
        opcodes[5] as f64 * 100.0 / total_chunks as f64,
    );

    let drishy_time = measure_corpus(&images, |bytes| {
        black_box(decode::decode(black_box(bytes)).expect("validated benchmark image"));
    });
    let qoi_time = measure_corpus(&images, |bytes| {
        black_box(qoi::decode_to_vec(black_box(bytes)).expect("validated benchmark image"));
    });
    let rapid_time = measure_corpus(&images, |bytes| {
        black_box(
            rapid_qoi::Qoi::decode_alloc(black_box(bytes)).expect("validated benchmark image"),
        );
    });
    print_results(
        &format!("Full corpus, {} images", images.len()),
        total_megapixels,
        drishy_time,
        qoi_time,
        rapid_time,
    );
    for (workers, index_build_time) in index_build_times {
        let (bound_decoders, validation_time) = bind_indexed_corpus(&images, workers)?;
        let checked_time = measure_indexed_corpus(&images, workers);
        let indexed_time = measure_bound_indexed_corpus(&bound_decoders);
        println!(
            "Indexed Drishy ({workers} segments): {:.0} Mp/s bound repeated; {:.0} Mp/s with per-call validation; {:.0} Mp/s including build + bind + one decode ({:.2}s build, {:.2}s bind)",
            total_megapixels / indexed_time.as_secs_f64(),
            total_megapixels / checked_time.as_secs_f64(),
            total_megapixels / (indexed_time + index_build_time + validation_time).as_secs_f64(),
            index_build_time.as_secs_f64(),
            validation_time.as_secs_f64(),
        );
    }
    let (built_decoders, build_time) = build_bound_indexed_corpus(&images, max_workers)?;
    let decode_time = measure_bound_indexed_corpus(&built_decoders);
    println!(
        "Fused build + bound decode ({max_workers} segments): {:.0} Mp/s including build + one decode ({:.2}s build)",
        total_megapixels / (build_time + decode_time).as_secs_f64(),
        build_time.as_secs_f64(),
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = env::args_os()
        .skip(1)
        .filter(|path| path != "--bench")
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        eprintln!("Usage: cargo bench --bench decode -- <image.qoi|image.png|corpus-directory>");
        std::process::exit(2);
    }
    if paths.len() == 1 && paths[0].is_dir() {
        benchmark_corpus(&paths[0])
    } else {
        for path in paths {
            benchmark_one(&path)?;
        }
        Ok(())
    }
}
