use crate::{AnalysisError, check_cancel, codec_tool};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
};

pub(crate) const CONTAINER_SIGNATURE: &[u8; 12] = b"\0\0\0\x0cJXL \r\n\x87\n";

pub(crate) struct JpegXl {
    encoder: PathBuf,
    pub decoder: PathBuf,
}

impl JpegXl {
    pub(crate) fn find() -> Option<Self> {
        Some(Self {
            encoder: codec_tool::executable("cjxl")?,
            decoder: decoder()?,
        })
    }

    pub(crate) fn compress(
        &self,
        input: &Path,
        output: &Path,
        effort: i32,
        cancel: &AtomicBool,
    ) -> Result<(), AnalysisError> {
        let mut command = Command::new(&self.encoder);
        command
            .args([input, output])
            .args(["--lossless_jpeg=1", "--num_threads=2"])
            .arg(format!("--effort={effort}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        codec_tool::run(command, "JPEG XL", cancel)
    }
}

pub(crate) fn decoder() -> Option<PathBuf> {
    codec_tool::executable("djxl")
}

pub(crate) fn reconstruct(
    decoder: &Path,
    mut archive: File,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<(), AnalysisError> {
    if !has_reconstruction_data(&mut archive, cancel)? {
        return Err(AnalysisError::InvalidOptions(
            "Bu JPEG XL dosyası orijinal JPEG’i geri açmak için gereken veriyi içermiyor.",
        ));
    }
    archive.rewind()?;
    let mut diagnostics = tempfile::tempfile()?;
    let mut command = Command::new(decoder);
    command
        .arg("-")
        .arg(output)
        .arg("--num_threads=2")
        .stdin(Stdio::from(archive))
        .stdout(Stdio::null())
        .stderr(Stdio::from(diagnostics.try_clone()?));
    codec_tool::run(command, "JPEG XL", cancel)?;
    diagnostics.rewind()?;
    let mut messages = Vec::new();
    diagnostics.take(64 * 1024).read_to_end(&mut messages)?;
    if !String::from_utf8_lossy(&messages)
        .lines()
        .any(|line| line.trim() == "Reconstructed to JPEG.")
    {
        return Err(AnalysisError::Verification);
    }
    Ok(())
}

fn has_reconstruction_data(input: &mut File, cancel: &AtomicBool) -> Result<bool, AnalysisError> {
    input.rewind()?;
    let len = input.metadata()?.len();
    let mut signature = [0; 12];
    if crate::block::fill(input, &mut signature)? != signature.len()
        || &signature != CONTAINER_SIGNATURE
    {
        return Ok(false);
    }
    while input.stream_position()? < len {
        check_cancel(cancel)?;
        let start = input.stream_position()?;
        let mut header = [0; 8];
        input.read_exact(&mut header)?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let (size, header_size) = match size {
            0 => (len - start, 8),
            1 => {
                let mut length = [0; 8];
                input.read_exact(&mut length)?;
                (u64::from_be_bytes(length), 16)
            }
            size => (size, 8),
        };
        if size < header_size || size > len - start {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "JPEG XL kutu uzunluğu geçersiz.",
            )
            .into());
        }
        if &header[4..] == b"jbrd" {
            return Ok(size > header_size);
        }
        input.seek(SeekFrom::Start(start + size))?;
    }
    Ok(false)
}
