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

## Current benchmark (2026-11-23)

- Machine: Apple M5, 10 CPU cores.
- Main corpus: the downloaded QOI archive produced 2,848 PNGs and 1.322 billion pixels. The official benchmark page currently says 2,879 images, so the 31-image mismatch is unresolved; results cover the files actually downloaded. Each decoder received the same in-memory QOI data; PNG conversion and file reading were outside timed decode.
- Allocating decode: `qoi` 572 Mpix/s, `rapid-qoi` 570, DRQ1 2,222. DRQ1 reached 3.89× vs `qoi` and 3.90× vs `rapid-qoi`. Five paired runs ranged from 3.66× to 4.21× vs `qoi`, and 3.76× to 4.15× vs `rapid-qoi`.
- Image-size results vs `qoi`: small (<65,536 pixels), DRQ1 375 vs 392 Mpix/s (0.96×); medium (65,536–1,048,575), 2,000 vs 557 (3.59×); large (at least 1,048,576), 3,084 vs 594 (5.19×). The aggregate gain comes from medium and large images; the small-image path is roughly equal to baseline.
- Slow large-image tail: across 301 images of at least 1,048,576 pixels, the weakest measured speedup was 1.25× on `email_PNG28.png` (1024×1024); the next was 1.40× on `wineglass_PNG2911.png` (2359×5722). Each per-image result is a median of three warmed samples lasting at least 5 ms. This supplements, but does not replace, the whole-corpus paired benchmark.
- Reused output: `qoi` 592 Mpix/s, `rapid-qoi` 585, DRQ1 2,567 (4.34× and 4.39×).
- Parallel coverage: 2,120/2,848 images used parallel decode and represented 99.3% of suite pixels. DRQ1 size was 100.1% of QOI size.
- Separate USC-SIPI corpus: 39 TIFFs available in the downloaded archive, converted to PNG before timing; 10.0 million pixels across color, grayscale, photos, and test patterns. The collection page lists 44 images and notes five are no longer distributed. The filenames do not occur in the QOI suite. Allocating decode: `qoi` 483 Mpix/s, `rapid-qoi` 465, DRQ1 1,527 (3.16×/3.28×). Medium images: 2.77× vs `qoi`; three large images: 4.64×. Reused buffer: 485/464/1,599 Mpix/s (3.30×/3.44×). The separate corpus is still small, so it does not prove universal performance.
- Encoding-only pass on the main corpus: reference `qoi` 413 Mpix/s, Drishy standard QOI 210, DRQ1 209. Drishy standard QOI output was 100.0% of reference size; DRQ1 was 100.1%. Encoding is a clear next optimization target; the decode speedup does not extend to encoding yet. On USC-SIPI the rates were 320/164/155 Mpix/s, with the same near-equal file sizes.
- Memory: the full benchmark process peaked at 3.1 GiB resident memory because it loads and retains QOI and DRQ1 compressed versions of the whole suite. This is a harness peak, not isolated decoder memory. The largest single decoded pixel buffer was 103.5 MiB.
- A cold first-call measurement on the largest image (1313×20667) was 0.053 s for `qoi` and 0.038 s for DRQ1 (1.41×, Rayon startup included). An earlier run produced a different ratio; do not use one cold sample as a headline. The stable headline is five paired warmed runs.
- Timings report five paired rounds, include block-directory parsing and output allocation where applicable, and exclude file I/O and PNG conversion. Encoder runs also exclude decoding QOI inputs to raw pixels. All decoders and all three encoders were checked for exact pixel equality.

## Verification and limits

- Decoder correctness has been cross-checked against independent `qoi` and `rapid-qoi` implementations. Encoder outputs were decoded back and compared byte-for-byte with source pixels. The first supplemental Kodak run was discovered to use the exact 24 `kodim` files already present in the official suite; keep it as a repeated photo-category check, not as independent-corpus evidence.
- This is evidence on two corpora on one Apple M5, not a promise for every image or CPU. We have not measured another computer, disk-to-display latency, or isolated single-decode peak RSS.
- The archive-count caveat remains: the current [QOI benchmark page](https://qoiformat.org/benchmark/) says 2,879 images; the downloaded archive contains 2,848 PNGs. The results are for the 2,848 local PNGs only.

## Research links

- [Official QOI specification](https://qoiformat.org/qoi-specification.pdf): raster decoding and previous-pixel/color-cache state.
- [QOIR format documentation](https://github.com/nigeltao/qoir/blob/main/doc/qoir_file_format.md): independent 64×64 tiles.
- [Rayon `ThreadPoolBuilder`](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html): configurable CPU worker pools.
- [USC-SIPI Image Database](https://sipi.usc.edu/database/?volume=misc): source of the supplemental research images.
