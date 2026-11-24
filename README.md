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

## Fuzz decoder inputs

Install `cargo-fuzz` once, then run either bounded decoder target with Rust nightly:

```sh
cargo install cargo-fuzz
mkdir -p fuzz/corpus/qoi_decode fuzz/corpus/restart_decode
cp -R fuzz/seed-corpus/qoi/. fuzz/corpus/qoi_decode/
cp -R fuzz/seed-corpus/restart/. fuzz/corpus/restart_decode/
cargo +nightly fuzz run qoi_decode -- -max_total_time=60
cargo +nightly fuzz run restart_decode -- -max_total_time=60
```

The targets cover malformed files and mutate valid small streams to reach deeper opcode handling. They cap input and decoded image sizes so fuzzing does not allocate large image buffers.

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

The runner saves a JSON summary and a companion `.samples.jsonl` file under `benchmark-results/`. Together they contain every raw sample, per-run summaries, the source commit and working-tree state, OS/CPU/Rust details, Rayon thread count, and a SHA-256 fingerprint of the sorted corpus paths and file contents. `--mode encode` isolates encoder timing; `--mode full` also measures decoding. Connect power and close CPU-heavy apps before a series, and keep the power mode and thread count the same across repeats. The benchmark prints raw rows directly with `--raw-samples` if you want to run it without the report script.

### Current result (2026-11-24)

Two independent full runs on an Apple M5 (10 cores, 10 Rayon threads) on commit `373166e` covered 2,848 PNGs and 1,321,975,407 pixels. Across 10 paired samples, allocating decode measured a median **2,042 Mpix/s** for DRQ1, **534** for `qoi`, and **520** for `rapid-qoi`. The median paired speedup was **3.78×** vs `qoi` (sample range 2.66×–4.07×) and **3.86×** vs `rapid-qoi` (3.10×–4.52×). By image size, DRQ1 was **0.96×** on small images, **3.56×** on medium images, and **4.64×** on large images vs `qoi`. With a reused output buffer, DRQ1 measured **2,068 Mpix/s** (4.05× vs `qoi`, 4.10× vs `rapid-qoi`). The encoder measured **660 Mpix/s**, 1.84× the `qoi` reference. Output size was **100.1%** of QOI, and parallel decoding covered 99.3% of pixels.

The downloaded official archive is about 1.1 GB. Although the benchmark page lists 2,879 images, the archive yielded 2,848 PNGs plus support files. The PNG set has the same SHA-256 fingerprint as the earlier report, so it can be copied to another computer for an exact corpus match. The report records Rust 1.93.0, the source commit, machine details, all samples, and the corpus fingerprint. See the [cross-computer benchmark guide](docs/cross-computer-benchmark.md) before repeating these results elsewhere.

The [JSON summary](benchmark-results/drishy-20261124T155944Z.json) and [raw samples](benchmark-results/drishy-20261124T155944Z.samples.jsonl) record the current baseline. The corpus SHA-256 is `1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7`. Earlier process memory measurements were whole-benchmark peaks, not single-image decoder memory: 1.8 GiB for encoder-only and 3.8 GiB for the full benchmark. A separate official C benchmark run measured 287 Mpix/s (an earlier run measured 443); its streams and run conditions differ, so do not treat that as a paired comparison.
