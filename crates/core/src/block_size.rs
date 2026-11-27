use crate::{AnalysisError, BlockSizeTrial, block, check_cancel, codec, geometry, optimizer};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::atomic::AtomicBool,
};

const KIB: usize = 1024;
pub(crate) const DEFAULT: usize = 64 * KIB;
pub(crate) const MINIMUM_SAMPLE_BYTES: u64 = 16 * KIB as u64;
const SAMPLE_WINDOW: usize = 1024 * KIB;

pub(crate) fn select(
    source: &mut (impl Read + Seek),
    length: u64,
    compressor: &mut zstd::bulk::Compressor<'_>,
    auto_stride: bool,
    cancel: &AtomicBool,
) -> Result<(usize, Vec<BlockSizeTrial>), AnalysisError> {
    if length <= MINIMUM_SAMPLE_BYTES {
        return Ok((DEFAULT, Vec::new()));
    }

    let mut trials = Vec::<BlockSizeTrial>::new();
    for block_size in [DEFAULT, 16 * KIB, 256 * KIB, SAMPLE_WINDOW] {
        // Two sizes at least as large as the file produce the same single block.
        if trials
            .iter()
            .any(|trial| (trial.block_size as u64).min(length) == (block_size as u64).min(length))
        {
            continue;
        }
        trials.push(BlockSizeTrial {
            block_size,
            sampled_bytes: 0,
            encoded_bytes: codec::HEADER_BYTES as u64,
        });
    }

    let windows = length.div_ceil(SAMPLE_WINDOW as u64);
    let mut sample = vec![0; length.min(SAMPLE_WINDOW as u64) as usize];
    let mut previous = None;
    for window in [0, (windows - 1) / 2, windows - 1] {
        if previous == Some(window) {
            continue;
        }
        previous = Some(window);
        check_cancel(cancel)?;
        let offset = window * SAMPLE_WINDOW as u64;
        let size = (length - offset).min(SAMPLE_WINDOW as u64) as usize;
        source.seek(SeekFrom::Start(offset))?;
        if block::fill(source, &mut sample[..size])? != size {
            return Err(AnalysisError::SourceChanged);
        }
        for trial in &mut trials {
            trial.sampled_bytes += size as u64;
            for bytes in sample[..size].chunks(trial.block_size) {
                check_cancel(cancel)?;
                let baseline = compressor.compress(bytes)?;
                let geometry = geometry::explore(bytes, auto_stride, cancel)?;
                let encoded = optimizer::choose(bytes, baseline, &geometry, compressor, cancel)?;
                trial.encoded_bytes += encoded.size() as u64;
            }
        }
    }
    source.seek(SeekFrom::Start(0))?;
    let best = trials
        .iter()
        .min_by_key(|trial| trial.encoded_bytes)
        .unwrap();
    Ok((best.block_size, trials))
}
