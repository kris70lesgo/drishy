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

### Current result (2026-11-24)

On an Apple M5 with 10 CPU cores, the latest full run over 2,848 PNGs (1.322 billion pixels) measured **1,481 Mpix/s** for DRQ1, **402** for `qoi`, and **420** for `rapid-qoi`: **3.68×** and **3.53×**. The five paired rounds ranged 3.35×–3.89× vs `qoi` and 3.18×–3.64× vs `rapid-qoi`. By image size, DRQ1 was **0.98×** as fast on small images, **2.84×** on medium images, and **4.18×** on large images vs `qoi`. With a reused output buffer, it reached **1,532 Mpix/s** (3.51× vs `qoi`). DRQ1 output was **100.1%** of QOI size, and parallel decoding covered 99.3% of suite pixels.

Encoder work now splits restart blocks across CPU threads in bounded batches and estimates output capacity from the first encoded batch. On an encoding-only run, DRQ1 measured **396 Mpix/s** vs **244** for reference `qoi` (1.62×), with output still **100.1%** of QOI size. A second run in the full validation benchmark measured 471 vs 272 Mpix/s, but encoder timings varied widely; treat 396 Mpix/s as the conservative headline. On the separate [USC-SIPI collection](https://sipi.usc.edu/database/?volume=misc), 39 available images measured 383 Mpix/s for DRQ1 vs 266 for `qoi` (1.44×); this corpus is small.

For context, the official C QOI benchmark measured **287 Mpix/s** on this Mac in its latest run (an earlier separate run measured 443). It uses separately encoded QOI streams and is not a paired comparison with Drishy; the variation reinforces that these figures are context only. The `--encoding-only` benchmark process peaked at **1.8 GiB** resident memory; the full decode-and-encode benchmark peaked at **3.8 GiB**. These are whole-harness figures, not single-image decode memory. The largest decoded pixel buffer was 103.5 MiB. The official [QOI benchmark page](https://qoiformat.org/benchmark/) lists 2,879 images while the downloaded archive contains 2,848 PNGs; the difference is unresolved, so results cover the 2,848 local files.
