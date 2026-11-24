# Drishy

Drishy is a Rust QOI image encoder and decoder. It converts PNG and QOI files, supports indexed parallel decoding of standard QOI, and includes an experimental restart-block format for first-pass parallel decoding.

## Use

```sh
cargo run --release -- encode input.png output.qoi
cargo run --release -- decode input.qoi output.png
cargo run --release -- encode-restart input.png output.drq
cargo run --release -- decode-restart input.drq output.png
cargo run --release -- index input.qoi input.qoi.idx
cargo run --release -- decode-indexed input.qoi input.qoi.idx output.png
```

The `.qoi` commands read and write the standard QOI format. The `.drq` commands use Drishy's experimental `DRQ1` format, which resets QOI decoding state every 16,384 pixels so CPU workers can decode the blocks independently. Images with at least 65,536 pixels are eligible for parallel decoding. `.drq` files are not QOI-compatible; compare both their decode speed and file size before choosing them.

`DRQ1` has a 22-byte header with the dimensions, channel count, colorspace, block size, and block count. A directory stores each block's byte length, followed by the block payloads. Each payload contains standard QOI pixel opcodes and starts with a fresh previous pixel and color-index table. Blocks correspond to consecutive pixels in row-major order, so each worker writes one contiguous section of the output buffer.

## Check

```sh
cargo test
```

## Benchmark

```sh
cargo bench --bench decode -- path/to/image.qoi
cargo bench --bench decode -- path/to/qoi_benchmark_suite
cargo bench --bench restart_qoi -- path/to/qoi_benchmark_suite
cargo bench --bench restart_qoi -- path/to/qoi_benchmark_suite 65536
cargo bench --bench restart_qoi -- path/to/sipi_png --encoding-only
cargo bench --bench restart_qoi -- path/to/qoi_benchmark_suite --slow-cases-only
```

The corpus run checks decoded pixels against two independent Rust decoders, then reports allocating and reused-output-buffer throughput. The restart-block run also reports speed by image size, the five weakest large-image cases, and encoder throughput. `--encoding-only` skips decoder timing; `--slow-cases-only` skips the suite-wide timed runs. Encoder timing excludes reading and decoding source images. The official suite is available from [qoiformat.org](https://qoiformat.org/benchmark/qoi_benchmark_suite.tar).

The restart-block benchmark checks decoded outputs against independent QOI decoders, reports compressed sizes, times first decodes, and compares five paired rounds with observed ranges. It reports both fresh output allocation and a reused output buffer. Its optional final argument selects the block size in pixels; the default is 16,384. Timings decode in-memory bytes and include block-directory parsing, output allocation, and parallel scheduling; file reads and PNG conversion are outside the timed region.

### Repeatable benchmark report

```sh
python3 scripts/repeat_benchmark.py /path/to/qoi_benchmark_suite --runs 3 --threads 10
python3 scripts/repeat_benchmark.py /path/to/qoi_benchmark_suite --mode encode --runs 3 --threads 10
```

The runner saves a JSON report under `benchmark-results/` with every raw sample, per-run summaries, the source commit and working-tree state, OS/CPU/Rust details, Rayon thread count, and a SHA-256 fingerprint of the sorted corpus paths and file contents. `--mode encode` isolates encoder timing; `--mode full` also measures decoding. Connect power and close CPU-heavy apps before a series, and keep the power mode and thread count the same across repeats. The benchmark prints raw rows directly with `--raw-samples` if you want to run it without the report script.

### Current result (2026-11-24)

Two independent full runs on an Apple M5 (10 cores, 10 Rayon threads) covered 2,848 PNGs and 1,321,975,407 pixels. Across the 10 paired samples, allocating decode measured a median **1,894 Mpix/s** for DRQ1, **527** for `qoi`, and **533** for `rapid-qoi`. The median paired speedup was **3.72×** vs `qoi` (sample range 3.13×–4.90×) and **3.92×** vs `rapid-qoi` (2.99×–4.65×). By image size, DRQ1 was **0.96×** on small images, **3.61×** on medium images, and **4.84×** on large images vs `qoi`. With a reused output buffer, DRQ1 measured **2,267 Mpix/s** (4.11× vs `qoi`, 4.19× vs `rapid-qoi`). Output size was **100.1%** of QOI, and parallel decoding covered 99.3% of pixels.

The bounded-batch parallel encoder measured a median **640 Mpix/s** vs **356** for reference `qoi` across 10 samples (**1.83×** paired median; range 1.72×–1.91×). DRQ1 output remained **100.1%** of QOI size. On the separate [USC-SIPI collection](https://sipi.usc.edu/database/?volume=misc), a prior run across 39 images measured 383 Mpix/s vs 266 for `qoi`; that smaller corpus has not yet had repeated runs.

The [full JSON report](benchmark-results/drishy-20261124T133224Z.json) records every timed sample, code revision, CPU/OS/Rust details, thread count, and the corpus SHA-256 fingerprint (`1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7`). The official QOI archive page lists 2,879 images, but this downloaded set contains 2,848; these results apply to the fingerprinted local set. Earlier process memory measurements were whole-benchmark peaks, not single-image decoder memory: 1.8 GiB for encoder-only and 3.8 GiB for the full benchmark. A separate official C benchmark run measured 287 Mpix/s (an earlier run measured 443); its streams and run conditions differ, so do not treat that as a paired comparison.
