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

### Current result

On an Apple M5 with 10 CPU cores, the downloaded suite's 2,848 PNGs (1.322 billion pixels) measured **2,222 Mpix/s** for DRQ1, **572** for `qoi`, and **570** for `rapid-qoi`: **3.89×** and **3.90×** the baseline median throughput. Across five paired runs, DRQ1 stayed 3.66×–4.21× vs `qoi` and 3.76×–4.15× vs `rapid-qoi`. By image size, DRQ1 was **0.96×** as fast on small images, **3.59×** on medium images, and **5.19×** on large images vs `qoi`. With a reused output buffer, it reached **2,567 Mpix/s** (4.34× vs `qoi`). DRQ1 output was **100.1%** of QOI size.

Separate corpus: the [USC-SIPI collection](https://sipi.usc.edu/database/?volume=misc) supplied 39 available TIFFs (the collection page lists 44 and notes five are no longer distributed); they were converted to PNG before timing. These 39 filenames do not occur in the QOI suite. Across 10.0 million pixels, DRQ1 reached 1,527 Mpix/s vs 483 for `qoi` and 465 for `rapid-qoi` (3.16×/3.28×); medium images were 2.77× faster and the three large images 4.64×. This is encouraging evidence from a second corpus, with modest corpus size. Encoder-only main-suite results: `qoi` 413 Mpix/s, Drishy QOI 210, DRQ1 209; output sizes were 100.0% and 100.1% of reference QOI size. The benchmark process peaked at **3.1 GiB** resident memory because it retains both compressed versions of the whole suite; this is not single-decode memory. The largest output pixel buffer was 103.5 MiB. The benchmark page lists 2,879 images while this archive contains 2,848 PNGs; the difference is unresolved, so results are reported for the 2,848 local files only.
