# Cross-computer benchmark

The next validation is to see whether Drishy's speedup holds on a second CPU. The reference report is [`drishy-20261124T155944Z.json`](../benchmark-results/drishy-20261124T155944Z.json): commit `373166e3d8e90251b532cbdf1f9a7b3552771e1f`, Apple M5, 10 Rayon threads, Rust 1.93.0, block size 16,384, and corpus fingerprint `1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7`.

## On the second computer

1. Copy the same `qoi_benchmark_suite.tar` archive from the first computer. It is about 1.1 GB. Extract it outside the project repository, keeping its `images/` directory intact. The archive page claims 2,879 images, but the archive currently provides 2,848 PNG files plus support files. The runner will print the exact file count and fingerprint before timing.
2. Clone Drishy and check out the reference commit:

   ```sh
   git clone https://github.com/kris70lesgo/drishy.git
   cd drishy
   git checkout 373166e3d8e90251b532cbdf1f9a7b3552771e1f
   ```

3. Install Rust 1.93.0 and select it for this checkout, then run the same two-process benchmark. Use the computer's logical-core count so Drishy can use all available CPU threads; the M5 reference used 10.

   ```sh
   rustup toolchain install 1.93.0
   rustup override set 1.93.0
   python3 scripts/repeat_benchmark.py /path/to/extracted/suite --runs 2 --threads 10
   ```

   Replace `10` with that computer's logical-core count when it differs. Connect power, close CPU-heavy applications, and keep the same power mode across both runs.

4. Compare the generated JSON report with the reference. Confirm that `source.commit`, `corpus.sha256`, block size, and benchmark mode match. Then compare the paired DRQ1 speedups against `qoi` and `rapid-qoi`, including the small, medium, and large image groups. Transfer the new `.json` and `.samples.jsonl` files back with the results.

The primary-Mac reference measured 3.78× allocating-decode speedup over `qoi` and 3.86× over `rapid-qoi` across 10 paired samples. These ratios describe this M5 and corpus; the second-computer run will show how they vary with CPU architecture and worker count.
