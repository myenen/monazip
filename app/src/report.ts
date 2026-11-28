export type FileInfo = { name: string; path: string; size: number };
export type ExtractionReport = { compressed_bytes: number; restored_bytes: number; elapsed_ms: number };
export type Format = "zstd" | "jpeg_xl" | "lepton" | "gzip" | "zip" | "rar";
export type ZstdTuning = "standard" | "parallel" | "parallel_long";
export type Progress = {
  format: Format;
  stage: "compressing" | "verifying";
  processed_bytes: number;
  total_bytes: number;
  format_count: number;
  format_index: number;
};
export type Measurement = {
  format: Format;
  level: number;
  bytes: number;
  compress_us: number;
  verify_us: number;
};
export type Report = {
  kind: keyof typeof fileKinds;
  archive_extension: "zst" | "jxl" | "lep";
  jpeg_xl_trial: Measurement | null;
  lepton_trial: Measurement | null;
  original_bytes: number;
  monazip: Measurement;
  zstd: Measurement;
  zstd_tuning: ZstdTuning;
  gzip: Measurement;
  zip: Measurement;
  rar: Measurement | null;
  rar_status: "verified" | "unavailable" | "failed";
  savings_percent: number | null;
  gzip_gain_percent: number;
  zip_gain_percent: number;
  rar_gain_percent: number | null;
  verified: boolean;
  elapsed_ms: number;
};

const integer = new Intl.NumberFormat("tr-TR");
const decimal = new Intl.NumberFormat("tr-TR", { maximumFractionDigits: 2 });
const percentage = new Intl.NumberFormat("tr-TR", { style: "percent", maximumFractionDigits: 2 });
export const number = (value: number) => decimal.format(value);
export const bytes = (value: number) => `${integer.format(value)} B`;
export const percent = (value: number | null) => value === null ? "—" : percentage.format(value / 100);
export const formats = { zstd: "MonaZip (.zst)", jpeg_xl: "JPEG XL (.jxl)", lepton: "Lepton (.lep)", gzip: "gzip (.gz)", zip: "ZIP (.zip)", rar: "RAR (.rar)" };
export const fileKinds = { sqlite: "SQLite", text: "Metin dosyası", jpeg: "JPEG", png: "PNG", webp: "WebP", gif: "GIF", bmp: "BMP", tiff: "TIFF" };
export const tuningLabels = {
  standard: "",
  parallel: "2 iş parçacığı",
  parallel_long: "2 iş parçacığı + 128 MiB uzak eşleşme",
};
