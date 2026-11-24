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

## Current benchmark (2026-11-24)

- Machine: Apple M5, 10 CPU cores.
- Main corpus: the downloaded QOI archive produced 2,848 PNGs and 1.322 billion pixels. The official benchmark page currently says 2,879 images, so the 31-image mismatch is unresolved; results cover the files actually downloaded. Each decoder received the same in-memory QOI data; PNG conversion and file reading were outside timed decode.
- Allocating decode, latest full run: `qoi` 402 Mpix/s, `rapid-qoi` 420, DRQ1 1,481. DRQ1 reached 3.68× vs `qoi` and 3.53× vs `rapid-qoi`. Five paired runs ranged from 3.35× to 3.89× vs `qoi`, and 3.18× to 3.64× vs `rapid-qoi`. An earlier run was faster (2,222/572/570 Mpix/s); rates vary across runs on this one machine, so paired ratios are the more useful comparison.
- Image-size results vs `qoi`: small (<65,536 pixels), DRQ1 277 vs 284 Mpix/s (0.98×); medium (65,536–1,048,575), 1,107 vs 390 (2.84×); large (at least 1,048,576), 1,778 vs 425 (4.18×). The aggregate gain comes from medium and large images; the small-image path is roughly even.
- Slow large-image tail: across 301 images of at least 1,048,576 pixels, the weakest measured speedup in this run was 1.31× on `rock02.png` (1024×1024), followed by 1.69× on `cd_dvd_PNG102294.png`. Per-image results are noisy; this supplements, but does not replace, the whole-corpus paired benchmark.
- Reused output: `qoi` 436 Mpix/s, `rapid-qoi` 431, DRQ1 1,532 (3.51× and 3.56×). Five paired rounds ranged 2.65×–3.77× vs `qoi` and 2.75×–4.00× vs `rapid-qoi`.
- Parallel coverage: 2,120/2,848 images used parallel decode and represented 99.3% of suite pixels. DRQ1 size was 100.1% of QOI size.
- Separate USC-SIPI corpus: 39 TIFFs available in the downloaded archive, converted to PNG before timing; 10.0 million pixels across color, grayscale, photos, and test patterns. The collection page lists 44 images and notes five are no longer distributed. The filenames do not occur in the QOI suite. Latest encoder-only run: `qoi` 266 Mpix/s, Drishy standard QOI 112, DRQ1 383 (1.44× vs `qoi`); output sizes remained 100.0% and 100.1% of reference QOI size. The separate corpus is still small, so it does not prove universal performance.
- Encoder optimization: prior main-corpus baseline was `qoi` 413, Drishy standard QOI 210, DRQ1 209 Mpix/s. The final bounded-batch encoder measured 244 [226–252], 116 [107–120], and 396 [353–405] Mpix/s respectively on the encoder-only pass; DRQ1 was 1.62× reference QOI in that run. A later full validation pass measured 272 [236–363], 128 [110–167], and 471 [283–674] Mpix/s. The earlier 209-to-396 change is encouraging but spans separate runs; use paired same-run comparisons and don't claim it as a controlled A/B result. Both current passes kept DRQ1 output at 100.1% of QOI size.
- Official C QOI reference: standalone `qoibench` runs measured 287 and, earlier, 443 Mpix/s on the same Mac. It used C-encoded streams in separate runs, so the large variation makes this context only, not a paired speedup claim. See the [official QOI C implementation and benchmark](https://github.com/phoboslab/qoi) and [benchmark page](https://qoiformat.org/benchmark/).
- Memory: the latest encoder-only benchmark process peaked at 1.8 GiB resident memory; the full decode-and-encode benchmark process peaked at 3.8 GiB. These are harness peaks, not isolated single-image decode memory. The largest single decoded pixel buffer was 103.5 MiB. The high-water result remains sensitive to benchmark mode and temporary allocations.
- A cold first-call measurement on the largest image (1313×20667) was 0.053 s for `qoi` and 0.038 s for DRQ1 (1.41×, Rayon startup included). An earlier run produced a different ratio; do not use one cold sample as a headline. The stable headline is five paired warmed runs.
- Timings report five paired rounds, include block-directory parsing and output allocation where applicable, and exclude file I/O and PNG conversion. Encoder runs also exclude decoding QOI inputs to raw pixels. All decoders and all three encoders were checked for exact pixel equality.

## Verification and limits

- Decoder correctness has been cross-checked against independent `qoi` and `rapid-qoi` implementations. Encoder outputs were decoded back and compared byte-for-byte with source pixels. The first supplemental Kodak run was discovered to use the exact 24 `kodim` files already present in the official suite; keep it as a repeated photo-category check, not as independent-corpus evidence.
- The parallel encoder's dedicated test checks repeatable output and exact decoded pixels for a multi-block image; the full 2,848-image benchmark also cross-checked every current DRQ1 output before timing.
- This is evidence on two corpora on one Apple M5, not a promise for every image or CPU. We have not measured another computer, disk-to-display latency, or isolated single-decode peak RSS.
- The archive-count caveat remains: the current [QOI benchmark page](https://qoiformat.org/benchmark/) says 2,879 images; the downloaded archive contains 2,848 PNGs. The results are for the 2,848 local PNGs only.
- Added `scripts/repeat_benchmark.py` to save independent repeats as JSON. Each run includes every timed sample's elapsed nanoseconds and repeat count, plus the source commit, working-tree status, machine and Rust details, Rayon thread count, and a SHA-256 fingerprint over the sorted corpus paths and contents. For future comparisons, use the same mode, block size, thread count, power connection, and corpus fingerprint; let the machine cool between long runs if temperatures are affecting results.

## Research links

- [Official QOI specification](https://qoiformat.org/qoi-specification.pdf): raster decoding and previous-pixel/color-cache state.
- [QOIR format documentation](https://github.com/nigeltao/qoir/blob/main/doc/qoir_file_format.md): independent 64×64 tiles.
- [Rayon `ThreadPoolBuilder`](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html): configurable CPU worker pools.
- [USC-SIPI Image Database](https://sipi.usc.edu/database/?volume=misc): source of the supplemental research images.
