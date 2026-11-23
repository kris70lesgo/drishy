# Drishy

Drishy is a Rust QOI image encoder and decoder. It converts PNG and QOI files and supports indexed, parallel decoding of QOI images.

## Use

```sh
cargo run --release -- encode input.png output.qoi
cargo run --release -- decode input.qoi output.png
cargo run --release -- index input.qoi input.qoi.idx
cargo run --release -- decode-indexed input.qoi input.qoi.idx output.png
```

## Check

```sh
cargo test
```

## Benchmark

```sh
cargo bench --bench decode -- path/to/image.qoi
cargo bench --bench decode -- path/to/qoi_benchmark_suite
```

The corpus run checks decoded pixels against two independent Rust decoders, then reports serial and indexed throughput. It separates index build and validation time from repeated decoding. Use `IndexedDecoder::build` for a fresh index or `IndexedDecoder::new` to validate a saved index. The official suite is available from [qoiformat.org](https://qoiformat.org/benchmark/qoi_benchmark_suite.tar).
