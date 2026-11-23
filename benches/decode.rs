use drishy::decode;
use std::{
    env, fs,
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

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

fn benchmark(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let (header, expected) = decode::decode(&bytes)?;
    let qoi_pixels = qoi::decode_to_vec(&bytes)?.1;
    let rapid_pixels = rapid_qoi::Qoi::decode_alloc(&bytes)?.1;
    assert_eq!(
        expected,
        qoi_pixels,
        "qoi crate output differs for {}",
        path.display()
    );
    assert_eq!(
        expected,
        rapid_pixels,
        "rapid-qoi output differs for {}",
        path.display()
    );

    let drishy_time = measure(|| {
        black_box(decode::decode(black_box(&bytes)).expect("valid benchmark image"));
    });
    let qoi_time = measure(|| {
        black_box(qoi::decode_to_vec(black_box(&bytes)).expect("valid benchmark image"));
    });
    let rapid_time = measure(|| {
        black_box(rapid_qoi::Qoi::decode_alloc(black_box(&bytes)).expect("valid benchmark image"));
    });
    let megapixels = f64::from(header.width) * f64::from(header.height) / 1_000_000.0;
    println!(
        "{} ({}x{}): Drishy {:.0} Mp/s, qoi {:.0} Mp/s ({:.2}x), rapid-qoi {:.0} Mp/s ({:.2}x)",
        path.display(),
        header.width,
        header.height,
        megapixels / drishy_time.as_secs_f64(),
        megapixels / qoi_time.as_secs_f64(),
        drishy_time.as_secs_f64() / qoi_time.as_secs_f64(),
        megapixels / rapid_time.as_secs_f64(),
        drishy_time.as_secs_f64() / rapid_time.as_secs_f64(),
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = env::args_os()
        .skip(1)
        .filter(|path| path != "--bench")
        .collect();
    if paths.is_empty() {
        eprintln!("Usage: cargo bench --bench decode -- <image.qoi> [image.qoi ...]");
        std::process::exit(2);
    }
    for path in paths {
        benchmark(Path::new(&path))?;
    }
    Ok(())
}
