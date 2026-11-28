use crate::{AnalysisError, check_cancel};
use std::{
    env,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    thread,
    time::Duration,
};

pub(crate) const CONTAINER_SIGNATURE: &[u8; 12] = b"\0\0\0\x0cJXL \r\n\x87\n";

fn executable(name: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let entries = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&entries)
        .chain(
            ["/opt/homebrew/bin", "/usr/local/bin"]
                .into_iter()
                .map(PathBuf::from),
        )
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(&name))
        .find(|candidate| candidate.is_file())
}

pub(crate) struct JpegXl {
    encoder: PathBuf,
    pub decoder: PathBuf,
}

impl JpegXl {
    pub(crate) fn find() -> Option<Self> {
        Some(Self {
            encoder: executable("cjxl")?,
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
            .stderr(Stdio::null());
        run(command, cancel)
    }
}

pub(crate) fn decoder() -> Option<PathBuf> {
    executable("djxl")
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
        .stderr(Stdio::from(diagnostics.try_clone()?));
    run(command, cancel)?;
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

fn run(mut command: Command, cancel: &AtomicBool) -> Result<(), AnalysisError> {
    check_cancel(cancel)?;
    command.stdout(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    let result = loop {
        if let Err(error) = check_cancel(cancel) {
            break Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(
                    io::Error::other(format!("JPEG XL işlemi başarısız oldu ({status}).")).into(),
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(1)),
            Err(error) => break Err(error.into()),
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    result
}
