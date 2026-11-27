# MonaZip

MonaZip, SQLite veritabanları ve metin dosyaları için yerel, kayıpsız bir sıkıştırma aracıdır. Bütün dosyayı standart bir Zstandard akışı olarak sıkıştırır. Karşılaştırma sırasında aynı kaynaktan gzip ve tek girdili ZIP arşivleri oluşturur. RAR aracı kullanılabiliyorsa RAR5 arşivi de oluşturur. Ölçülen tüm çıktılar açılır ve kaynakla bayt bayt karşılaştırılır.

## macOS uygulaması

Apple Silicon paketi Finder’da `target/release/bundle/macos/MonaZip.app` konumundadır. Paket yerel çalıştırma için ad hoc imzalıdır.

```sh
open target/release/bundle/macos/MonaZip.app
```

SQLite veya metin dosyasını seçin ya da sürükleyip bırakın. **Sıkıştır ve karşılaştır** düğmesi ölçülen biçimlerin boyutlarını ve sürelerini gösterir. **.zst kaydet** doğrulanmış arşivi, **JSON kaydet** ölçümleri dışa aktarır. Hızlı, Dengeli ve Yoğun modlar sırasıyla Zstd 3, 6 ve 9 kullanır. Arşiv modunda Zstd 15 ve 19 seçenekleri de vardır.

**.zst aç** düğmesiyle bir Zstandard arşivi seçin ve dönecek dosyanın konumunu belirtin. MonaZip, `.zst` veya `.zstd` uzantısını kaldırarak bir ad önerir. Açılan veriyi yeni bir dosyaya yazar.

SQLite dosyaları dosya başlığından tanınır. Metin için TXT, MD, RST, SQL, CSV, TSV, JSON, JSONL, NDJSON, XML, YAML, TOML, INI, LOG ve HTML uzantıları desteklenir. SQLite verisi seçilen dosyanın baytları olarak işlenir. Tutarlı bir canlı veritabanı kopyası gerektiğinde SQLite’ın ürettiği yedek dosyayı seçin.

Zstd 9 için iki iş parçacığı ve iki iş parçacığına ek 128 MiB uzak eşleşme seçenekleri de vardır. Sonuç ekranı ve JSON raporu kullanılan ayarı gösterir. Uzak eşleşme daha fazla bellek kullanabilir. Bu seçenekler standart `.zst` çıktısı üretir.

## CLI

```sh
# Boyutları ve süreleri karşılaştır
./target/release/monazip veritabani.sqlite
./target/release/monazip olaylar.jsonl --json --quiet

# Doğrulanmış .zst dosyasını doğrudan kaydet
./target/release/monazip veritabani.sqlite --compress-to veritabani.sqlite.zst

# MonaZip ile aç
./target/release/monazip veritabani.sqlite.zst --extract-to geri-yuklenen.sqlite
```

`--compress-to` tek başına sıkıştırma ve Zstd doğrulaması yapar. Aynı komutta `--json` veya `--output rapor.json` da verilirse gzip, ZIP ve varsa RAR karşılaştırması çalışır. Yeni çıktı yolları kullanılır. `Ctrl+C` sıkıştırmayı veya doğrulamayı iptal eder ve süreç 130 koduyla çıkar.

## Kaynaktan derleme

Bu paket Rust 1.100, Node.js 25 ve Xcode ile derlendi. Windows ve Linux için Tauri’nin platforma özgü bağımlılıkları gerekir. [Tauri önkoşulları](https://v2.tauri.app/start/prerequisites/)

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --locked
cargo build --release --locked -p monazip
npm ci --prefix app
npm run build --prefix app
cargo test --release --locked -p monazip-desktop
npm run tauri --prefix app -- build --bundles app -- --locked
```

Bu çalışma alanındaki bağımlılıklarla çevrimdışı derleme için proje kökünde `export CARGO_HOME="$PWD/.setup/cargo" CARGO_NET_OFFLINE=true` kullanabilirsiniz.

## RAR karşılaştırması

MonaZip RAR5 karşılaştırmasında resmî `rar` komutunu seviye 3 ve tek iş parçacığıyla çalıştırır. Bu çalışma alanında Apple Silicon için RARLAB 7.23 deneme sürümü `.setup/rar/rar/rar` konumunda hazırdır. Uygulama ayrıca sistemde kurulu komutu arar. CLI’da farklı bir kurulum kullanmak için `MONAZIP_RAR_PATH=/tam/yol/rar` verilebilir. RAR komutu bulunamazsa rapordaki RAR alanı boş kalır ve durum gösterilir. [RARLAB indirmeleri](https://www.rarlab.com/download.htm)

## Ölçümler

gzip ve ZIP karşılaştırmaları DEFLATE seviye 6 ile yapılır. Arşiv başlıkları boyuta dahildir. RAR sürelerine harici işlemin başlatılması dahildir. Sıkıştırma süresi kaynak okuma ve geçici arşiv yazmayı, açma ve doğrulama süresi ise arşivi açıp kaynakla karşılaştırmayı kapsar. [Benchmark notları](benchmarks/README.md)

```sh
python3 benchmarks/db_text.py dosya.sqlite dokum.sql rapor.csv
```

Önceki stride ve blok boyutu araştırması CLI’da `--experimental` ile erişilebilir. Örneğin `./target/release/monazip --experimental dosya.bin --block-size 65536 --fixed-strides` başlangıçtaki sabit ayarları kullanır. [Araştırma tasarımı](MonaZip-PROJECT.md)
