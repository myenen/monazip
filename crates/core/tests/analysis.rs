use monazip_core::{AnalysisError, Options, Phase, analyze};
use std::{
    fs,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

#[test]
fn whole_file_and_block_sizes_include_real_overhead() {
    let bytes = [vec![55; 4096], (0..4099).map(|i| (i % 7) as u8).collect()].concat();
    let mut path = tempfile::NamedTempFile::new().unwrap();
    path.write_all(&bytes).unwrap();
    let report = analyze(
        path.path(),
        Options {
            block_size: 4096,
            ..Options::default()
        },
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(report.verified);
    assert_eq!(report.block_count, 3);
    assert_eq!(report.blocks[2].original_bytes, 3);
    assert_eq!(
        report.experimental_bytes,
        49 + report
            .blocks
            .iter()
            .map(|block| block.experimental_bytes as u64)
            .sum::<u64>()
    );
    assert_eq!(
        report.zstd_block_bytes,
        report
            .blocks
            .iter()
            .map(|block| block.zstd_bytes as u64)
            .sum::<u64>()
    );
    let stream = zstd::stream::encode_all(bytes.as_slice(), 3).unwrap();
    assert_eq!(report.zstd_stream_bytes, stream.len() as u64);
    assert!(report.entropy_bits_per_byte.unwrap() > 1.0);
    assert_eq!(fs::read(path.path()).unwrap(), bytes);
}

#[test]
fn empty_file_has_no_ratios_and_short_raw_reports_growth() {
    let mut path = tempfile::NamedTempFile::new().unwrap();
    let report = analyze(
        path.path(),
        Options::default(),
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(report.verified);
    assert_eq!(report.experimental_bytes, 49);
    assert_eq!(report.savings_percent, None);
    assert_eq!(report.zstd_block_gain_percent, None);
    path.write_all(b"ABC").unwrap();
    let report = analyze(
        path.path(),
        Options::default(),
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(report.savings_percent.unwrap() < 0.0);
    assert_eq!(report.blocks[0].model, "RAW");
}

#[test]
fn cancellation_during_verification_is_detected() {
    let mut path = tempfile::NamedTempFile::new().unwrap();
    path.write_all(b"abcabcabcabc").unwrap();
    let flag = AtomicBool::new(false);
    let result = analyze(
        path.path(),
        Options::default(),
        |progress| {
            if matches!(progress.phase, Phase::Verifying) {
                flag.store(true, Ordering::Relaxed);
            }
        },
        &flag,
    );
    assert!(matches!(result, Err(AnalysisError::Cancelled)));
}

#[test]
fn modification_during_verification_is_detected() {
    let mut path = tempfile::NamedTempFile::new().unwrap();
    path.write_all(b"abcabcabcabc").unwrap();
    let result = analyze(
        path.path(),
        Options::default(),
        |progress| {
            if matches!(progress.phase, Phase::Verifying) && progress.processed_bytes == 0 {
                fs::write(path.path(), b"123123123123").unwrap();
            }
        },
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(AnalysisError::SourceChanged)));
}

#[test]
fn searches_for_stride_outside_fixed_candidates() {
    let mut random = 321_u32;
    let mut next = || {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        random
    };
    let mut bytes: Vec<u8> = (0..73).map(|_| next() as u8).collect();
    for index in 73..24 * 1024 {
        let step = (next() % 3) as u8;
        bytes.push(bytes[index - 73].wrapping_add(step).wrapping_sub(1));
    }
    let mut path = tempfile::NamedTempFile::new().unwrap();
    path.write_all(&bytes).unwrap();
    let cancel = AtomicBool::new(false);
    let fixed = analyze(
        path.path(),
        Options {
            auto_stride: false,
            ..Options::default()
        },
        |_| {},
        &cancel,
    )
    .unwrap();
    let automatic = analyze(path.path(), Options::default(), |_| {}, &cancel).unwrap();
    assert!(automatic.verified);
    assert_eq!(automatic.blocks[0].stride, Some(73));
    assert!(automatic.experimental_bytes < fixed.experimental_bytes / 2);
}

#[test]
fn automatic_block_size_uses_the_lowest_measured_cost() {
    let path = tempfile::NamedTempFile::new().unwrap();
    fs::write(path.path(), b"MonaZip".repeat(5000)).unwrap();
    let cancel = AtomicBool::new(false);
    let report = analyze(
        path.path(),
        Options {
            auto_block_size: true,
            ..Options::default()
        },
        |_| {},
        &cancel,
    )
    .unwrap();
    assert!(report.verified);
    assert_eq!(report.block_size_trials.len(), 2);
    assert!(
        report
            .block_size_trials
            .iter()
            .all(|trial| trial.sampled_bytes == report.original_bytes)
    );
    let best = report
        .block_size_trials
        .iter()
        .min_by_key(|trial| trial.encoded_bytes)
        .unwrap();
    assert_eq!(report.options.block_size, best.block_size);
    assert_eq!(report.experimental_bytes, best.encoded_bytes);
}

#[test]
fn cancellation_during_block_size_selection_is_detected() {
    let path = tempfile::NamedTempFile::new().unwrap();
    fs::write(path.path(), vec![0; 17 * 1024]).unwrap();
    let cancel = AtomicBool::new(false);
    let result = analyze(
        path.path(),
        Options {
            auto_block_size: true,
            ..Options::default()
        },
        |progress| {
            if matches!(progress.phase, Phase::SelectingBlockSize) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(result, Err(AnalysisError::Cancelled)));
}
