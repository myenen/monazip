import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import { bytes, fileKinds, formats, number, percent, tuningLabels, type ExtractionReport, type FileInfo, type Progress, type Report } from "./report";
import "./style.css";

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="metric"><span>{label}</span><strong>{value}</strong></div>;
}

function Comparison({ report }: { report: Report }) {
  const measurements = [report.monazip];
  for (const candidate of [report.zstd, report.jpeg_xl_trial, report.lepton_trial]) {
    if (candidate && candidate.format !== report.monazip.format) measurements.push(candidate);
  }
  measurements.push(report.gzip, report.zip);
  if (report.rar) measurements.push(report.rar);
  const rarLabel = report.rar_status === "unavailable" ? "RAR aracı bulunamadı" : "RAR ölçümü tamamlanamadı";
  const series = [
    { label: "Kaynak dosya", size: report.original_bytes, color: "source" },
    ...measurements.map((item) => ({ label: formats[item.format], size: item.bytes, color: item.format })),
  ];
  const max = Math.max(1, ...series.map(({ size }) => size));
  const difference = (value: number) => value === 0
    ? "Aynı boyut"
    : `${percent(Math.abs(value))} daha ${value > 0 ? "küçük" : "büyük"}`;
  return <>
    <div className="metrics">
      <Metric label={`MonaZip çıktısı (.${report.archive_extension})`} value={bytes(report.monazip.bytes)} />
      <Metric label="gzip’e göre" value={difference(report.gzip_gain_percent)} />
      <Metric label="ZIP’e göre" value={difference(report.zip_gain_percent)} />
      <Metric label="RAR’a göre" value={report.rar_gain_percent === null ? rarLabel : difference(report.rar_gain_percent)} />
    </div>
    <section className="panel chart" aria-label="Dosya boyutu karşılaştırması">
      <div className="panel-heading">
        <h2>Dosya boyutları</h2>
        <span>{fileKinds[report.kind]}</span>
      </div>
      {series.map(({ label, size, color }) => <div className="bar-row" key={color}>
        <span>{label}</span><div className="track"><div className={color} style={{ width: `${size / max * 100}%` }} /></div>
        <strong>{bytes(size)}</strong>
      </div>)}
    </section>
    <section className="panel timings">
      <div className="panel-heading"><h2>İşlem süreleri</h2><span>Toplam {number(report.elapsed_ms / 1000)} sn</span></div>
      <div className="table-wrap"><table>
        <thead><tr><th>Biçim</th><th>Seviye</th><th>Sıkıştırma</th><th>Açma ve doğrulama</th></tr></thead>
        <tbody>{measurements.map((item) => <tr key={item.format}>
          <td>{formats[item.format]}{item.format === "zstd" && report.zstd_tuning !== "standard" &&
            <small className="tuning-label">{tuningLabels[report.zstd_tuning]}</small>}</td>
          <td>{item.format === "jpeg_xl" ? `Efor ${item.level}` : item.format === "lepton" ? "—" : item.level}</td>
          <td>{number(item.compress_us / 1000)} ms</td><td>{number(item.verify_us / 1000)} ms</td>
        </tr>)}</tbody>
      </table></div>
    </section>
  </>;
}

function App() {
  const [file, setFile] = useState<FileInfo>();
  const [archiveMode, setArchiveMode] = useState(false);
  const [progress, setProgress] = useState<Progress>();
  const [operation, setOperation] = useState<"analysis" | "extract">();
  const [restored, setRestored] = useState<number>();
  const running = operation !== undefined;
  const [canceling, setCanceling] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [report, setReport] = useState<Report>();
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [tab, setTab] = useState<"comparison" | "json">("comparison");
  const [elapsed, setElapsed] = useState(0);
  const busy = useRef(false);
  const selection = useRef(0);

  async function select(path: string) {
    if (busy.current) return;
    const ticket = ++selection.current;
    setError("");
    setNotice("");
    try {
      const info = await invoke<FileInfo>("inspect_file", { path });
      if (ticket !== selection.current || busy.current) return;
      setFile(info);
      setReport(undefined);
    } catch (reason) {
      if (ticket === selection.current) setError(String(reason));
    }
  }

  useEffect(() => {
    const listener = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "drop") {
        setDragging(false);
        if (busy.current) return;
        if (payload.paths.length === 1) void select(payload.paths[0]);
        else setError("Bir seferde tek dosya seçin.");
      } else {
        setDragging(!busy.current && payload.type !== "leave");
      }
    });
    void listener.catch((reason) => setError(String(reason)));
    return () => { void listener.then((dispose) => dispose(), () => {}); };
  }, []);

  useEffect(() => {
    if (!running) return;
    const started = performance.now();
    const timer = window.setInterval(() => setElapsed((performance.now() - started) / 1000), 100);
    return () => window.clearInterval(timer);
  }, [running]);

  async function chooseFile() {
    try {
      const path = await open({ multiple: false, directory: false, title: "SQLite, metin veya görsel dosyası seçin" });
      if (path) await select(path);
    } catch (reason) { setError(String(reason)); }
  }

  async function start() {
    if (!file || busy.current) return;
    busy.current = true;
    setOperation("analysis");
    setReport(undefined);
    setProgress(undefined);
    setElapsed(0);
    setError("");
    setNotice("");
    setTab("comparison");
    const channel = new Channel<Progress>();
    channel.onmessage = (update) => { if (busy.current) setProgress(update); };
    try {
      const result = await invoke<Report>("start_analysis", {
        path: file.path,
        level: archiveMode ? 19 : 9,
        tuning: archiveMode ? "standard" : "parallel_long",
        progress: channel,
      });
      setReport(result);
      setFile({ ...file, size: result.original_bytes });
    } catch (reason) {
      const message = String(reason);
      if (message === "Analiz iptal edildi.") setNotice(message);
      else setError(message);
    } finally {
      busy.current = false;
      setOperation(undefined);
      setCanceling(false);
    }
  }

  async function cancel() {
    setCanceling(true);
    try { await invoke("cancel_operation"); }
    catch (reason) { setError(String(reason)); setCanceling(false); }
  }

  async function openArchive() {
    if (busy.current) return;
    busy.current = true;
    setError("");
    setNotice("");
    try {
      const path = await open({
        multiple: false, directory: false, title: "Açılacak arşivi seçin",
        filters: [{ name: "MonaZip arşivleri", extensions: ["zst", "zstd", "jxl", "lep"] }],
      });
      if (!path) return;
      const isJpegArchive = /\.(jxl|lep)$/i.test(path);
      let suggested = path.replace(/\.(zst|zstd|jxl|lep)$/i, "");
      if (suggested === path || /[\\/]$/.test(suggested)) suggested = "acilan-dosya";
      if (isJpegArchive && !/\.jpe?g$/i.test(suggested)) suggested += ".jpg";
      const destination = await save({
        title: "Açılan dosyanın konumunu seçin",
        defaultPath: suggested,
      });
      if (!destination) return;

      setElapsed(0);
      setRestored(undefined);
      setOperation("extract");
      const channel = new Channel<number>();
      channel.onmessage = (update) => { if (busy.current) setRestored(update); };
      const result = await invoke<ExtractionReport>("extract_archive", { path, destination, progress: channel });
      setNotice(`Dosya açıldı · ${bytes(result.restored_bytes)} · ${destination}`);
    } catch (reason) {
      const message = String(reason);
      if (message === "Analiz iptal edildi.") setNotice("Arşiv açma iptal edildi.");
      else setError(message);
    } finally {
      busy.current = false;
      setOperation(undefined);
      setCanceling(false);
    }
  }

  async function exportJson() {
    setError("");
    setNotice("");
    try {
      const path = await save({ defaultPath: `${file?.name ?? "monazip"}.report.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (!path) return;
      await invoke("save_report", { path });
      setNotice("JSON raporu kaydedildi.");
    } catch (reason) { setError(String(reason)); }
  }

  async function exportArchive() {
    if (!report || busy.current) return;
    busy.current = true;
    setError("");
    setNotice("");
    try {
      const extension = report.archive_extension;
      const path = await save({
        defaultPath: `${file?.name ?? "monazip"}.${extension}`,
        filters: [{ name: { jxl: "JPEG XL", lep: "Lepton", zst: "Zstandard" }[extension], extensions: [extension] }],
      });
      if (!path) return;
      await invoke("save_archive", { path });
      setNotice(`Doğrulanmış .${extension} kaydedildi · ${bytes(report.monazip.bytes)} · ${path}`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      busy.current = false;
    }
  }

  const phaseFraction = progress && progress.total_bytes > 0 ? progress.processed_bytes / progress.total_bytes : 0;
  const formatIndex = progress?.format_index ?? 0;
  const stageIndex = progress?.stage === "verifying" ? 1 : 0;
  const progressPercent = (formatIndex * 2 + stageIndex + phaseFraction) / ((progress?.format_count ?? 3) * 2) * 100;
  const phaseLabel = progress ? `${formats[progress.format]} · ${stageIndex ? "Doğrulanıyor" : "Sıkıştırılıyor"}` : "Dosya okunuyor";
  return <div className="shell">
    <header className="topbar">
      <div className="brand"><span className="brand-icon" aria-hidden="true"><i /><i /><i /><i /></span><span>MonaZip</span></div>
      <button className="secondary" disabled={running} onClick={openArchive}>Arşiv aç</button>
    </header>
    <main>
      <section className="hero">
        <h1>Dosya Sıkıştırma</h1>
        <p>SQLite, metin ve görselleri kayıpsız sıkıştır. Boyut ve süreyi gzip, ZIP ve RAR ile karşılaştır.</p>
      </section>
      <section className={`input-panel ${dragging ? "dropping" : ""}`} aria-label="Dosya ve analiz ayarları">
        <div className="file-area">
          <span className="file-icon" aria-hidden="true"><i /></span>
          <div className="file-label">
            <strong title={file?.path}>{file?.name ?? "SQLite, metin veya görsel dosyası bırak"}</strong>
            <span>{file ? bytes(file.size) : "JPEG, PNG, GIF, WebP, BMP, TIFF ve yaygın metin biçimleri."}</span>
          </div>
          <button className="secondary" disabled={running} onClick={chooseFile}>{file ? "Değiştir" : "Dosya seç"}</button>
        </div>
        <div className="settings">
          <label className="mode">Sıkıştırma modu
            <select disabled={running} value={archiveMode ? "archive" : "default"} onChange={(event) => setArchiveMode(event.target.value === "archive")}>
              <option value="default">Hızlı · Zstd 9</option>
              <option value="archive">Arşiv · Zstd 19</option>
            </select>
          </label>
          {running ? <button className="secondary" disabled={(operation === "extract" ? restored === undefined : !progress) || canceling} onClick={cancel}>{canceling ? "İptal ediliyor…" : "İptal et"}</button> :
            <button className="primary" disabled={!file} onClick={start}>Sıkıştır ve karşılaştır <span aria-hidden="true">↗</span></button>}
        </div>
      </section>
      {error && <p className="message error" role="alert">{error}</p>}
      {notice && <p className="message" role="status">{notice}</p>}
      {running && <section className="panel progress-panel" aria-live="polite">
        <div className="panel-heading"><h2>{operation === "extract" ? "Arşiv açılıyor" : phaseLabel}</h2><span>{number(elapsed)} sn</span></div>
        {operation === "extract" ? <p>{bytes(restored ?? 0)} geri açıldı</p> : <>
          <progress aria-label="Analiz ilerlemesi" max={100} value={progressPercent} />
          <p>{bytes(progress?.processed_bytes ?? 0)} / {bytes(progress?.total_bytes ?? file?.size ?? 0)}</p>
        </>}
      </section>}
      {!running && !report && <section className="panel idle">
        <div className="idle-chart" aria-hidden="true"><i /><i /><i /></div>
        <h2>Analiz sonuçları</h2>
        <p>{file ? "Boyutları ve işlem sürelerini karşılaştırmak için sıkıştır." : "Sonuçları görmek için bir dosya seç."}</p>
      </section>}
      {report && <section className="results" aria-label="Analiz sonuçları">
        <div className="result-toolbar">
          <div className="tabs" aria-label="Rapor görünümü">
            <button aria-pressed={tab === "comparison"} onClick={() => setTab("comparison")}>Karşılaştırma</button>
            <button aria-pressed={tab === "json"} onClick={() => setTab("json")}>JSON</button>
          </div>
          {report.verified && <span className="verified" role="status">✓ Birebir doğrulandı</span>}
          <button className="secondary" disabled={running} onClick={exportJson}>JSON kaydet</button>
          <button className="primary" disabled={running} onClick={exportArchive}>.{report.archive_extension} kaydet</button>
        </div>
        {tab === "comparison" && <Comparison report={report} />}
        {tab === "json" && <pre className="panel json">{JSON.stringify(report, null, 2)}</pre>}
      </section>}
    </main>
  </div>;
}

createRoot(document.getElementById("root")!).render(<App />);
