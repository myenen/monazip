"""Measure automatic stride and block-size selection separately and together."""

import argparse
import hashlib
import json
import os
import platform
import subprocess
import tempfile
from pathlib import Path

MODES = {
    "fixed": ["--block-size", "65536", "--fixed-strides"],
    "stride": ["--block-size", "65536"],
    "block_size": ["--fixed-strides"],
    "both": [],
}


def measure(binary, path, mode):
    args = [str(binary), str(path), "--experimental", "--quiet", "--json", "--detail-limit", "0", *MODES[mode]]
    system = platform.system()
    peak_rss = None
    if system in {"Darwin", "Linux"}:
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            child = subprocess.Popen(args, stdout=stdout, stderr=stderr)
            _, status, stats = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)
            if child.returncode:
                stderr.seek(0)
                raise RuntimeError(f"{path.name} ({mode}): {stderr.read().decode(errors='replace')}")
            stdout.seek(0)
            report = json.load(stdout)
        peak_rss = stats.ru_maxrss * (1024 if system == "Linux" else 1)
    else:
        report = json.loads(subprocess.check_output(args))
    if not report["verified"]:
        raise RuntimeError(f"{path.name} ({mode}): verification failed")
    return {
        "mode": mode,
        "original_bytes": report["original_bytes"],
        "experimental_bytes": report["experimental_bytes"],
        "zstd_stream_bytes": report["zstd_stream_bytes"],
        "zstd_block_bytes": report["zstd_block_bytes"],
        "block_size": report["options"]["block_size"],
        "elapsed_ms": report["elapsed_ms"],
        "peak_rss_bytes": peak_rss,
        "verified": report["verified"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path, help="Directory of independent test files")
    parser.add_argument("--binary", type=Path, default=Path("target/release/monazip"))
    parser.add_argument("--output", type=Path, default=Path("benchmarks/adaptive-results.json"))
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    paths = sorted(path for path in args.corpus.iterdir() if path.is_file() and not path.name.startswith("."))
    if not paths:
        parser.error("the corpus directory must contain at least one file")

    results = []
    for path in paths:
        with path.open("rb") as sample:
            digest = hashlib.file_digest(sample, "sha256").hexdigest()
        values = [measure(binary, path, mode) for mode in MODES]
        results.append({"sample": path.name, "sha256": digest, "measurements": values})
        print(f'{path.name:16} {", ".join(str(item["experimental_bytes"]) for item in values)}', flush=True)

    summary = {}
    for mode in MODES:
        values = [record for case in results for record in case["measurements"] if record["mode"] == mode]
        resident_sizes = [entry["peak_rss_bytes"] for entry in values if entry["peak_rss_bytes"] is not None]
        summary[mode] = {
            "total_experimental_bytes": sum(entry["experimental_bytes"] for entry in values),
            "total_zstd_stream_bytes": sum(entry["zstd_stream_bytes"] for entry in values),
            "total_elapsed_ms": sum(entry["elapsed_ms"] for entry in values),
            "larger_than_source": sum(entry["experimental_bytes"] > entry["original_bytes"] for entry in values),
            "largest_peak_rss_bytes": max(resident_sizes, default=None),
        }
    args.output.write_text(json.dumps({
        "system": platform.platform(), "zstd_level": 3, "summary": summary, "results": results,
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
