# Benchmark results

## SQLite and text compression

Nine SQLite and text files were benchmarked on Apple Silicon macOS. MonaZip used Zstd level 6 in Balanced mode. gzip and ZIP used DEFLATE level 6. RARLAB 7.23 produced RAR5 archives at level 3 with one worker. RAR timings include launching the external process.

The table sums the median times from five runs per file. Compression time includes reading the source and writing a temporary archive. Decompression and verification time includes comparing the restored bytes with the source.

| Format | Total archive size | Compression | Decompression and verification |
|---|---:|---:|---:|
| MonaZip (.zst) | 962,242 B | 33.21 ms | 4.02 ms |
| gzip (.gz) | 1,108,902 B | 39.90 ms | 6.15 ms |
| ZIP (.zip) | 1,109,826 B | 39.49 ms | 6.31 ms |
| RAR (.rar) | 843,701 B | 434.26 ms | 51.94 ms |

MonaZip produced smaller archives than gzip on 8 of 9 files and smaller archives than ZIP on all 9. For Chinook SQLite, MonaZip produced 328,078 B, gzip 421,797 B, ZIP 421,905 B, and RAR 225,860 B. gzip won on `cp.html` with 7,967 B, compared with MonaZip's 8,025 B. RAR produced 12.3% fewer bytes than MonaZip across the full set.

The same nine files were used to compare compression levels for Balanced mode. All 180 archives across the four formats were decompressed and verified byte for byte against their sources.

The SQL dump was generated from the Chinook SQLite database. The other samples included a USGS CSV export, an npm JSON lockfile, and text files from Canterbury. File-level results are in [db-text-results.json](db-text-results.json). [Chinook](https://github.com/lerocha/chinook-database) · [Canterbury Corpus](https://corpus.canterbury.ac.nz/descriptions/) · [USGS catalog query](https://earthquake.usgs.gov/fdsnws/event/1/query?format=csv&starttime=2024-01-01&endtime=2024-03-01&minmagnitude=4.5)

```sh
python3 benchmarks/db_text.py database.sqlite dump.sql report.csv
```

## Earlier experimental block encoder

The experimental encoder was tested on 19 files with 64 KiB blocks and two Zstd levels on Apple Silicon macOS. All 38 experimental outputs were decoded and verified against the source byte for byte and with BLAKE3. Sizes are in bytes.

### Independent samples

| File | Experimental level 3 | Zstd stream level 3 | Experimental level 9 | Zstd stream level 9 |
|---|---:|---:|---:|---:|
| USGS CSV | 66,974 | 64,367 | 62,750 | 58,627 |
| libjpeg-turbo JPEG | 5,571 | 5,606 | 5,557 | 5,601 |
| Chinook SQLite | 332,671 | 353,237 | 306,231 | 323,644 |
| MonaZip npm lockfile | 9,575 | 9,514 | 9,024 | 9,027 |

The experimental container was 5.82% smaller than the Zstd stream at level 3 and 5.38% smaller at level 9 for Chinook SQLite. For USGS CSV, it was 4.05% larger at level 3 and 7.03% larger at level 9.

The external samples came from the USGS catalog, the libjpeg-turbo test images, and the Chinook database project. [USGS catalog query](https://earthquake.usgs.gov/fdsnws/event/1/query?format=csv&starttime=2024-01-01&endtime=2024-03-01&minmagnitude=4.5) · [libjpeg-turbo 3.1.2 JPEG sample](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/3.1.2/testimages/testorig.jpg) · [Chinook SQLite sample](https://github.com/lerocha/chinook-database/blob/master/ChinookDatabase/DataSources/Chinook_Sqlite.sqlite)

### Selected synthetic samples

| File | Source | Experimental level 3 | Zstd stream level 3 |
|---|---:|---:|---:|
| Ramp | 262,144 | 177 | 291 |
| 73-byte row dependency | 262,144 | 213 | 18,736 |
| Noisy matrix | 262,144 | 129,928 | 221,479 |
| Random bytes | 262,144 | 262,241 | 262,159 |

Container overhead outweighed the savings for some files. Whole-file Zstd streaming also found repetitions across the experimental block boundaries. The synthetic samples show how the encoder responds to specific patterns. The independent sample set contains four files.

[results.json](results.json) also records SHA-256 hashes, timings, dominant methods, and the per-block Zstd baseline. Use `run.py --real` to test additional files.

## Automatic stride and block-size selection

Eleven Canterbury Corpus files were benchmarked at Zstd level 3 with four configurations. The fixed configuration uses 64 KiB blocks and the original list of candidate strides. Analysis time is the sum of per-file times. Peak RSS is the highest resident memory usage recorded for a process in the run. [Canterbury Corpus](https://corpus.canterbury.ac.nz/descriptions/)

| Configuration | Total container size | Analysis time | Peak RSS | Larger than source |
|---|---:|---:|---:|---:|
| Fixed | 616,367 B | 0.94 s | 11.1 MiB | 0/11 |
| Automatic stride only | 616,367 B | 1.02 s | 10.2 MiB | 0/11 |
| Automatic block size only | 571,044 B | 4.62 s | 24.5 MiB | 0/11 |
| Both automatic | 571,044 B | 5.07 s | 24.4 MiB | 0/11 |

Whole-file Zstd streaming produced 633,109 B in total. Enabling both options reduced output size by 7.35% relative to the fixed MonaZip settings and by 9.80% relative to the Zstd stream. All 44 outputs across the four configurations were verified byte for byte. Automatic stride discovery yielded the same output size on this corpus.

On a 256 KiB synthetic sample with a dependency every 73 bytes, the fixed configuration produced 259,049 B. Automatic stride alone produced 69,525 B, and both options together produced 67,453 B. All three outputs were verified. The sample used Python's `random.Random(321)`. After the first 73 bytes, each byte was generated by adding a randomly chosen −1, 0, or +1 to the value 73 positions earlier, modulo 256.

[adaptive-results.json](adaptive-results.json) contains per-file measurements, selected block sizes, and source SHA-256 hashes. To repeat the benchmark, extract the corpus into a directory and pass that directory to the script.

```sh
python3 benchmarks/adaptive.py .setup/canterbury/files
```
