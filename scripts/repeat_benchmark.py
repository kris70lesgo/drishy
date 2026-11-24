#!/usr/bin/env python3
"""Run the Drishy corpus benchmark repeatedly and save raw samples plus context."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import platform
import re
import shlex
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from pathlib import Path
from statistics import median


REPO = Path(__file__).resolve().parents[1]
RAW_PREFIX = "DRISHY_SAMPLE\t"


def command_output(command: list[str], cwd: Path = REPO) -> str | None:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return result.stdout.strip()


def git_metadata(output_path: Path) -> dict[str, object]:
    commit = command_output(["git", "rev-parse", "HEAD"])
    branch = command_output(["git", "branch", "--show-current"])
    status = command_output(["git", "status", "--porcelain", "--untracked-files=all"])
    if status is None:
        clean = False
    else:
        try:
            report_relative = output_path.relative_to(REPO).as_posix()
        except ValueError:
            report_relative = None
        dirty_entries = [
            line
            for line in status.splitlines()
            if report_relative is None or line[3:].strip() != report_relative
        ]
        clean = not dirty_entries
    return {
        "commit": commit,
        "branch": branch,
        "working_tree_clean_at_start": clean,
    }


def machine_metadata() -> dict[str, object]:
    cpu_model = platform.processor() or None
    physical_cores = None
    if sys.platform == "darwin":
        cpu_model = (
            command_output(["sysctl", "-n", "machdep.cpu.brand_string"])
            or command_output(["sysctl", "-n", "hw.model"])
            or cpu_model
        )
        try:
            physical_cores = int(command_output(["sysctl", "-n", "hw.physicalcpu"]) or "")
        except ValueError:
            pass
    elif sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/cpuinfo").read_text().splitlines():
                if line.lower().startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                    break
        except OSError:
            pass

    return {
        "system": platform.system(),
        "release": platform.release(),
        "version": platform.version(),
        "architecture": platform.machine(),
        "cpu_model": cpu_model,
        "physical_cores": physical_cores,
        "logical_cores": os.cpu_count(),
        "python": platform.python_version(),
        "rayon_num_threads_env": os.environ.get("RAYON_NUM_THREADS"),
    }


def corpus_paths(root: Path) -> list[Path]:
    if root.is_file():
        return [root] if root.suffix in {".png", ".qoi"} else []
    return sorted(
        (path for path in root.rglob("*") if path.is_file() and path.suffix in {".png", ".qoi"}),
        key=lambda path: path.relative_to(root).as_posix(),
    )


def fingerprint_corpus(root: Path, paths: list[Path]) -> dict[str, object]:
    digest = hashlib.sha256()
    byte_count = 0
    relative_root = root if root.is_dir() else root.parent
    for path in paths:
        relative = path.relative_to(relative_root).as_posix().encode("utf-8")
        size = path.stat().st_size
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(size.to_bytes(8, "big"))
        with path.open("rb") as source:
            while chunk := source.read(1024 * 1024):
                digest.update(chunk)
        byte_count += size
    return {
        "label": root.name or root.parent.name,
        "file_count": len(paths),
        "input_bytes": byte_count,
        "sha256": digest.hexdigest(),
        "fingerprint_method": "SHA-256 over sorted relative paths, file sizes, and file contents",
    }


def parse_sample_lines(
    output: str, run_index: int
) -> tuple[list[dict[str, object]], str, int | None, dict[str, int]]:
    samples: list[dict[str, object]] = []
    summary_lines: list[str] = []
    rayon_threads = None
    corpus_stats: dict[str, int] = {}
    for line in output.splitlines():
        match = re.fullmatch(
            r"(\d+) images, [\d.]+ Gpixels; restart block size = (\d+) pixels", line
        )
        if match:
            corpus_stats["benchmark_images"] = int(match.group(1))
            corpus_stats["block_pixels"] = int(match.group(2))
        if line.startswith(RAW_PREFIX):
            fields = line.split("\t")
            if len(fields) != 9:
                raise ValueError(f"Malformed benchmark sample row: {line!r}")
            _, mode, group, implementation, sample, elapsed_ns, repeats, pixels, rate = fields
            elapsed = int(elapsed_ns)
            repeat_count = int(repeats)
            samples.append(
                {
                    "run": run_index,
                    "mode": mode,
                    "group": group,
                    "implementation": implementation,
                    "sample": int(sample),
                    "elapsed_ns": elapsed,
                    "repeats": repeat_count,
                    "per_pass_ns": elapsed / repeat_count,
                    "pixels": int(pixels),
                    "mpix_per_second": float(rate),
                }
            )
        else:
            summary_lines.append(line)
            if line.startswith("Rayon worker threads:"):
                rayon_threads = int(line.rsplit(" ", 1)[1])
    if samples and not corpus_stats.get("total_pixels"):
        # Aggregate rows include the exact corpus pixel count; size-band and
        # slow-case rows do not, so use the aggregate total when available.
        aggregate = next(
            (
                sample
                for sample in samples
                if sample["group"] == "all" and sample["mode"] in {"decode_allocating", "encode"}
            ),
            None,
        )
        if aggregate:
            corpus_stats["total_pixels"] = int(aggregate["pixels"])
    return samples, "\n".join(summary_lines), rayon_threads, corpus_stats


def summarize_samples(samples: list[dict[str, object]]) -> dict[str, object]:
    grouped: dict[tuple[str, str, str], list[dict[str, object]]] = defaultdict(list)
    paired: dict[tuple[int, str, str, int], dict[str, float]] = defaultdict(dict)
    for sample in samples:
        key = (str(sample["mode"]), str(sample["group"]), str(sample["implementation"]))
        grouped[key].append(sample)
        if sample["implementation"] in {"qoi", "rapid_qoi", "drq1"}:
            pair_key = (
                int(sample["run"]),
                str(sample["mode"]),
                str(sample["group"]),
                int(sample["sample"]),
            )
            paired[pair_key][str(sample["implementation"])] = float(sample["mpix_per_second"])

    throughput = []
    for (mode, group, implementation), entries in sorted(grouped.items()):
        rates = [float(entry["mpix_per_second"]) for entry in entries]
        throughput.append(
            {
                "mode": mode,
                "group": group,
                "implementation": implementation,
                "sample_count": len(rates),
                "median_mpix_per_second": median(rates),
                "min_mpix_per_second": min(rates),
                "max_mpix_per_second": max(rates),
            }
        )

    speedups: dict[tuple[str, str, str], list[float]] = defaultdict(list)
    for (_, mode, group, _), rates in paired.items():
        if "drq1" not in rates:
            continue
        for reference in ("qoi", "rapid_qoi"):
            if reference in rates:
                speedups[(mode, group, reference)].append(rates["drq1"] / rates[reference])
    paired_speedups = [
        {
            "mode": mode,
            "group": group,
            "drq1_vs": reference,
            "sample_count": len(values),
            "median": median(values),
            "min": min(values),
            "max": max(values),
        }
        for (mode, group, reference), values in sorted(speedups.items())
    ]
    return {"throughput": throughput, "paired_speedups": paired_speedups}


def atomic_write(path: Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path, help="Directory or single PNG/QOI benchmark input")
    parser.add_argument("--mode", choices=("full", "encode", "slow"), default="full")
    parser.add_argument("--runs", type=int, default=3, help="Independent benchmark processes (default: 3)")
    parser.add_argument("--block-pixels", type=int, default=16384)
    parser.add_argument("--threads", type=int, help="Set RAYON_NUM_THREADS to a fixed count")
    parser.add_argument("--output", type=Path, help="JSON report path; defaults to benchmark-results/")
    args = parser.parse_args()

    if args.runs < 1:
        parser.error("--runs must be at least 1")
    if args.block_pixels < 1:
        parser.error("--block-pixels must be at least 1")
    if args.threads is not None and args.threads < 1:
        parser.error("--threads must be at least 1")

    corpus = args.corpus.expanduser().resolve()
    paths = corpus_paths(corpus)
    if not paths:
        parser.error(f"no lowercase .png or .qoi files found at {corpus}")

    now = dt.datetime.now(dt.timezone.utc)
    if args.output is None:
        stamp = now.strftime("%Y%m%dT%H%M%SZ")
        output_path = REPO / "benchmark-results" / f"drishy-{stamp}.json"
    else:
        output_path = args.output.expanduser().resolve()

    print(f"Fingerprinting {len(paths)} corpus files; this is outside the timed benchmark.", flush=True)
    hash_started = time.monotonic()
    corpus_info = fingerprint_corpus(corpus, paths)
    corpus_info["fingerprint_seconds"] = round(time.monotonic() - hash_started, 3)

    command = ["cargo", "bench", "--locked", "--bench", "restart_qoi", "--", str(corpus)]
    if args.block_pixels != 16384:
        command.append(str(args.block_pixels))
    if args.mode == "encode":
        command.append("--encoding-only")
    elif args.mode == "slow":
        command.append("--slow-cases-only")
    command.append("--raw-samples")

    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    if args.threads is not None:
        environment["RAYON_NUM_THREADS"] = str(args.threads)

    template = ["cargo", "bench", "--locked", "--bench", "restart_qoi", "--", "<corpus>"]
    if args.block_pixels != 16384:
        template.append(str(args.block_pixels))
    if args.mode == "encode":
        template.append("--encoding-only")
    elif args.mode == "slow":
        template.append("--slow-cases-only")
    template.append("--raw-samples")

    report: dict[str, object] = {
        "schema_version": 1,
        "created_utc": now.isoformat(),
        "source": git_metadata(output_path),
        "machine": machine_metadata(),
        "rustc": command_output(["rustc", "-Vv"]),
        "cargo_lock_sha256": hashlib.sha256((REPO / "Cargo.lock").read_bytes()).hexdigest(),
        "corpus": corpus_info,
        "benchmark": {
            "mode": args.mode,
            "block_pixels": args.block_pixels,
            "external_runs_requested": args.runs,
            "command_template": template,
            "rayon_num_threads_requested": environment.get("RAYON_NUM_THREADS"),
            "build_environment": {
                name: os.environ.get(name)
                for name in (
                    "RUSTFLAGS",
                    "CARGO_PROFILE_BENCH_OPT_LEVEL",
                    "CARGO_PROFILE_BENCH_LTO",
                    "CARGO_PROFILE_BENCH_CODEGEN_UNITS",
                )
            },
            "notes": "Each raw sample stores total measured nanoseconds and repeat count; throughput is per corpus pass.",
        },
        "runs": [],
    }

    print("Before a run: connect power, close CPU-heavy apps, and keep the same power mode for all repeats.", flush=True)
    runs = report["runs"]
    assert isinstance(runs, list)
    for run_index in range(1, args.runs + 1):
        run_started = dt.datetime.now(dt.timezone.utc)
        print(f"Starting run {run_index}/{args.runs}: {shlex.join(template)}", flush=True)
        with tempfile.TemporaryFile(mode="w+t", encoding="utf-8") as stdout_file:
            completed = subprocess.run(
                command,
                cwd=REPO,
                env=environment,
                stdout=stdout_file,
                check=False,
            )
            stdout_file.seek(0)
            stdout = stdout_file.read()
        samples, readable_output, rayon_threads, corpus_stats = parse_sample_lines(
            stdout, run_index
        )
        run_record = {
            "run": run_index,
            "started_utc": run_started.isoformat(),
            "finished_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "exit_code": completed.returncode,
            "rayon_threads_reported": rayon_threads,
            "corpus_stats": corpus_stats,
            "summary_output": readable_output,
            "samples": samples,
        }
        runs.append(run_record)
        all_samples = [sample for run in runs for sample in run["samples"]]
        report["summary"] = summarize_samples(all_samples)
        atomic_write(output_path, report)
        print(readable_output, flush=True)
        print(f"Saved raw samples to {output_path}", flush=True)
        if completed.returncode != 0:
            print(f"Benchmark run {run_index} failed with exit code {completed.returncode}.", file=sys.stderr)
            return completed.returncode

    summary = report["summary"]
    assert isinstance(summary, dict)
    print("\nAcross all saved samples:")
    for entry in summary["throughput"]:
        if entry["group"] == "all" and entry["mode"] in {"decode_allocating", "decode_reused", "encode"}:
            print(
                f"  {entry['mode']} / {entry['implementation']}: "
                f"median {entry['median_mpix_per_second']:.1f} Mpix/s "
                f"(range {entry['min_mpix_per_second']:.1f}–{entry['max_mpix_per_second']:.1f}, "
                f"n={entry['sample_count']})"
            )
    for entry in summary["paired_speedups"]:
        if entry["group"] == "all" and entry["mode"] in {"decode_allocating", "decode_reused", "encode"}:
            print(
                f"  {entry['mode']} / DRQ1 vs {entry['drq1_vs']}: "
                f"median {entry['median']:.2f}× "
                f"(range {entry['min']:.2f}–{entry['max']:.2f}, n={entry['sample_count']})"
            )
    print(f"Report: {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
