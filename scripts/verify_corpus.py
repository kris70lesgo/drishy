#!/usr/bin/env python3
"""Verify that a benchmark folder matches Drishy's saved reference corpus."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from repeat_benchmark import corpus_paths, fingerprint_corpus


EXPECTED_SHA256 = "1c25bfa55fb2a542d29dd4a69584deb5e2eda755d13642ef6d18c9498ea493b7"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path, help="Extracted benchmark folder")
    args = parser.parse_args()

    root = args.corpus.expanduser().resolve()
    paths = corpus_paths(root)
    if not paths:
        parser.error(f"no lowercase .png or .qoi files found at {root}")

    fingerprint = fingerprint_corpus(root, paths)
    actual = str(fingerprint["sha256"])
    print(f"Files: {fingerprint['file_count']}")
    print(f"Input bytes: {fingerprint['input_bytes']}")
    print(f"SHA-256: {actual}")
    if actual != EXPECTED_SHA256:
        print(f"MISMATCH: expected {EXPECTED_SHA256}", file=sys.stderr)
        return 1

    print("Verified: this is the benchmark corpus used for the M5 baseline.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
