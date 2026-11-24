# Drishy: notes for a future blog

Working notes, not the blog draft. Keep the story honest as the design and results change.

## Project and approach

- Goal: build a Rust image decoder experiment that can use the CPU cores in an Apple MacBook to decode lossless images faster.
- Normal QOI decoding is sequential because each pixel can depend on the previous pixel and the 64-entry color cache. Research confirmed that independent restart points are a practical way to parallelize this kind of format. QOIR is prior art using independent 64×64 tiles; DRQ1 uses fixed-size row-major blocks.
- DRQ1 is a new experimental format, not standard QOI. Each block resets the QOI previous-pixel and color-cache state. A length directory lets workers locate blocks. Standard `.qoi` remains supported; `.drq` files are incompatible with ordinary QOI tools.
- Current implementation: Rust, Rayon for CPU parallelism, and the `image` crate for PNG input/output. The `qoi` and `rapid-qoi` crates are correctness and performance baselines in development benchmarks. No AI or GPU is involved in decoding.

## Decisions and experiments

1. Started with a correct QOI codec and tests, then benchmarked against independent decoders.
2. Research showed that changing the standard QOI decoder alone cannot remove its sequential state dependency. We introduced DRQ1 restart blocks instead, while preserving standard QOI support.
3. Tested block sizes and parallel thresholds on the full official corpus. The current best was 16,384 pixels per block, with parallel decoding starting at 65,536 pixels. Smaller blocks provided enough independent work for medium-sized images; 4- and 8-worker settings did not beat Rayon’s automatic worker count on this Mac.
4. Expanded the benchmark to report image-size bands, reused-output decoding, encoding speed, file size, and benchmark-process peak resident memory. Added image-size and slow-case reports, measured encoder speed and memory, and validated a separate USC-SIPI corpus. Pixel-by-pixel round-trip checks cover every encoder.
5. Parallelized DRQ1 encoding by assigning independent restart blocks to Rayon workers in bounded batches. Per-block temporary vectors and a two-pass exact-sizing attempt were rejected: the former pushed the full-corpus process above 8 GiB, and the latter spent too much time encoding every block twice. The final path reuses one bounded scratch area and estimates final output capacity from the first encoded batch.
6. Added `cargo-fuzz` targets for standard QOI, restart-block files, and QOI index sidecars. Fuzz inputs are capped at 16 KiB and generated images at 4,096 pixels. Tightened the sidecar checkpoint limit to 1,024 so a malformed index cannot trigger the previous maximum allocation of about 263 MiB; the matching index builder rejects larger worker counts.

## Current benchmark (2026-11-24, reproducible report)

- Machine: Apple M5, Darwin 25.6.0, ARM64, 10 physical/logical cores, 10 Rayon threads. The report records Rust compiler details and build-environment overrides.
- Main corpus: 2,848 PNGs, 1,321,975,407 pixels, 1,144,723,797 input bytes. Corpus SHA-256 over sorted relative paths, sizes, and contents: `1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7`. The official archive page lists 2,879 images; results use only this fingerprinted local set.
- Current baseline: commit `373166e`, Apple M5, Darwin 25.6.0, Rust 1.93.0, 10 Rayon threads; two full processes, five paired rounds each. Across 10 allocating-decode samples, `qoi` 534 Mpix/s, `rapid-qoi` 520, DRQ1 2,042. Paired median speedups were 3.78× vs `qoi` (range 2.66×–4.07×) and 3.86× vs `rapid-qoi` (3.10×–4.52×).
- Current image-size medians vs `qoi`: small (<65,536 pixels), DRQ1 332 vs 352 Mpix/s (0.96×); medium (65,536–1,048,575), 1,799 vs 509 (3.56×); large (at least 1,048,576), 2,515 vs 548 (4.64×). Small images remain roughly even; medium and large images drive the aggregate gain.
- Current reused-output medians: `qoi` 549 Mpix/s, `rapid-qoi` 510, DRQ1 2,068. Paired speedups were 4.05× vs `qoi` (range 2.67×–4.30×) and 4.10× vs `rapid-qoi` (3.44×–4.85×). Current DRQ1 encoder median was 660 Mpix/s vs 364 for `qoi` (1.84×; range 1.76×–1.91×).
- Parallel coverage: 2,120/2,848 images used parallel decode and represented 99.3% of suite pixels. DRQ1 size was 100.1% of QOI size.
- Repeated encoding: across 10 samples, reference `qoi` 356 Mpix/s, Drishy standard QOI 164, DRQ1 640. Paired DRQ1 speedup was 1.83× (range 1.72×–1.91×); output size stayed at 100.1% of reference QOI.
- The current JSON summary is [`benchmark-results/drishy-20261124T155944Z.json`](../benchmark-results/drishy-20261124T155944Z.json); its 3,842 raw timing rows are in [`benchmark-results/drishy-20261124T155944Z.samples.jsonl`](../benchmark-results/drishy-20261124T155944Z.samples.jsonl). Both runs used 10 Rayon threads, block size 16,384, commit `373166e`, and the same corpus fingerprint. The prior report at commit `ebf2039` used the same image files and provides the earlier 3.72× / 3.92× result.
- Separate USC-SIPI corpus: 39 TIFFs available, converted to PNG before timing; 10.0 million pixels. A prior, unrepeated run measured `qoi` 266 and DRQ1 383 Mpix/s (1.44×); retain it as supplemental evidence only.
- A standalone official C benchmark previously varied from 287 to 443 Mpix/s. It used C-encoded QOI streams and separate runs, so it is context only, not a paired speedup claim.
- Earlier memory measurements were whole-harness peaks: 1.8 GiB encoder-only and 3.8 GiB full decode-and-encode. They are not isolated single-image decoder memory. The largest decoded pixel buffer was 103.5 MiB.
- Full decode checks compared every generated DRQ1 stream against independent QOI decoder outputs before timing. Timed decoding uses in-memory streams; file reading and PNG conversion are excluded. Encoder timing excludes QOI-to-pixel preparation. Per-image slow-case timings are noisy.

## Verification and limits

- Decoder correctness has been cross-checked against independent `qoi` and `rapid-qoi` implementations. Encoder outputs were decoded back and compared byte-for-byte with source pixels. The first supplemental Kodak run was discovered to use the exact 24 `kodim` files already present in the official suite; keep it as a repeated photo-category check, not as independent-corpus evidence.
- The parallel encoder's dedicated test checks repeatable output and exact decoded pixels for a multi-block image; the full 2,848-image benchmark also cross-checked every current DRQ1 output before timing.
- Decoder hardening adds a malformed-sidecar regression test and bounded `qoi_decode` / `restart_decode` fuzz targets. Initial local smoke sessions completed without a crash: QOI ran 869,242 cases in 11 seconds (1,052 coverage points; libFuzzer-reported RSS 425 MB), and DRQ1 ran 543,228 cases in 11 seconds (476 coverage points; RSS 448 MB). These short sessions are useful smoke coverage, not a proof of safety.
- This is evidence on two corpora on one Apple M5, not a promise for every image or CPU. We have not measured another computer, disk-to-display latency, or isolated single-decode peak RSS.
- The archive-count caveat remains: the current [QOI benchmark page](https://qoiformat.org/benchmark/) says 2,879 images; the 1.1 GB archive contains 2,848 PNGs plus support files. Its 2,848 PNG corpus fingerprint matches both benchmark reports. That extracted corpus is saved outside the repository for the cross-computer run.
- Cross-computer validation is now the next stage. Use commit `373166e`, the same extracted corpus fingerprint, and the recorded benchmark command; compare per-machine paired speedups and keep each machine's worker count and compiler version in the report.
- Added `scripts/repeat_benchmark.py` to save a JSON summary and JSONL raw-sample sidecar. Each sample row includes elapsed nanoseconds and repeat count; the report records source commit, working-tree status, machine and Rust details, Rayon thread count, and a SHA-256 fingerprint over sorted corpus paths and contents. For future comparisons, use the same mode, block size, thread count, power connection, and corpus fingerprint; let the machine cool between long runs if temperatures are affecting results.

## Research links

- [Official QOI specification](https://qoiformat.org/qoi-specification.pdf): raster decoding and previous-pixel/color-cache state.
- [QOIR format documentation](https://github.com/nigeltao/qoir/blob/main/doc/qoir_file_format.md): independent 64×64 tiles.
- [Rayon `ThreadPoolBuilder`](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html): configurable CPU worker pools.
- [USC-SIPI Image Database](https://sipi.usc.edu/database/?volume=misc): source of the supplemental research images.
