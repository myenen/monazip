# MonaZip

MonaZip is a local, lossless compression tool for SQLite databases, text files, and images. It creates and restores Zstandard (`.zst`), JPEG XL (`.jxl`), and Lepton (`.lep`) archives, and compares their size and performance with gzip, ZIP, and optional RAR5. It includes a macOS desktop app and a command-line interface.

## Desktop app

Select or drop a SQLite database, text file, or image, choose a compression mode, and click **Sıkıştır ve karşılaştır** (Compress and compare). Use the save button to export the verified archive or **JSON kaydet** to export the measurements.

- **Hızlı · Zstd 9** (Fast) is the default. It uses two compression workers and a 128 MiB window to find repetitions farther apart.
- **Arşiv · Zstd 19** spends more time compressing to favor smaller archives.

To restore a file, click **Arşiv aç** (Open archive), select a `.zst`, `.zstd`, JPEG reconstruction `.jxl`, or Lepton `.lep` archive, and choose a new output path.

SQLite and JPEG, PNG, GIF, WebP, BMP, and TIFF images are recognized by their file signatures. Common text formats include SQL dumps, TXT, CSV, TSV, JSON, JSONL, XML, YAML, HTML, and logs. MonaZip processes the bytes of the selected file. For a consistent copy of a live SQLite database, use a backup produced by SQLite.

## How it works

| Input | Compression route |
|---|---|
| Text and SQLite | Zstandard with the selected mode. |
| JPEG | Try Zstandard, lossless JPEG XL, and Lepton. Compare the verified outputs and keep the smallest archive. |
| PNG, GIF, WebP | Zstandard preserves the original image file. Fast mode uses level 3 for these already compressed formats. |
| BMP and TIFF | Zstandard with the selected mode. |

JPEG XL uses the official `cjxl` and `djxl` tools when available. On macOS, install them with `brew install jpeg-xl`. To restore an original JPEG, MonaZip requires JPEG reconstruction data and checks that the decoder reports a successful reconstruction. [JPEG XL tools](https://github.com/libjxl/libjxl)

Lepton recompresses the JPEG's existing image data so the original JPEG can be reconstructed byte for byte. Build Microsoft's Rust implementation and put `lepton_jpeg_util` on your PATH, in `/opt/homebrew/bin` or `/usr/local/bin` for the macOS app, or under `.setup/bin` in a MonaZip checkout. You can also set `MONAZIP_LEPTON_PATH` to the full path of the executable in the process environment. MonaZip uses two workers and skips the Lepton trial if the helper is unavailable. Opening a `.lep` archive requires the helper. [Microsoft Rust Lepton](https://github.com/microsoft/lepton_jpeg_rust)

- **Streaming compression.** The Rust engine reads the source in chunks and feeds a single Zstandard stream. Zstandard represents repeated byte sequences using references to earlier data. The compression level controls how much work the encoder spends searching for matches. A larger window can increase memory use.
- **Lossless verification.** MonaZip includes a checksum in its Zstandard frames. JPEG XL stores JPEG reconstruction metadata. During a comparison it decompresses every measured archive and compares the result byte for byte with the source. Saving and restoring use a temporary file and publish the result to a new path after the operation succeeds.
- **Real format comparisons.** gzip and single-entry ZIP use DEFLATE level 6. When the official `rar` executable is available, MonaZip also creates a RAR5 archive at level 3 with one worker. Size includes archive headers. Compression time includes reading the source and writing the temporary archive. Decompression time includes the byte comparison. RAR timings also include launching the external process.
- **Experimental block encoding.** The CLI research mode samples block sizes and looks for useful distances between related bytes. It tries reversible transforms such as delta encoding, XOR, transposition, and bit shuffling. It chooses the smallest measured representation for each block, including metadata, then decodes the result and checks it against the source. [Research notes](MonaZip-PROJECT.md)

The desktop interface uses Tauri and React with TypeScript. The desktop app and CLI share the same Rust compression and extraction code.

## Command line

```sh
# Compare archive sizes and timings
./target/release/monazip database.sqlite
./target/release/monazip events.jsonl --json --quiet

# Create a verified archive
./target/release/monazip database.sqlite --compress-to database.sqlite.zst

# Save or restore a byte-exact JPEG XL archive
./target/release/monazip image.jpg --compress-to image.jpg.jxl
./target/release/monazip image.jpg.jxl --extract-to restored-jxl.jpg

# Save or restore a byte-exact Lepton archive
./target/release/monazip image.jpg --compress-to image.jpg.lep
./target/release/monazip image.jpg.lep --extract-to restored-lepton.jpg

# Restore the original bytes
./target/release/monazip database.sqlite.zst --extract-to restored.sqlite

# Run the experimental block encoder with the original fixed settings
./target/release/monazip --experimental sample.bin --block-size 65536 --fixed-strides
```

With a `.zst` output path, `--compress-to` runs Zstandard encoding and verification directly. A `.jxl` or `.lep` output path requests that format explicitly and saves the archive after byte-for-byte verification. For a `.zst` output path, add `--json` or `--output report.json` to run and report the other format comparisons too. Use `--zstd-level` to choose a compression level. Press **Ctrl+C** to cancel compression, verification, or extraction.

For RAR5 comparisons, install the official `rar` executable. MonaZip searches common install locations and the system path. Set `MONAZIP_RAR_PATH=/absolute/path/to/rar` to specify a different executable. The report shows RAR availability. [RARLAB downloads](https://www.rarlab.com/download.htm)

## Build from source

The macOS build was tested with Rust 1.100, Node.js 25, and Xcode. Tauri lists the platform dependencies for macOS, Windows, and Linux. [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --locked
cargo build --release --locked -p monazip
npm ci --prefix app
npm run build --prefix app
cargo test --release --locked -p monazip-desktop
npm run tauri --prefix app -- build --bundles app -- --locked
```

The macOS build produces an app bundle with an ad hoc signature for local use.

```sh
open target/release/bundle/macos/MonaZip.app
```

## Benchmarks

On the nine SQLite and text files in the checked-in benchmark, MonaZip at Zstd level 6 produced **962,242 bytes** in total. gzip produced **1,108,902 bytes**, ZIP **1,109,826 bytes**, and RAR5 **843,701 bytes**. MonaZip's aggregate compression time was **33.21 ms** on the tested Apple Silicon Mac. Results are specific to the files, machine, and levels used. [Benchmark results and methodology](benchmarks/README.md)

```sh
python3 benchmarks/db_text.py database.sqlite dump.sql report.csv
```
