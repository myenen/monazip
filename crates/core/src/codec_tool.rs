use crate::{AnalysisError, check_cancel};
use std::{
    env, io, path::PathBuf, process::Command, sync::atomic::AtomicBool, thread, time::Duration,
};
pub(crate) fn executable(name: &str) -> Option<PathBuf> {
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

pub(crate) fn run(
    mut command: Command,
    name: &str,
    cancel: &AtomicBool,
) -> Result<(), AnalysisError> {
    check_cancel(cancel)?;
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
                    io::Error::other(format!("{name} işlemi başarısız oldu ({status}).")).into(),
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
