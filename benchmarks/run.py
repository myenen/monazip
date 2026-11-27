"""Generate reproducible compression samples and run both Zstd levels."""

import argparse
import csv
import hashlib
import io
import json
import platform
import random
import shutil
import sqlite3
import struct
import subprocess
import tempfile
import zipfile
import zlib
from pathlib import Path

SIZE = 256 * 1024
WIDTH = 512


def write_png(path, pixels):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    rows = b"".join(b"\x00" + pixels[i:i + WIDTH] for i in range(0, len(pixels), WIDTH))
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, SIZE // WIDTH, 8, 0, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


def generate(root, binary):
    rng = random.Random(9051)
    patterned = bytes((3 * (i // 73) + (i % 73) ** 2) % 256 for i in range(SIZE))
    raw_image = bytes(
        (x // 4 + y // 7 + rng.randrange(8) + (30 if (x ^ y) & 128 else 0)) % 256
        for y in range(SIZE // WIDTH)
        for x in range(WIDTH)
    )
    synthetic = {
        "constant": b"\x4a" * SIZE,
        "ramp": bytes((i * 3) % 256 for i in range(SIZE)),
        "periodic": (b"MonaZip" * (SIZE // 7 + 1))[:SIZE],
        "stride_73": patterned,
        "noisy_matrix": raw_image,
        "random": rng.randbytes(SIZE),
    }
    cases = []
    for name, data in synthetic.items():
        path = root / name
        path.write_bytes(data)
        cases.append(("sentetik", path))

    rows = [
        (index, f"sensor-{index % 8}", round(16 + index % 90 * 0.138 + rng.random(), 3))
        for index in range(4500)
    ]
    csv_buffer = io.StringIO()
    writer = csv.writer(csv_buffer)
    writer.writerow(("tick", "sensor", "celsius"))
    writer.writerows(rows)
    csv_file = root / "telemetry.csv"
    csv_file.write_text(csv_buffer.getvalue())
    json_file = root / "telemetry.json"
    json_file.write_text(json.dumps([dict(zip(("tick", "sensor", "celsius"), row)) for row in rows]))
    db = root / "telemetry.sqlite"
    with sqlite3.connect(db) as connection:
        connection.execute("CREATE TABLE readings(tick INTEGER PRIMARY KEY, sensor TEXT, celsius REAL)")
        connection.executemany("INSERT INTO readings VALUES (?, ?, ?)", rows)
    image = root / "image.pgm"
    image.write_bytes(f"P5\n{WIDTH} {SIZE // WIDTH}\n255\n".encode() + raw_image)
    sensor = root / "sensor.i16"
    sensor.write_bytes(b"".join(struct.pack("<h", round(row[2] * 1000)) for row in rows))
    archive = root / "telemetry.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zip_file:
        zip_file.writestr(zipfile.ZipInfo("telemetry.json"), json_file.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
    examples = [csv_file, json_file, db, image, sensor, binary, archive]

    if sips := shutil.which("sips"):
        png_file = root / "image.png"
        write_png(png_file, raw_image)
        jpeg = root / "image.jpg"
        subprocess.run([sips, "-s", "format", "jpeg", str(png_file), "--out", str(jpeg)], check=True, capture_output=True)
        examples.append(jpeg)
    if openssl := shutil.which("openssl"):
        encrypted = root / "encrypted.bin"
        subprocess.run(
            [openssl, "enc", "-aes-256-ctr", "-K", "61" * 32, "-iv", "62" * 16, "-in", str(json_file), "-out", str(encrypted)],
            check=True,
        )
        examples.append(encrypted)
    return cases + [("dosya biçimi örneği", path) for path in examples]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/monazip"))
    parser.add_argument("--output", type=Path, default=Path("benchmarks/results.json"))
    parser.add_argument("--real", type=Path, nargs="*", default=[], help="Bağımsız corpus dosyaları")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    results = []
    with tempfile.TemporaryDirectory(prefix="monazip-bench-") as temp:
        cases = generate(Path(temp), binary)
        cases.extend(("bağımsız corpus", path) for path in args.real)
        for group, path in cases:
            with path.open("rb") as file:
                digest = hashlib.file_digest(file, "sha256").hexdigest()
            for level in (3, 9):
                invocation = [
                    str(binary), str(path), "--experimental", "--zstd-level", str(level), "--quiet", "--json",
                    "--detail-limit", "0", "--block-size", "65536", "--fixed-strides",
                ]
                report = json.loads(subprocess.check_output(invocation))
                if not report["verified"]:
                    raise RuntimeError(f"{path.name}: verification failed")
                fields = ("original_bytes", "experimental_bytes", "metadata_bytes", "zstd_stream_bytes",
                          "zstd_block_bytes", "savings_percent", "zstd_stream_gain_percent", "dominant_method",
                          "elapsed_ms", "throughput_mib_per_sec", "verified")
                results.append({"group": group, "sample": path.name, "sha256": digest, "level": level,
                                **{field: report[field] for field in fields}})
                print(f"{path.name:20} {level:2}  deneysel {report['experimental_bytes']:9,} B  "
                      f"Zstd akış {report['zstd_stream_bytes']:9,} B  {report['elapsed_ms']:5} ms")
    args.output.write_text(json.dumps({"system": platform.platform(), "results": results}, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
