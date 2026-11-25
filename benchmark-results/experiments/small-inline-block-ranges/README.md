# Small inline block-range experiment

## Question

Could removing the heap allocation for a decoded stream's block-range list improve small-image decoding? The candidate stored up to four ranges inline, which covers streams with up to 65,536 pixels at the default 16,384-pixel block size.

## Result

The candidate passed the local Rust test suite (13 integration tests). It did not improve the target metric on the M5. The paired median speedup for small-image allocating decode was **0.9545×** over `qoi` with the candidate, compared with **0.9550×** in the previous clean M5 baseline. That difference is negligible and slightly favors the baseline, so the code change was discarded.

The benchmark used two complete processes, 10 Rayon threads, and the same 2,848-image corpus. The overall score moved between sessions, so it is not used as evidence for or against this small-path change. The candidate report records a dirty working tree because the experiment was uncommitted when measured.

## Reproduction record

- Base commit: `04d5a11e51043b781621ca60e6a6602650ad0c55`.
- Candidate code diff: [`candidate.patch`](candidate.patch).
- Full benchmark report: [`drishy-20261125T170814Z.json`](drishy-20261125T170814Z.json).
- Raw timing samples: [`drishy-20261125T170814Z.samples.jsonl`](drishy-20261125T170814Z.samples.jsonl).
- Clean M5 comparison: [`../../drishy-20261124T155944Z.json`](../../drishy-20261124T155944Z.json).

The implementation was reverted because it added code and test surface without a measured speed gain.
