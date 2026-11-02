#!/usr/bin/env python3
"""Measure a READY release host_cost example; never build or read user data.

Allocator counters include only Host::show, not constructing its input. They
measure requested heap bytes, not RSS/PSS or allocator metadata. Wall time also
includes serialization, counter overhead and the draining pipe reader.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import threading


def measure(binary, kind, rows, size, iterations):
    process = subprocess.Popen([str(binary), kind, str(rows), str(size), str(iterations)],
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    digest = hashlib.sha256()
    encoded_bytes = 0
    errors = []
    stderr = bytearray()

    def drain():
        nonlocal encoded_bytes
        try:
            while block := process.stdout.read(64 * 1024):
                digest.update(block)
                encoded_bytes += len(block)
        except Exception as error:
            errors.append(str(error))

    def drain_stderr():
        try:
            while block := process.stderr.read(4096):
                if len(stderr) + len(block) > 64 * 1024:
                    errors.append("measurement stderr exceeds 64 KiB")
                    continue
                stderr.extend(block)
        except Exception as error:
            errors.append(str(error))

    reader = threading.Thread(target=drain)
    diagnostics = threading.Thread(target=drain_stderr)
    reader.start()
    diagnostics.start()
    try:
        process.wait(timeout=30)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        reader.join(timeout=5)
        diagnostics.join(timeout=5)
    if reader.is_alive() or diagnostics.is_alive() or errors:
        raise RuntimeError(f"wire reader failed: {errors}")
    process.stdout.close()
    process.stderr.close()
    if process.returncode:
        raise RuntimeError(f"example exited {process.returncode}: {stderr.decode(errors='replace')}")
    samples = [json.loads(line) for line in stderr.splitlines()]
    if len(samples) != iterations or [sample["iteration"] for sample in samples] != list(range(iterations)):
        raise RuntimeError("incomplete or reordered example measurements")
    # First show initializes stdout and the shown map. Report it separately;
    # ordinary refresh medians use only subsequent calls with an existing view.
    refresh = samples[1:]
    fields = ("allocated_bytes", "allocation_calls", "peak_extra_live_bytes", "elapsed_ns")
    return dict(wire_sha256=digest.hexdigest(), wire_bytes=encoded_bytes,
                first=samples[0], refresh_median={key: statistics.median(s[key] for s in refresh) for key in fields},
                samples=samples)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rows", type=int, default=512)
    parser.add_argument("--bytes", type=int, default=4096)
    parser.add_argument("--iterations", type=int, default=11)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if not (1 <= args.rows <= 4096 and 1 <= args.bytes <= 16384 and args.rows * args.bytes <= 8 * 1024 * 1024):
        parser.error("rows 1..4096, bytes 1..16384, total text at most 8 MiB")
    if not (2 <= args.iterations <= 100 and 1 <= args.repeats <= 10):
        parser.error("iterations 2..100, repeats 1..10")
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    report = dict(ok=False, binary=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  benchmark_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  fixture=dict(rows=args.rows, text_bytes_per_row=args.bytes, iterations=args.iterations, repeats=args.repeats),
                  measurement=__doc__, kinds={})
    try:
        for kind in ("keys", "title"):
            runs = [measure(binary, kind, args.rows, args.bytes, args.iterations) for _ in range(args.repeats)]
            if len({r["wire_sha256"] for r in runs}) != 1:
                raise RuntimeError("nondeterministic wire output")
            report["kinds"][kind] = dict(runs=runs, refresh_median={
                key: statistics.median(run["refresh_median"][key] for run in runs)
                for key in runs[0]["refresh_median"]})
        report["ok"] = True
    finally:
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({kind: result["refresh_median"] for kind, result in report["kinds"].items()}))


if __name__ == "__main__":
    main()
