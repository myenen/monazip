use monazip_core::{AnalysisError, extract::extract_zstd};
use std::{
    fs,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

#[test]
fn extracts_concatenated_frames_and_protects_existing_paths() {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("sample.zst");
    let destination = work.path().join("sample.txt");
    let bytes = b"hello\0world\n".repeat(24_000);
    let compressed = [
        zstd::stream::encode_all(&bytes[..65536], 3).unwrap(),
        zstd::stream::encode_all(&bytes[65536..], 9).unwrap(),
    ]
    .concat();
    fs::write(&source, &compressed).unwrap();
    let cancel = AtomicBool::new(false);
    let mut restored = Vec::new();
    let result =
        extract_zstd(&source, &destination, |count| restored.push(count), &cancel).unwrap();
    assert_eq!(result.compressed_bytes, compressed.len() as u64);
    assert_eq!(result.restored_bytes, bytes.len() as u64);
    assert_eq!(restored.first(), Some(&0));
    assert_eq!(restored.last(), Some(&result.restored_bytes));
    assert_eq!(fs::read(&destination).unwrap(), bytes);
    assert!(extract_zstd(&source, &destination, |_| {}, &cancel).is_err());
    assert!(extract_zstd(&source, &source, |_| {}, &cancel).is_err());
    assert_eq!(fs::read(&source).unwrap(), compressed);
}

#[test]
fn invalid_or_truncated_streams_leave_no_partial_output() {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("input.zst");
    let destination = work.path().join("output.txt");
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.write_all(&vec![42; 260_000]).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut checksum_invalid = compressed.clone();
    *checksum_invalid.last_mut().unwrap() ^= 1;
    for data in [
        b"broken archive".as_slice(),
        &compressed[..compressed.len() - 1],
        &checksum_invalid,
    ] {
        fs::write(&source, data).unwrap();
        let result = extract_zstd(&source, &destination, |_| {}, &AtomicBool::new(false));
        assert!(result.is_err());
        assert!(!destination.exists());
        assert_eq!(work.path().read_dir().unwrap().count(), 1);
    }
}

#[test]
fn cancellation_discards_bytes_already_written() {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("input.zst");
    let destination = work.path().join("output.txt");
    let compressed = zstd::stream::encode_all(&vec![42; 260_000][..], 3).unwrap();
    fs::write(&source, compressed).unwrap();
    let cancel = AtomicBool::new(false);
    let result = extract_zstd(
        &source,
        &destination,
        |count| {
            if count != 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(result, Err(AnalysisError::Cancelled)));
    assert!(!destination.exists());
    assert_eq!(work.path().read_dir().unwrap().count(), 1);
}

#[test]
fn restores_large_archive_saved_with_parallel_long_matching() {
    use monazip_core::focused::{ZstdTuning, analyze_focused_with_tuning};
    use std::io::Read;

    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("original.sql");
    let archive = work.path().join("original.sql.zst");
    let restored = work.path().join("restored.sql");
    let mut source = fs::File::create(&input).unwrap();
    let prefix = b"INSERT INTO log VALUES (12345, 'first');\r\n".repeat(4096);
    source.write_all(&prefix).unwrap();
    // A distant repeat and a non-aligned tail exercise multiple decoder reads.
    let row = b"INSERT INTO log VALUES (54321, 'middle');\n".repeat(4096);
    for _ in 0..160 {
        source.write_all(&row).unwrap();
    }
    source.write_all(&prefix).unwrap();
    source.write_all(b"COMMIT;\r\n").unwrap();
    drop(source);

    let cancel = AtomicBool::new(false);
    let result =
        analyze_focused_with_tuning(&input, 9, ZstdTuning::ParallelLong, |_| {}, &cancel).unwrap();
    assert!(result.report.verified);
    result.save_archive(&archive).unwrap();
    let report = extract_zstd(&archive, &restored, |_| {}, &cancel).unwrap();
    assert_eq!(report.compressed_bytes, result.report.monazip.bytes);
    assert_eq!(report.restored_bytes, result.report.original_bytes);
    let digest = |path| {
        let mut source = fs::File::open(path).unwrap();
        let mut hasher = blake3::Hasher::new();
        let mut block = vec![0; 128 * 1024];
        loop {
            let len = source.read(&mut block).unwrap();
            if len == 0 {
                break;
            }
            hasher.update(&block[..len]);
        }
        hasher.finalize()
    };
    assert_eq!(digest(&input), digest(&restored));
}
