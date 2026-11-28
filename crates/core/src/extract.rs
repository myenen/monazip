use crate::{AnalysisError, block, check_cancel, jpegxl, lepton, unchanged};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Seek, Write},
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
    progress: impl FnMut(u64),
    cancel: &AtomicBool,
) -> Result<ExtractionReport, AnalysisError> {
    extract(
        input.as_ref(),
        destination.as_ref(),
        progress,
        cancel,
        false,
    )
}

pub fn extract_archive(
    input: impl AsRef<Path>,
    destination: impl AsRef<Path>,
    progress: impl FnMut(u64),
    cancel: &AtomicBool,
) -> Result<ExtractionReport, AnalysisError> {
    extract(input.as_ref(), destination.as_ref(), progress, cancel, true)
}

fn extract(
    input: &Path,
    destination: &Path,
    mut progress: impl FnMut(u64),
    cancel: &AtomicBool,
    allow_image_archive: bool,
) -> Result<ExtractionReport, AnalysisError> {
    check_cancel(cancel)?;
    let started = Instant::now();
    let before = input.metadata()?;
    if !before.is_file() {
        return Err(AnalysisError::NotAFile);
    }
    if destination.symlink_metadata().is_ok() {
        return Err(output_exists().into());
    }
    let mut source = File::open(input)?;
    if !unchanged(&before, &source.metadata()?) {
        return Err(AnalysisError::SourceChanged);
    }
    let directory = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(directory)?;

    let mut signature = [0; 12];
    let count = if allow_image_archive {
        block::fill(&mut source, &mut signature)?
    } else {
        0
    };
    let signature = &signature[..count];
    let is_jpeg_xl =
        signature.starts_with(jpegxl::CONTAINER_SIGNATURE) || signature.starts_with(b"\xff\x0a");
    let is_lepton = signature.starts_with(lepton::SIGNATURE);
    source.rewind()?;
    let decoded = (|| {
        progress(0);
        let decoded_jpeg;
        let mut decoder: Box<dyn Read> = if is_jpeg_xl || is_lepton {
            if !destination
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg")
                })
            {
                return Err(AnalysisError::InvalidOptions(
                    "Geri açılan JPEG için .jpg veya .jpeg uzantılı bir hedef seçin.",
                ));
            }
            decoded_jpeg = tempfile::tempdir()?;
            let path = decoded_jpeg.path().join("restored.jpg");
            if is_lepton {
                let tool = lepton::find().ok_or(AnalysisError::InvalidOptions(
                    "Lepton arşivini açmak için lepton_jpeg_util aracı gerekli.",
                ))?;
                lepton::convert(&tool, source.try_clone()?, &path, false, cancel)?;
            } else {
                let tool = jpegxl::decoder().ok_or(AnalysisError::InvalidOptions(
                    "JPEG XL arşivini açmak için djxl aracı gerekli.",
                ))?;
                jpegxl::reconstruct(&tool, source.try_clone()?, &path, cancel)?;
            }
            Box::new(BufReader::new(File::open(path)?))
        } else {
            Box::new(zstd::stream::read::Decoder::new(&source)?)
        };
        let mut writer = BufWriter::new(&mut output);
        let mut buffer = vec![0; 128 * 1024];
        let mut restored_bytes = 0;
        loop {
            check_cancel(cancel)?;
            let count = block::fill(&mut decoder, &mut buffer).map_err(|error| {
                io::Error::new(error.kind(), format!("Arşiv açılamadı ({error})."))
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
