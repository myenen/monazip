use monazip_core::{
    AnalysisError,
    focused::{FileKind, Format, Stage, analyze_focused},
};
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
};

#[test]
fn exports_verified_zstd_without_replacing_source_or_existing_output() {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("report.TXT");
    let data = "Kayıpsız\r\nmetin\0".repeat(16_000).into_bytes();
    fs::write(&path, &data).unwrap();
    let result = analyze_focused(&path, 3, |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(matches!(result.report.kind, FileKind::Text));
    assert!(result.report.verified);
    assert!(result.report.monazip.bytes < data.len() as u64);
    let output = workspace.path().join("report.txt.zst");
    result.save_archive(&output).unwrap();
    assert_eq!(
        output.metadata().unwrap().len(),
        result.report.monazip.bytes
    );
    let decoded = zstd::stream::decode_all(fs::File::open(&output).unwrap()).unwrap();
    assert_eq!(decoded, data);
    assert!(result.save_archive(&path).is_err());
    assert!(result.save_archive(&output).is_err());
    assert_eq!(fs::read(&path).unwrap(), data);
}

#[test]
fn detects_sqlite_by_header_and_rejects_unrecognized_files() {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("database");
    let cancel = AtomicBool::new(false);
    fs::write(&path, b"SQLite format 3\0").unwrap();
    let result = analyze_focused(&path, 3, |_| {}, &cancel).unwrap();
    assert!(matches!(result.report.kind, FileKind::Sqlite));
    fs::write(&path, b"PK\x03\x04").unwrap();
    assert!(matches!(
        analyze_focused(&path, 3, |_| {}, &cancel),
        Err(AnalysisError::InvalidOptions(_))
    ));
}

#[test]
fn empty_text_and_cancellation_during_zip_verification() {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("empty.txt");
    let cancel = AtomicBool::new(false);
    fs::write(&path, []).unwrap();
    let report = analyze_focused(&path, 3, |_| {}, &cancel).unwrap().report;
    assert!(report.verified);
    assert_eq!(report.savings_percent, None);
    let result = analyze_focused(
        &path,
        3,
        |progress| {
            if matches!(
                (progress.format, progress.stage),
                (Format::Zip, Stage::Verifying)
            ) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(result, Err(AnalysisError::Cancelled)));
}

#[test]
fn verifies_all_formats_against_unchanged_source() {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("source.log");
    let initial = b"alfa\nbravo\n";
    fs::write(&path, initial).unwrap();
    let result = analyze_focused(
        &path,
        3,
        |progress| {
            if matches!(
                (progress.format, progress.stage),
                (Format::Gzip, Stage::Verifying)
            ) && progress.processed_bytes == 0
            {
                fs::write(&path, b"0000\n00000\n").unwrap();
            }
        },
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(AnalysisError::SourceChanged)));
}
