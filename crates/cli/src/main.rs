use clap::Parser;
use monazip_core::{
    AnalysisError, Options, Phase, analyze,
    extract::extract_archive,
    focused::{
        FocusProgress, Format, RarStatus, Stage, analyze_focused, analyze_focused_for_archive,
        compress_focused,
    },
};

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "SQLite, metin ve görsel dosyalarını sıkıştırır ve arşivleri karşılaştırır."
)]
struct Args {
    /// Kaynak dosya veya açılacak arşiv
    input: PathBuf,
    /// Önceki blok tabanlı araştırma modunu kullan
    #[arg(long)]
    experimental: bool,
    /// Doğrulanmış .zst, .jxl veya .lep çıktısını yeni dosyaya kaydet
    #[arg(long, value_name = "PATH", conflicts_with = "experimental")]
    compress_to: Option<PathBuf>,
    /// Arşivi yeni bir dosyaya aç
    #[arg(long, value_name = "PATH", conflicts_with_all = ["experimental", "compress_to", "zstd_level", "output"])]
    extract_to: Option<PathBuf>,
    /// Sabit blok boyutu (bayt). Boş bırakıldığında otomatik seçilir.
    #[arg(long, requires = "experimental")]
    block_size: Option<usize>,
    /// Stride keşfini kapat ve yalnızca sabit adayları dene
    #[arg(long, requires = "experimental")]
    fixed_strides: bool,
    /// Zstd seviyesi (DB/metin varsayılanı 6, araştırma modu varsayılanı 3)
    #[arg(long)]
    zstd_level: Option<i32>,
    /// Rapora yazılacak en fazla blok detayı
    #[arg(long, requires = "experimental", default_value_t = Options::default().detail_limit)]
    detail_limit: usize,
    /// Sonucu JSON olarak stdout'a yaz
    #[arg(long)]
    json: bool,
    /// JSON raporunu yeni bir dosyaya kaydet
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
    /// İlerleme satırlarını gizle
    #[arg(long)]
    quiet: bool,
}

fn save_report(
    path: &Path,
    report: &impl serde::Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    writeln!(file)?;
    file.persist_noclobber(path)?;
    Ok(())
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    if let Some(path) = &args.extract_to {
        if !args.quiet {
            eprintln!("Arşiv açılıyor");
        }
        let report = extract_archive(&args.input, path, |_| {}, &cancel)?;
        let mut stdout = io::stdout().lock();
        if args.json {
            serde_json::to_writer_pretty(&mut stdout, &report)?;
            writeln!(stdout)?;
        } else {
            writeln!(
                stdout,
                "Açıldı · {} bayt · {}",
                report.restored_bytes,
                path.display()
            )?;
        }
        return Ok(());
    }
    if !args.experimental {
        return run_focused(&args, &cancel);
    }
    let mut last_update: Option<Instant> = None;
    let report = analyze(
        &args.input,
        Options {
            block_size: args
                .block_size
                .unwrap_or_else(|| Options::default().block_size),
            auto_block_size: args.block_size.is_none(),
            auto_stride: !args.fixed_strides,
            zstd_level: args
                .zstd_level
                .unwrap_or_else(|| Options::default().zstd_level),
            detail_limit: args.detail_limit,
        },
        |progress| {
            let now = Instant::now();
            if !args.quiet
                && (progress.processed_bytes == 0
                    || last_update
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(350)))
            {
                let phase = match progress.phase {
                    Phase::SelectingBlockSize => "Blok boyutu seçiliyor",
                    Phase::Analyzing => "Analiz",
                    Phase::Verifying => "Doğrulama",
                };
                eprintln!(
                    "{phase} · {} / {} bayt · {:.2} sn",
                    progress.processed_bytes,
                    progress.total_bytes,
                    progress.elapsed_ms as f64 / 1000.0
                );
                last_update = Some(now);
            }
        },
        &cancel,
    )?;

    if let Some(path) = args.output {
        save_report(&path, &report)?;
    }
    let mut stdout = io::stdout().lock();
    if args.json {
        serde_json::to_writer_pretty(&mut stdout, &report)?;
        writeln!(stdout)?;
    } else {
        let percentage =
            |value: Option<f64>| value.map_or_else(|| "N/A".into(), |n| format!("%{n:.2}"));
        writeln!(
            stdout,
            "Blok boyutu         {} bayt",
            report.options.block_size
        )?;
        writeln!(stdout, "Kaynak              {} bayt", report.original_bytes)?;
        writeln!(
            stdout,
            "Deneysel            {} bayt ({} bayt ek yük)",
            report.experimental_bytes, report.metadata_bytes
        )?;
        writeln!(
            stdout,
            "Zstd akış           {} bayt",
            report.zstd_stream_bytes
        )?;
        writeln!(
            stdout,
            "Zstd blok           {} bayt",
            report.zstd_block_bytes
        )?;
        writeln!(
            stdout,
            "Tasarruf            {}",
            percentage(report.savings_percent)
        )?;
        writeln!(
            stdout,
            "Zstd akışa göre      {}",
            percentage(report.zstd_stream_gain_percent)
        )?;
        writeln!(
            stdout,
            "Zstd bloklara göre   {}",
            percentage(report.zstd_block_gain_percent)
        )?;
        writeln!(
            stdout,
            "Baskın yöntem       {}",
            report.dominant_method.as_deref().unwrap_or("N/A")
        )?;
        writeln!(
            stdout,
            "Doğrulama           Birebir eşleşti · {} blok · {:.2} sn",
            report.block_count,
            report.elapsed_ms as f64 / 1000.0
        )?;
        for warning in report.warnings {
            writeln!(stdout, "{warning}")?;
        }
    }
    Ok(())
}

fn run_focused(args: &Args, cancel: &AtomicBool) -> Result<(), Box<dyn std::error::Error>> {
    let level = args.zstd_level.unwrap_or(6);
    let notify = |progress: FocusProgress| {
        if !args.quiet && progress.processed_bytes == 0 {
            let stage = match progress.stage {
                Stage::Compressing => "Sıkıştırılıyor",
                Stage::Verifying => "Doğrulanıyor",
            };
            eprintln!("{stage} · {}", progress.format.name());
        }
    };
    let requested_format = args.compress_to.as_ref().map(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(Format::from_image_extension)
            .unwrap_or(Format::Zstd)
    });
    if let Some(path) = &args.compress_to
        && requested_format == Some(Format::Zstd)
        && !args.json
        && args.output.is_none()
    {
        let output = compress_focused(&args.input, level, notify, cancel)?;
        output.save_archive(path)?;
        writeln!(
            io::stdout().lock(),
            "Doğrulandı · {} bayt · {}",
            output.measurement.bytes,
            path.display()
        )?;
        return Ok(());
    }
    let result = if let Some(format) = requested_format {
        analyze_focused_for_archive(&args.input, level, format, notify, cancel)?
    } else {
        analyze_focused(&args.input, level, notify, cancel)?
    };
    if let Some(path) = &args.compress_to {
        result.save_archive(path)?;
    }
    let report = &result.report;
    if let Some(path) = &args.output {
        save_report(path, report)?;
    }
    let mut stdout = io::stdout().lock();
    if args.json {
        serde_json::to_writer_pretty(&mut stdout, report)?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "Kaynak · {} bayt", report.original_bytes)?;
        writeln!(stdout, "Seçilen arşiv · .{}", report.archive_extension)?;
        for row in std::iter::once(&report.monazip)
            .chain(
                [
                    Some(&report.zstd),
                    report.jpeg_xl_trial.as_ref(),
                    report.lepton_trial.as_ref(),
                ]
                .into_iter()
                .flatten()
                .filter(|row| row.format != report.monazip.format),
            )
            .chain([&report.gzip, &report.zip])
            .chain(&report.rar)
        {
            writeln!(
                stdout,
                "{} · {} bayt · sıkıştırma {:.3} ms · açma ve doğrulama {:.3} ms",
                row.format.name(),
                row.bytes,
                row.compress_us as f64 / 1_000.0,
                row.verify_us as f64 / 1_000.0,
            )?;
        }
        writeln!(
            stdout,
            "gzip’e göre %{:.2} · ZIP’e göre %{:.2}",
            report.gzip_gain_percent, report.zip_gain_percent
        )?;
        match (report.rar_gain_percent, report.rar_status) {
            (Some(gain), _) => writeln!(stdout, "RAR’a göre %{gain:.2}")?,
            (None, RarStatus::Unavailable) => writeln!(stdout, "RAR aracı bulunamadı.")?,
            (None, _) => writeln!(stdout, "RAR karşılaştırması tamamlanamadı.")?,
        }
        writeln!(stdout, "Ölçülen çıktılar birebir doğrulandı.")?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            if matches!(error.downcast_ref(), Some(AnalysisError::Cancelled)) {
                ExitCode::from(130)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
