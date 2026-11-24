# Friend's laptop benchmark steps

This run checks whether Drishy's speedup carries over to another laptop. The two runs took about 13.5 minutes on the M5 reference; allow longer on a slower laptop. Please run it while plugged in, with CPU-heavy apps closed.

## What Agastya needs to provide

- Copy of `qoi_benchmark_suite.tar` (about 1.1 GB). It is stored outside the repository on the M5 Mac. Copy it by AirDrop, an external drive, or another file-transfer method.
- The receiving laptop needs Git, Python 3, and Rustup. No Docker or GPU is needed.

Do not put the image archive in GitHub. It is too large and is only benchmark data.

## Steps

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

5. Run two complete benchmark processes. Use the laptop's logical-core count for the thread setting. On macOS/Linux, find it and run:

   ```sh
   THREADS=$(python3 -c 'import os; print(os.cpu_count())')
   python3 scripts/repeat_benchmark.py ../drishy-benchmark-data/full-suite --runs 2 --threads "$THREADS"
   ```

   On Windows PowerShell:

   ```powershell
   $threads = py -3 -c "import os; print(os.cpu_count())"
   py -3 scripts\repeat_benchmark.py ..\drishy-benchmark-data\full-suite --runs 2 --threads $threads
   ```

   Leave the laptop plugged in and avoid other heavy work until both runs finish. The first build and the two benchmark processes may take longer on a slower laptop.

## What to send back

Send Agastya these two files from the new `benchmark-results` folder:

- The generated `drishy-<timestamp>.json`
- Its matching `drishy-<timestamp>.samples.jsonl`

Also tell him the laptop model, whether it was plugged in, and whether the benchmark completed normally. The JSON report records the CPU, operating system, Rust version, thread count, source commit, corpus fingerprint, and timing samples. The source commit and fingerprint must match the M5 reference.

The M5 reference is [`drishy-20261124T155944Z.json`](../benchmark-results/drishy-20261124T155944Z.json): 3.78× median allocating-decode speedup over `qoi` and 3.86× over `rapid-qoi` across 10 paired samples. The official archive page lists 2,879 images, but this archive yields 2,848 benchmark PNGs plus support files; the verification command protects against comparing different image sets.
