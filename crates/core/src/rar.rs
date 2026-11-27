use crate::{AnalysisError, check_cancel};
use std::{
    env,
    fs::File,
    io,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::atomic::AtomicBool,
    thread,
    time::Duration,
};

pub(crate) const LEVEL: i32 = 3;
const EXECUTABLE: &str = if cfg!(windows) { "rar.exe" } else { "rar" };

pub(crate) fn find() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MONAZIP_RAR_PATH") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }

    let from_path = env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(EXECUTABLE))
            .find(|candidate| candidate.is_file())
    });
    if from_path.is_some() {
        return from_path;
    }
    #[cfg(windows)]
    if let Some(program_files) = env::var_os("ProgramFiles") {
        let candidate = PathBuf::from(program_files).join("WinRAR").join(EXECUTABLE);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    #[cfg(unix)]
    for directory in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let candidate = Path::new(directory).join(EXECUTABLE);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let launcher = env::current_exe().ok()?;
    launcher.ancestors().find_map(|directory| {
        let candidate = directory.join(".setup/rar/rar").join(EXECUTABLE);
        (directory.join("Cargo.toml").is_file() && candidate.is_file()).then_some(candidate)
    })
}

fn command(tool: &Path, action: &str) -> Command {
    let mut command = Command::new(tool);
    command
        .args([action, "-cfg-", "-inul", "-y", "-mt1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub(crate) fn compress(
    tool: &Path,
    source: File,
    archive: &Path,
    name: &str,
    cancel: &AtomicBool,
) -> Result<(), AnalysisError> {
    Process(
        command(tool, "a")
            .arg(format!("-si{name}"))
            .arg(format!("-m{LEVEL}"))
            .arg("-ma5")
            .arg(archive)
            .stdin(Stdio::from(source))
            .spawn()?,
    )
    .finish(cancel)
}

pub(crate) fn decompress(tool: &Path, archive: &Path) -> io::Result<(Process, ChildStdout)> {
    let mut process = Process(
        command(tool, "p")
            .arg(archive)
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let output = process.0.stdout.take().expect("RAR çıktı borusu açıldı");
    Ok((process, output))
}

pub(crate) struct Process(Child);

impl Process {
    pub(crate) fn finish(mut self, cancel: &AtomicBool) -> Result<(), AnalysisError> {
        loop {
            check_cancel(cancel)?;
            if let Some(status) = self.0.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("RAR işlemi başarısız oldu ({status}).")).into())
                };
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
