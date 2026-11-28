use crate::{AnalysisError, block, check_cancel, jpegxl, rar, statistics, unchanged};
use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Mutex, atomic::AtomicBool},
    time::{Duration, Instant},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "text", "md", "rst", "sql", "csv", "tsv", "json", "jsonl", "ndjson", "xml", "yaml",
    "yml", "toml", "ini", "log", "html", "htm",
];
const DEFLATE_LEVEL: u32 = 6;
const BUFFER_SIZE: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZstdTuning {
    #[default]
    Standard,
    Parallel,
    ParallelLong,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Sqlite,
    Text,
    Jpeg,
    Png,
    Webp,
    Gif,
    Bmp,
    Tiff,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Zstd,
    JpegXl,
    Gzip,
    Zip,
    Rar,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Self::Zstd => "MonaZip (.zst)",
            Self::JpegXl => "MonaZip (JPEG XL)",
            Self::Gzip => "gzip (.gz)",
            Self::Zip => "ZIP (.zip)",
            Self::Rar => "RAR (.rar)",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Compressing,
    Verifying,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct FocusProgress {
    pub format: Format,
    pub stage: Stage,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub format_count: u8,
    pub format_index: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormatMeasurement {
    pub format: Format,
    pub level: i32,
    pub bytes: u64,
    pub compress_us: u64,
    pub verify_us: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RarStatus {
    Verified,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusReport {
    pub kind: FileKind,
    pub original_bytes: u64,
    pub monazip: FormatMeasurement,
    pub archive_extension: &'static str,
    pub jpeg_xl_trial: Option<FormatMeasurement>,
    pub zstd_tuning: ZstdTuning,
    pub gzip: FormatMeasurement,
    pub zip: FormatMeasurement,
    pub rar: Option<FormatMeasurement>,
    pub rar_status: RarStatus,
    pub savings_percent: Option<f64>,
    pub gzip_gain_percent: f64,
    pub zip_gain_percent: f64,
    pub rar_gain_percent: Option<f64>,
    pub verified: bool,
    pub elapsed_ms: u64,
}

pub struct FocusResult {
    pub report: FocusReport,
    archive: Mutex<File>,
}

impl FocusResult {
    pub fn save_archive(&self, path: &Path) -> io::Result<()> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches!(self.report.monazip.format, Format::JpegXl)
            != extension.eq_ignore_ascii_case("jxl")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Arşiv için .{} uzantılı bir hedef seçin.",
                    self.report.archive_extension,
                ),
            ));
        }
        save_archive(&self.archive, path, self.report.monazip.bytes)
    }
}

pub struct FocusArchive {
    pub measurement: FormatMeasurement,
    archive: Mutex<File>,
}

impl FocusArchive {
    pub fn save_archive(&self, path: &Path) -> io::Result<()> {
        save_archive(&self.archive, path, self.measurement.bytes)
    }
}

fn save_archive(archive: &Mutex<File>, path: &Path, expected_bytes: u64) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    let mut input = archive
        .lock()
        .map_err(|_| io::Error::other("Arşiv okunamadı."))?;
    input.rewind()?;
    let copied = io::copy(&mut *input, &mut output)?;
    if copied != expected_bytes || output.as_file().metadata()?.len() != expected_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Kaydedilen arşivin boyutu doğrulanmış çıktıyla eşleşmedi.",
        ));
    }
    output.persist_noclobber(path)?;
    Ok(())
}

fn identify(path: &Path, source: &mut (impl Read + Seek)) -> Result<FileKind, AnalysisError> {
    let mut header = [0; SQLITE_HEADER.len()];
    let count = block::fill(source, &mut header)?;
    source.rewind()?;
    if count == header.len() && &header == SQLITE_HEADER {
        return Ok(FileKind::Sqlite);
    }
    let header = &header[..count];
    if header.starts_with(b"\xff\xd8\xff") {
        return Ok(FileKind::Jpeg);
    }
    if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(FileKind::Png);
    }
    if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        return Ok(FileKind::Gif);
    }
    if header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP") {
        return Ok(FileKind::Webp);
    }
    if count >= 14 && header.starts_with(b"BM") && header[6..10] == [0; 4] {
        return Ok(FileKind::Bmp);
    }
    if [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"]
        .iter()
        .any(|signature| header.starts_with(*signature))
    {
        return Ok(FileKind::Tiff);
    }
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str())
        && TEXT_EXTENSIONS
            .iter()
            .any(|text| text.eq_ignore_ascii_case(extension))
    {
        return Ok(FileKind::Text);
    }
    Err(AnalysisError::InvalidOptions(
        "SQLite, metin veya JPEG, PNG, GIF, WebP, BMP, TIFF görseli seçin.",
    ))
}

fn micros(elapsed: Duration) -> u64 {
    elapsed.as_micros() as u64
}

fn zip_error(error: zip::result::ZipError) -> io::Error {
    io::Error::other(error)
}

struct Run<'a, F> {
    source: BufReader<File>,
    cancel: &'a AtomicBool,
    progress: F,
    total: u64,
    format_count: u8,
    jpeg_xl_trial: bool,
    zstd_tuning: ZstdTuning,
    input: Vec<u8>,
    decoded: Vec<u8>,
}

impl<F: FnMut(FocusProgress)> Run<'_, F> {
    fn notify(&mut self, format: Format, stage: Stage, processed_bytes: u64) {
        let format_index = match format {
            Format::Zstd => 0,
            Format::JpegXl => 1,
            Format::Gzip => 1 + u8::from(self.jpeg_xl_trial),
            Format::Zip => 2 + u8::from(self.jpeg_xl_trial),
            Format::Rar => 3 + u8::from(self.jpeg_xl_trial),
        };
        (self.progress)(FocusProgress {
            format,
            stage,
            processed_bytes,
            total_bytes: self.total,
            format_count: self.format_count,
            format_index,
        });
    }

    fn transfer(&mut self, output: &mut impl Write, format: Format) -> Result<(), AnalysisError> {
        self.source.rewind()?;
        self.notify(format, Stage::Compressing, 0);
        let mut processed = 0;
        loop {
            check_cancel(self.cancel)?;
            let count = block::fill(&mut self.source, &mut self.input)?;
            if count == 0 {
                break;
            }
            processed += count as u64;
            if processed > self.total {
                return Err(AnalysisError::SourceChanged);
            }
            output.write_all(&self.input[..count])?;
            self.notify(format, Stage::Compressing, processed);
        }
        if processed != self.total {
            return Err(AnalysisError::SourceChanged);
        }
        Ok(())
    }

    fn verify(&mut self, mut decoded: impl Read, format: Format) -> Result<(), AnalysisError> {
        self.source.rewind()?;
        self.notify(format, Stage::Verifying, 0);
        let mut processed = 0;
        while processed < self.total {
            check_cancel(self.cancel)?;
            let count = (self.total - processed).min(self.input.len() as u64) as usize;
            if block::fill(&mut self.source, &mut self.input[..count])? != count {
                return Err(AnalysisError::SourceChanged);
            }
            if block::fill(&mut decoded, &mut self.decoded[..count])? != count
                || self.input[..count] != self.decoded[..count]
            {
                return Err(AnalysisError::Verification);
            }
            processed += count as u64;
            self.notify(format, Stage::Verifying, processed);
        }
        if block::fill(&mut self.source, &mut self.input[..1])? != 0 {
            return Err(AnalysisError::SourceChanged);
        }
        if block::fill(&mut decoded, &mut self.decoded[..1])? != 0 {
            return Err(AnalysisError::Verification);
        }
        check_cancel(self.cancel)
    }

    fn measure(
        &mut self,
        format: Format,
        level: i32,
        name: &str,
    ) -> Result<(FormatMeasurement, File), AnalysisError> {
        let mut archive = tempfile::tempfile()?;
        let compression = Instant::now();
        let mut output = BufWriter::with_capacity(BUFFER_SIZE, &mut archive);
        match format {
            Format::Zstd => {
                let mut encoder = zstd::stream::write::Encoder::new(&mut output, level)?;
                if self.zstd_tuning != ZstdTuning::Standard {
                    encoder.multithread(2)?;
                }
                if self.zstd_tuning == ZstdTuning::ParallelLong {
                    encoder.long_distance_matching(true)?;
                    encoder.window_log(27)?;
                }
                encoder.include_checksum(true)?;
                encoder.set_pledged_src_size(Some(self.total))?;
                self.transfer(&mut encoder, format)?;
                encoder.finish()?;
            }
            Format::Gzip => {
                let mut encoder = GzEncoder::new(&mut output, Compression::new(level as u32));
                self.transfer(&mut encoder, format)?;
                encoder.finish()?;
            }
            Format::Zip => {
                let mut encoder = ZipWriter::new(&mut output);
                let options = SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Deflated)
                    .compression_level(Some(level.into()))
                    .large_file(self.total >= 0xffe0_0000);
                encoder.start_file(name, options).map_err(zip_error)?;
                self.transfer(&mut encoder, format)?;
                encoder.finish().map_err(zip_error)?;
            }
            Format::Rar | Format::JpegXl => unreachable!("Uses an external encoder"),
        }
        output.flush()?;
        drop(output);
        let compress_us = micros(compression.elapsed());

        let bytes = archive.metadata()?.len();
        archive.seek(SeekFrom::Start(0))?;
        let verification = Instant::now();
        let input = BufReader::with_capacity(BUFFER_SIZE, &mut archive);
        match format {
            Format::Zstd => {
                self.verify(zstd::stream::read::Decoder::with_buffer(input)?, format)?
            }
            Format::Gzip => self.verify(MultiGzDecoder::new(input), format)?,
            Format::Zip => {
                let mut zip = ZipArchive::new(input).map_err(zip_error)?;
                self.verify(zip.by_index(0).map_err(zip_error)?, format)?;
            }
            Format::Rar | Format::JpegXl => unreachable!("Uses an external decoder"),
        }
        let verify_us = micros(verification.elapsed());
        Ok((
            FormatMeasurement {
                format,
                level,
                bytes,
                compress_us,
                verify_us,
            },
            archive,
        ))
    }

    fn measure_jpeg_xl(
        &mut self,
        tool: &jpegxl::JpegXl,
        effort: i32,
    ) -> Result<(FormatMeasurement, File), AnalysisError> {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.jpg");
        let archive = directory.path().join("archive.jxl");
        let restored = directory.path().join("restored.jpg");
        let format = Format::JpegXl;
        let compression = Instant::now();
        let mut input = BufWriter::new(File::create(&source)?);
        self.transfer(&mut input, format)?;
        input.flush()?;
        drop(input);
        tool.compress(&source, &archive, effort, self.cancel)?;
        let compress_us = micros(compression.elapsed());
        let encoded = File::open(&archive)?;
        let bytes = encoded.metadata()?.len();
        let verification = Instant::now();
        self.notify(format, Stage::Verifying, 0);
        jpegxl::reconstruct(&tool.decoder, encoded.try_clone()?, &restored, self.cancel)?;
        self.verify(BufReader::new(File::open(restored)?), format)?;
        let verify_us = micros(verification.elapsed());
        // Copy to an anonymous file so the temporary directory also works on Windows.
        let mut saved = tempfile::tempfile()?;
        io::copy(&mut BufReader::new(File::open(archive)?), &mut saved)?;
        Ok((
            FormatMeasurement {
                format,
                level: effort,
                bytes,
                compress_us,
                verify_us,
            },
            saved,
        ))
    }

    fn measure_rar(&mut self, tool: &Path, name: &str) -> Result<FormatMeasurement, AnalysisError> {
        let directory = tempfile::tempdir()?;
        let archive = directory.path().join("comparison.rar");
        let format = Format::Rar;
        self.source.rewind()?;
        self.notify(format, Stage::Compressing, 0);
        check_cancel(self.cancel)?;
        let compression = Instant::now();
        rar::compress(
            tool,
            self.source.get_ref().try_clone()?,
            &archive,
            name,
            self.cancel,
        )?;
        let compress_us = micros(compression.elapsed());
        self.notify(format, Stage::Compressing, self.total);
        let bytes = archive.metadata()?.len();

        let verification = Instant::now();
        let (decoder, output) = rar::decompress(tool, &archive)?;
        self.verify(BufReader::new(output), format)?;
        decoder.finish(self.cancel)?;
        let verify_us = micros(verification.elapsed());
        Ok(FormatMeasurement {
            format,
            level: rar::LEVEL,
            bytes,
            compress_us,
            verify_us,
        })
    }
}

fn with_source<F: FnMut(FocusProgress), R>(
    path: &Path,
    level: i32,
    progress: F,
    cancel: &AtomicBool,
    job: impl FnOnce(&mut Run<'_, F>, FileKind, &str) -> Result<R, AnalysisError>,
) -> Result<R, AnalysisError> {
    if !(1..=22).contains(&level) {
        return Err(AnalysisError::InvalidOptions(
            "Zstd seviyesi 1–22 arasında olmalıdır.",
        ));
    }
    check_cancel(cancel)?;
    let before = path.metadata()?;
    if !before.is_file() {
        return Err(AnalysisError::NotAFile);
    }
    let source = File::open(path)?;
    if !unchanged(&before, &source.metadata()?) {
        return Err(AnalysisError::SourceChanged);
    }
    let mut run = Run {
        source: BufReader::with_capacity(BUFFER_SIZE, source),
        total: before.len(),
        format_count: 1,
        jpeg_xl_trial: false,
        zstd_tuning: ZstdTuning::Standard,
        cancel,
        progress,
        input: vec![0; BUFFER_SIZE],
        decoded: vec![0; BUFFER_SIZE],
    };
    let kind = identify(path, &mut run.source)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source");
    let result = job(&mut run, kind, name);
    if !unchanged(&before, &run.source.get_ref().metadata()?)
        || !path
            .metadata()
            .is_ok_and(|after| unchanged(&before, &after))
    {
        return Err(AnalysisError::SourceChanged);
    }
    result
}

pub fn compress_focused(
    path: impl AsRef<Path>,
    level: i32,
    progress: impl FnMut(FocusProgress),
    cancel: &AtomicBool,
) -> Result<FocusArchive, AnalysisError> {
    with_source(path.as_ref(), level, progress, cancel, |run, _, name| {
        let (measurement, archive) = run.measure(Format::Zstd, level, name)?;
        Ok(FocusArchive {
            measurement,
            archive: Mutex::new(archive),
        })
    })
}

pub fn analyze_focused(
    path: impl AsRef<Path>,
    level: i32,
    progress: impl FnMut(FocusProgress),
    cancel: &AtomicBool,
) -> Result<FocusResult, AnalysisError> {
    analyze_focused_with_tuning(path, level, ZstdTuning::Standard, progress, cancel)
}

pub fn analyze_focused_zstd(
    path: impl AsRef<Path>,
    level: i32,
    progress: impl FnMut(FocusProgress),
    cancel: &AtomicBool,
) -> Result<FocusResult, AnalysisError> {
    analyze_with_image_routing(path, level, ZstdTuning::Standard, progress, cancel, false)
}

pub fn analyze_focused_with_tuning(
    path: impl AsRef<Path>,
    level: i32,
    tuning: ZstdTuning,
    progress: impl FnMut(FocusProgress),
    cancel: &AtomicBool,
) -> Result<FocusResult, AnalysisError> {
    analyze_with_image_routing(path, level, tuning, progress, cancel, true)
}

fn analyze_with_image_routing(
    path: impl AsRef<Path>,
    level: i32,
    tuning: ZstdTuning,
    progress: impl FnMut(FocusProgress),
    cancel: &AtomicBool,
    allow_jpeg_xl: bool,
) -> Result<FocusResult, AnalysisError> {
    if level != 9 && tuning != ZstdTuning::Standard {
        return Err(AnalysisError::InvalidOptions(
            "Paralel ve uzak eşleşme seçenekleri Zstd 9 için kullanılabilir.",
        ));
    }
    let started = Instant::now();
    let rar_tool = rar::find();
    with_source(path.as_ref(), level, progress, cancel, |run, kind, name| {
        let jpeg_xl = (allow_jpeg_xl && matches!(kind, FileKind::Jpeg))
            .then(jpegxl::JpegXl::find)
            .flatten();
        run.jpeg_xl_trial = jpeg_xl.is_some();
        run.format_count = 3 + u8::from(rar_tool.is_some()) + u8::from(run.jpeg_xl_trial);
        let (zstd_level, applied_tuning) = if matches!(
            kind,
            FileKind::Jpeg | FileKind::Png | FileKind::Webp | FileKind::Gif
        ) && tuning == ZstdTuning::ParallelLong
        {
            (3, ZstdTuning::Standard)
        } else {
            (level, tuning)
        };
        run.zstd_tuning = applied_tuning;
        let (mut monazip, mut archive) = run.measure(Format::Zstd, zstd_level, name)?;
        let mut jpeg_xl_trial = None;
        if let Some(tool) = jpeg_xl {
            let effort = if level >= 19 { 7 } else { 3 };
            match run.measure_jpeg_xl(&tool, effort) {
                Ok((measurement, encoded)) => {
                    jpeg_xl_trial = Some(measurement.clone());
                    if measurement.bytes < monazip.bytes {
                        monazip = measurement;
                        archive = encoded;
                    }
                }
                Err(error @ (AnalysisError::Cancelled | AnalysisError::SourceChanged)) => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        let archive_extension = if matches!(monazip.format, Format::JpegXl) {
            "jxl"
        } else {
            "zst"
        };
        let (gzip, _) = run.measure(Format::Gzip, DEFLATE_LEVEL as i32, name)?;
        let (zip, _) = run.measure(Format::Zip, DEFLATE_LEVEL as i32, name)?;
        let (rar, rar_status) = match rar_tool {
            Some(tool) => match run.measure_rar(&tool, name) {
                Ok(measurement) => (Some(measurement), RarStatus::Verified),
                Err(error @ (AnalysisError::Cancelled | AnalysisError::SourceChanged)) => {
                    return Err(error);
                }
                Err(_) => (None, RarStatus::Failed),
            },
            None => (None, RarStatus::Unavailable),
        };
        check_cancel(run.cancel)?;
        let total = run.total;
        Ok(FocusResult {
            report: FocusReport {
                kind,
                zstd_tuning: applied_tuning,
                archive_extension,
                jpeg_xl_trial,
                original_bytes: total,
                savings_percent: statistics::savings(monazip.bytes, total),
                gzip_gain_percent: statistics::savings(monazip.bytes, gzip.bytes)
                    .unwrap_or_default(),
                zip_gain_percent: statistics::savings(monazip.bytes, zip.bytes).unwrap_or_default(),
                rar_gain_percent: rar
                    .as_ref()
                    .and_then(|item| statistics::savings(monazip.bytes, item.bytes)),
                monazip,
                gzip,
                zip,
                rar,
                rar_status,
                verified: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
            archive: Mutex::new(archive),
        })
    })
}
