use crate::{AnalysisError, codec_tool};
use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
};

pub(crate) const SIGNATURE: &[u8; 2] = b"\xcf\x84";
const EXECUTABLE: &str = if cfg!(windows) {
    "lepton_jpeg_util.exe"
} else {
    "lepton_jpeg_util"
};

pub(crate) fn find() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MONAZIP_LEPTON_PATH") {
        return PathBuf::from(path)
            .canonicalize()
            .ok()
            .filter(|file| file.is_file());
    }
    codec_tool::executable("lepton_jpeg_util").or_else(|| {
        env::current_exe().ok()?.ancestors().find_map(|root| {
            let tool = root.join(".setup/bin").join(EXECUTABLE);
            (root.join("Cargo.toml").is_file() && tool.is_file()).then_some(tool)
        })
    })
}

pub(crate) fn convert(
    tool: &Path,
    input: File,
    output: &Path,
    encoding: bool,
    cancel: &AtomicBool,
) -> Result<(), AnalysisError> {
    let mut command = Command::new(tool);
    command.args(["--threads", "2", "--quiet"]);
    if encoding {
        // The comparison reads the generated archive back against the source.
        command.arg("--noverify");
    }
    command
        .stdin(Stdio::from(input))
        .stdout(Stdio::from(File::create(output)?))
        .stderr(Stdio::null());
    codec_tool::run(command, "Lepton", cancel)
}
