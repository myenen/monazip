mod block;
mod block_size;
mod codec;
pub mod extract;
pub mod focused;
mod geometry;
mod models;
mod optimizer;
mod rar;
mod report;
mod residual;
mod statistics;
mod verify;

pub use geometry::{Direction, Neighbor};
pub use report::{
    AnalysisReport, BlockDetail, BlockSizeTrial, Options, Phase, Progress, StrideSummary,
};

use std::{
    collections::BTreeMap,
    fmt,
    fs::{File, Metadata},
    io::{self, BufReader, BufWriter, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

const MAX_BLOCK_SIZE: usize = 1024 * 1024;
const MAX_DETAILS: usize = 1024;

#[derive(Debug)]
pub enum AnalysisError {
    Io(io::Error),
    InvalidOptions(&'static str),
    NotAFile,
    Cancelled,
    SourceChanged,
    Verification,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::InvalidOptions(message) => formatter.write_str(message),
            Self::NotAFile => formatter.write_str("Okunabilir bir normal dosya seçin."),
            Self::Cancelled => formatter.write_str("Analiz iptal edildi."),
            Self::SourceChanged => formatter.write_str("Kaynak dosya analiz sırasında değişti."),
            Self::Verification => {
                formatter.write_str("Deneysel çıktı kaynakla birebir doğrulanamadı.")
            }
        }
    }
}

impl std::error::Error for AnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for AnalysisError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<(), AnalysisError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }
    Ok(())
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.ino() != after.ino()
            || before.dev() != after.dev()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return false;
        }
    }
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}

pub fn analyze(
    path: impl AsRef<Path>,
    mut options: Options,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<AnalysisReport, AnalysisError> {
    let path = path.as_ref();
    let started = Instant::now();
    let elapsed_ms = || started.elapsed().as_millis() as u64;
    if !(1..=MAX_BLOCK_SIZE).contains(&options.block_size) {
        return Err(AnalysisError::InvalidOptions(
            "Blok boyutu 1–1.048.576 bayt olmalıdır.",
        ));
    }
    if !(1..=22).contains(&options.zstd_level) {
        return Err(AnalysisError::InvalidOptions(
            "Zstd seviyesi 1–22 arasında olmalıdır.",
        ));
    }
    if options.detail_limit > MAX_DETAILS {
        return Err(AnalysisError::InvalidOptions(
            "En fazla 1024 blok detayı alınabilir.",
        ));
    }
    check_cancel(cancel)?;
    let before = path.metadata()?;
    if !before.is_file() {
        return Err(AnalysisError::NotAFile);
    }
    let file = File::open(path)?;
    if !unchanged(&before, &file.metadata()?) {
        return Err(AnalysisError::SourceChanged);
    }
    let original_bytes = before.len();
    let mut notify = |phase, processed_bytes| {
        progress(Progress {
            phase,
            processed_bytes,
            total_bytes: original_bytes,
            elapsed_ms: elapsed_ms(),
        })
    };

    let mut source = BufReader::new(file);
    let mut compressor = zstd::bulk::Compressor::new(options.zstd_level)?;
    let mut block_size_trials = Vec::new();
    if options.auto_block_size {
        if original_bytes > block_size::MINIMUM_SAMPLE_BYTES {
            notify(Phase::SelectingBlockSize, 0);
        }
        (options.block_size, block_size_trials) = block_size::select(
            &mut source,
            original_bytes,
            &mut compressor,
            options.auto_stride,
            cancel,
        )?;
        if !unchanged(&before, &source.get_ref().metadata()?) {
            return Err(AnalysisError::SourceChanged);
        }
    }
    let mut container = BufWriter::new(tempfile::tempfile()?);
    codec::write_header(&mut container, original_bytes, options.block_size, &[0; 32])?;
    let mut metadata_bytes = codec::HEADER_BYTES as u64;
    let mut zstd_block_bytes = 0;
    let mut block_count = 0;
    let mut processed = 0;
    let mut hash = blake3::Hasher::new();
    let mut stream =
        zstd::stream::write::Encoder::new(block::ByteCount::default(), options.zstd_level)?;
    let mut buffer = vec![0; options.block_size];
    let mut file_counts = [0; 256];
    let mut repeats = 0;
    let mut previous_byte = None;
    let mut blocks = Vec::new();
    let mut model_counts = BTreeMap::new();
    let mut method_bytes = BTreeMap::new();
    let mut stride_counts = BTreeMap::new();

    notify(Phase::Analyzing, 0);
    loop {
        check_cancel(cancel)?;
        let len = block::fill(&mut source, &mut buffer)?;
        if len == 0 {
            break;
        }
        if len as u64 > original_bytes - processed {
            return Err(AnalysisError::SourceChanged);
        }
        let bytes = &buffer[..len];
        hash.update(bytes);
        stream.write_all(bytes)?;
        let baseline = compressor.compress(bytes)?;
        let baseline_len = baseline.len();
        zstd_block_bytes += baseline_len as u64;
        let counts = statistics::histogram(bytes);
        for (total, count) in file_counts.iter_mut().zip(counts) {
            *total += count;
        }
        let block_repeats = bytes.windows(2).filter(|pair| pair[0] == pair[1]).count() as u64;
        repeats += block_repeats + u64::from(previous_byte == bytes.first().copied());
        previous_byte = bytes.last().copied();

        let geometry = geometry::explore(bytes, options.auto_stride, cancel)?;
        for neighbor in geometry.strides.iter().chain(&geometry.discovered) {
            *stride_counts.entry(neighbor.width).or_insert(0) += 1;
        }
        let best = optimizer::choose(bytes, baseline, &geometry, &mut compressor, cancel)?;
        best.write(&mut container, len)?;
        metadata_bytes += best.metadata_size() as u64;
        *model_counts.entry(best.model.name()).or_insert(0) += 1;
        let method = format!("{} + {}", best.model.name(), best.codec_name());
        *method_bytes.entry(method).or_insert(0) += len as u64;
        if blocks.len() < options.detail_limit {
            let stride = if best.model.uses_width() {
                Some(best.model.validate(&best.params, len)?)
            } else {
                None
            };
            blocks.push(BlockDetail {
                index: block_count,
                offset: processed,
                original_bytes: len,
                experimental_bytes: best.size(),
                zstd_bytes: baseline_len,
                model: best.model.name(),
                codec: best.codec_name(),
                parameter_bytes: best.params.len(),
                stride,
                entropy_bits_per_byte: statistics::entropy(&counts, len as u64),
                zero_percent: statistics::percent(counts[0], len as u64).unwrap_or_default(),
                adjacent_repeat_percent: statistics::percent(
                    block_repeats,
                    len.saturating_sub(1) as u64,
                )
                .unwrap_or_default(),
                top_neighbors: geometry
                    .strides
                    .into_iter()
                    .chain(geometry.discovered)
                    .chain(geometry.spatial)
                    .collect(),
            });
        }
        block_count += 1;
        processed += len as u64;
        notify(Phase::Analyzing, processed);
    }
    if processed != original_bytes || !unchanged(&before, &source.get_ref().metadata()?) {
        return Err(AnalysisError::SourceChanged);
    }
    let zstd_stream_bytes = stream.finish()?.0;
    let experimental_bytes = container.stream_position()?;
    let digest = hash.finalize();
    container.seek(SeekFrom::Start(0))?;
    codec::write_header(
        &mut container,
        original_bytes,
        options.block_size,
        digest.as_bytes(),
    )?;
    container.flush()?;
    notify(Phase::Verifying, 0);
    let mut encoded = BufReader::new(container.get_mut());
    let verification = verify::verify(
        &mut source,
        &mut encoded,
        original_bytes,
        options.block_size,
        digest.as_bytes(),
        cancel,
        |done| {
            notify(Phase::Verifying, done);
        },
    );
    if !unchanged(&before, &source.get_ref().metadata()?)
        || !path
            .metadata()
            .is_ok_and(|after| unchanged(&before, &after))
    {
        return Err(AnalysisError::SourceChanged);
    }
    verification?;

    let mut warnings = Vec::new();
    if original_bytes == 0 {
        warnings.push("Boş dosya için oranlar hesaplanmadı.");
    } else if experimental_bytes > original_bytes {
        warnings.push("Deneysel çıktı, kapsayıcı ek yükü dahil kaynak dosyadan daha büyük.");
    }
    if block_count > blocks.len() as u64 {
        warnings.push("Blok ayrıntıları rapordaki blok sınırıyla kısıtlandı.");
    }
    let mut top_strides: Vec<_> = stride_counts
        .into_iter()
        .map(|(stride, selected_for_trials)| StrideSummary {
            stride,
            selected_for_trials,
        })
        .collect();
    top_strides.sort_by_key(|item| (std::cmp::Reverse(item.selected_for_trials), item.stride));
    top_strides.truncate(8);
    let dominant_method = method_bytes
        .iter()
        .max_by_key(|(_, bytes)| *bytes)
        .map(|(name, _)| name.clone());
    Ok(AnalysisReport {
        original_bytes,
        experimental_bytes,
        metadata_bytes,
        zstd_stream_bytes,
        zstd_block_bytes,
        savings_percent: statistics::savings(experimental_bytes, original_bytes),
        compression_ratio: (original_bytes > 0)
            .then(|| original_bytes as f64 / experimental_bytes as f64),
        zstd_stream_gain_percent: (original_bytes > 0)
            .then(|| statistics::savings(experimental_bytes, zstd_stream_bytes))
            .flatten(),
        zstd_block_gain_percent: (original_bytes > 0)
            .then(|| statistics::savings(experimental_bytes, zstd_block_bytes))
            .flatten(),
        verified: true,
        elapsed_ms: elapsed_ms(),
        throughput_mib_per_sec: original_bytes as f64
            / 1_048_576.0
            / started.elapsed().as_secs_f64().max(1e-9),
        block_count,
        model_counts,
        method_bytes,
        dominant_method,
        top_strides,
        entropy_bits_per_byte: (original_bytes > 0)
            .then(|| statistics::entropy(&file_counts, original_bytes)),
        zero_percent: statistics::percent(file_counts[0], original_bytes),
        adjacent_repeat_percent: statistics::percent(repeats, original_bytes.saturating_sub(1)),
        options,
        block_size_trials,
        warnings,
        blocks,
    })
}
