use crate::{AnalysisError, block, check_cancel, unchanged};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
    sync::atomic::AtomicBool,
    time::Instant,
};

#[derive(Debug, Serialize)]
pub struct ExtractionReport {
    pub compressed_bytes: u64,
    pub restored_bytes: u64,
    pub elapsed_ms: u64,
}

fn output_exists() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Hedef dosya zaten var. Açılan dosya için yeni bir ad seçin.",
    )
}

pub fn extract_zstd(
    input: impl AsRef<Path>,
    destination: impl AsRef<Path>,
    mut progress: impl FnMut(u64),
    cancel: &AtomicBool,
) -> Result<ExtractionReport, AnalysisError> {
    check_cancel(cancel)?;
    let started = Instant::now();
    let input = input.as_ref();
    let destination = destination.as_ref();
    let before = input.metadata()?;
    if !before.is_file() {
        return Err(AnalysisError::NotAFile);
    }
    if destination.symlink_metadata().is_ok() {
        return Err(output_exists().into());
    }
    let source = File::open(input)?;
    if !unchanged(&before, &source.metadata()?) {
        return Err(AnalysisError::SourceChanged);
    }
    let directory = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(directory)?;

    let decoded = (|| {
        let mut decoder = zstd::stream::read::Decoder::new(&source)?;
        let mut writer = BufWriter::new(&mut output);
        let mut buffer = vec![0; 128 * 1024];
        let mut restored_bytes = 0;
        progress(0);
        loop {
            check_cancel(cancel)?;
            let count = block::fill(&mut decoder, &mut buffer).map_err(|error| {
                io::Error::new(error.kind(), format!("Zstd arşivi açılamadı ({error})."))
            })?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count])?;
            restored_bytes += count as u64;
            progress(restored_bytes);
        }
        writer.flush()?;
        Ok::<_, AnalysisError>(restored_bytes)
    })();

    if !unchanged(&before, &source.metadata()?)
        || !input
            .metadata()
            .is_ok_and(|after| unchanged(&before, &after))
    {
        return Err(AnalysisError::SourceChanged);
    }
    let restored_bytes = decoded?;
    check_cancel(cancel)?;
    if output.as_file().metadata()?.len() != restored_bytes {
        return Err(AnalysisError::Verification);
    }
    output.persist_noclobber(destination).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            output_exists()
        } else {
            error.error
        }
    })?;
    Ok(ExtractionReport {
        compressed_bytes: before.len(),
        restored_bytes,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}
