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
