# Cross-computer benchmark

The cross-computer validation is complete on a Dell G15 5530. This guide records the completed thread sweep and keeps the steps for reproducing it on another machine. The benchmark times decoding in memory, so it measures codec throughput rather than disk-to-display latency.

## Completed Dell G15 5530 sweep

The friend confirmed the machine as a Dell G15 5530 with a 13th Gen Intel Core i9-13900HX. PowerShell reported 24 cores and 32 logical processors; the JSON itself records a generic Intel CPU identifier, Windows 10, and Rust 1.93.0. All four reports use commit `373166e3d8e90251b532cbdf1f9a7b3552771e1f` and the matching 2,848-file corpus fingerprint `1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7`. Each thread setting has two successful external runs and 10 paired samples.

| Rayon threads | Fresh-output decode vs `qoi` | Reused-buffer decode vs `qoi` | DRQ1 encode vs `qoi` |
| ---: | ---: | ---: | ---: |
| 8 | 3.27× | 4.39× | 1.73× |
| 16 | 3.24× | 5.46× | 1.87× |
| 24 | 3.06× | 6.18× | 1.87× |
| 32 | 2.91× | 6.33× | 1.87× |

These are medians of paired samples, not guarantees for every file or computer. Fresh-output decoding peaked at 8 threads for this corpus and machine. Reused-buffer decoding was fastest at 32, though it gained only about 2% over 24. The encoder was essentially flat from 16 to 32. Small images remained slower than `qoi` (about 0.88×) at every thread count; medium and large images were faster.

Run notes report AC power, the Windows Balanced power plan, OneDrive sync paused during the final sweep, and Codex codebase-memory workers active. The Dell thermal setting was unknown. All benchmark processes exited 0; the notes say `cargo test --locked` passed 12 tests. The JSON reports record the expected commit and corpus, but also mark the working tree as not clean without listing changed paths. Treat that as a reproducibility caveat rather than proof that the working tree was clean.

The four JSON reports, four matching raw-sample files, and supplied run notes are preserved in [`../benchmark-results/dell-g15-5530-thread-sweep/`](../benchmark-results/dell-g15-5530-thread-sweep/).

## Reproduce the Dell thread sweep

### Inputs needed on the test computer

- Copy of `qoi_benchmark_suite.tar` (about 1.1 GB). It is stored outside the repository on the M5 Mac. Copy it by AirDrop, an external drive, or another file-transfer method.
- The receiving laptop needs Git, Python 3, and Rustup. No Docker or GPU is needed.

Do not put the image archive in GitHub. It is too large and is only benchmark data.

### Steps

1. Open Terminal on macOS/Linux, or PowerShell on Windows. Clone the repository and install the benchmark's Rust version:

   ```sh
   git clone https://github.com/kris70lesgo/drishy.git
   cd drishy
   rustup toolchain install 1.93.0
   ```

2. Extract the copied archive outside the repository. Keep the archive's `images/` folder intact. For example, on macOS/Linux, if the archive is in Downloads:

   ```sh
   mkdir -p ../drishy-benchmark-data/full-suite
   tar -xf ~/Downloads/qoi_benchmark_suite.tar -C ../drishy-benchmark-data/full-suite
   ```

   On Windows PowerShell, choose a destination folder and use:

   ```powershell
   New-Item -ItemType Directory -Force ..\drishy-benchmark-data\full-suite | Out-Null
   tar -xf "$HOME\Downloads\qoi_benchmark_suite.tar" -C ..\drishy-benchmark-data\full-suite
   ```

3. Before the long run, verify the extracted image set. The result must say **2,848 files** and show SHA-256 `1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7`. If it says `MISMATCH`, stop and send Agastya the output; do not benchmark that folder.

   ```sh
   python3 scripts/verify_corpus.py ../drishy-benchmark-data/full-suite
   ```

   In Windows PowerShell, use `py -3` instead of `python3`:

   ```powershell
   py -3 scripts\verify_corpus.py ..\drishy-benchmark-data\full-suite
   ```

4. Check out the exact code used for the M5 reference, and select the matching Rust compiler:

   ```sh
   git checkout 373166e3d8e90251b532cbdf1f9a7b3552771e1f
   rustup override set 1.93.0
   ```

5. Run two complete benchmark processes at each thread count. For the Dell sweep, use 8, 16, 24, and 32. On macOS/Linux:

   ```sh
   for THREADS in 8 16 24 32; do
     python3 scripts/repeat_benchmark.py ../drishy-benchmark-data/full-suite --runs 2 --threads "$THREADS"
   done
   ```

   On Windows PowerShell:

   ```powershell
   foreach ($threads in @(8, 16, 24, 32)) {
     py -3 scripts\repeat_benchmark.py ..\drishy-benchmark-data\full-suite --runs 2 --threads $threads
     if ($LASTEXITCODE -ne 0) { throw "Benchmark failed at $threads threads" }
   }
   ```

   Leave the laptop plugged in and avoid other heavy work until both runs finish. The first build and the two benchmark processes may take longer on a slower laptop.

### Files produced

Each invocation saves a JSON report and matching `.samples.jsonl` file under `benchmark-results/`. Keep both for every thread setting. Also record whether the computer was plugged in, its power mode, and whether CPU-heavy background work was running. The source commit and corpus fingerprint must match the M5 reference.

The M5 reference is [`drishy-20261124T155944Z.json`](../benchmark-results/drishy-20261124T155944Z.json): 3.78× median allocating-decode speedup over `qoi` and 3.86× over `rapid-qoi` across 10 paired samples. The official archive page lists 2,879 images, but this archive yields 2,848 benchmark PNGs plus support files; the verification command protects against comparing different image sets.
