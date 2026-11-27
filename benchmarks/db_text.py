"""Compare verified DB and text archives with gzip, ZIP and RAR."""

import argparse
import hashlib
import json
import platform
import sqlite3
import statistics
import subprocess
import tempfile
from pathlib import Path

FORMATS = ("monazip", "gzip", "zip", "rar")


def prepare(root, project):
    db = project / ".setup/corpus/chinook.sqlite"
    downloaded = project / ".setup/canterbury/files"
    sql = root / "chinook.sql"
    with sqlite3.connect(f"{db.resolve().as_uri()}?mode=ro", uri=True) as connection, sql.open("w", encoding="utf-8") as dump:
        for statement in connection.iterdump():
            dump.write(f"{statement}\n")
    return [
        db, sql, project / ".setup/corpus/usgs.csv", project / "app/package-lock.json",
        *(downloaded / name for name in ("alice29.txt", "asyoulik.txt", "cp.html", "lcet10.txt", "plrabn12.txt")),
    ]


def measure(binary, path, runs):
    args = [str(binary), str(path), "--quiet", "--json"]
    reports = [json.loads(subprocess.check_output(args)) for _ in range(runs)]
    first = reports[0]
    if not all(report["verified"] for report in reports):
        raise RuntimeError(f"{path.name}: round-trip failed")
    first["elapsed_ms"] = statistics.median(report["elapsed_ms"] for report in reports)
    for fmt in FORMATS:
        if any((report[fmt] is None) != (first[fmt] is None) for report in reports):
            raise RuntimeError(f"{path.name}: {fmt} availability changed")
        if first[fmt] is None:
            continue
        if any(report[fmt]["bytes"] != first[fmt]["bytes"] for report in reports):
            raise RuntimeError(f"{path.name}: output size changed")
        for metric in ("compress_us", "verify_us"):
            first[fmt][metric] = statistics.median(report[fmt][metric] for report in reports)
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    return {"sample": path.name, "sha256": digest, **first}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/monazip"))
    parser.add_argument("--output", type=Path, default=Path("benchmarks/db-text-results.json"))
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("files", type=Path, nargs="*")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    binary = args.binary.resolve(strict=True)
    project = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory() as directory:
        paths = args.files or prepare(Path(directory), project)
        rows = [measure(binary, path, args.runs) for path in paths]
    summary = {}
    for fmt in FORMATS:
        measured = [row[fmt] for row in rows if row[fmt] is not None]
        if measured and len(measured) != len(rows):
            raise RuntimeError(f"{fmt}: some files lack a verified measurement")
        if measured:
            summary[fmt] = {metric: sum(row[metric] for row in measured) for metric in ("bytes", "compress_us", "verify_us")}
    output = {"system": platform.platform(), "runs_per_file": args.runs, "summary": summary, "files": rows}
    args.output.write_text(json.dumps(output, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
