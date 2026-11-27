# MonaZip

MonaZip is a local, lossless compression tool for SQLite databases and text files. It creates standard Zstandard (`.zst`) archives, restores them, and compares their size and performance with gzip, ZIP, and optional RAR5. It includes a macOS desktop app and a command-line interface.

## Desktop app

Select or drop a SQLite or text file, choose a compression mode, and click **Sıkıştır ve karşılaştır** (Compress and compare). Use **.zst kaydet** to save the verified archive or **JSON kaydet** to export the measurements.

To restore a file, click **.zst aç** (Open .zst), select a `.zst` or `.zstd` archive, and choose a new output path. The app suggests a name by removing the archive extension.

SQLite files are recognized by their header. Common text formats include SQL dumps, TXT, CSV, TSV, JSON, JSONL, XML, YAML, HTML, and logs. MonaZip processes the bytes of the selected file. For a consistent copy of a live SQLite database, use a backup produced by SQLite.

## How it works

- **Streaming compression.** The Rust engine reads the source in chunks and feeds a single Zstandard stream. Zstandard represents repeated byte sequences using references to earlier data. Levels 3, 6, 9, 15, and 19 trade compression time for archive size. The desktop app also offers level 9 with two compression workers, with or without a 128 MiB long-distance matching window. The larger window can find repetitions farther apart and can increase memory use.
- **Lossless verification.** MonaZip includes a checksum in its Zstandard frames. During a comparison it decompresses every measured archive and compares the result byte for byte with the source. Saving and restoring use a temporary file and publish the result to a new path after the operation succeeds.
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

# Restore the original bytes
./target/release/monazip database.sqlite.zst --extract-to restored.sqlite

# Run the experimental block encoder with the original fixed settings
./target/release/monazip --experimental sample.bin --block-size 65536 --fixed-strides
```

`--compress-to` runs Zstandard encoding and verification directly. Add `--json` or `--output report.json` to run and report the other format comparisons too. Use `--zstd-level` to choose a compression level. Press **Ctrl+C** to cancel compression, verification, or extraction.

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
