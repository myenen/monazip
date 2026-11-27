# MonaZip --- Matematiksel Sıkıştırma Araştırma Projesi

**Sürüm:** 0.1 / MVP · **Durum:** Araştırma prototipi · **Dil:** Rust

## 1. Amaç

Herhangi bir dosyanın binary içeriğini analiz ederek farklı boyutlarda, byte ve bit düzeyinde matematiksel ilişkiler keşfetmek; bu ilişkilerin kayıpsız sıkıştırmaya sağlayabileceği katkıyı ölçmek.

İlk sürüm bir arşivleme uygulaması değildir. Kullanıcı dosya seçer, analiz başlatır ve hangi yöntemle ne kadar küçültme elde edilebildiğini görür. Dosya değiştirilmez.

**Hipotez:** Veri türünü önceden bilmeden, uygun komşuluk mesafeleri, matematiksel tahminler ve tersinir dönüşümler keşfedilerek klasik sıkıştırıcılardan daha ekonomik temsiller elde edilebilir. Bu hipotez henüz doğrulanmamıştır. Delta, tahmin, bit-plane ve çok boyutlu kodlama bilinen yöntemlerdir; özgünlük otomatik keşif stratejisinde aranacaktır.

## 2. MVP kullanıcı deneyimi

1.  Dosya seç veya sürükle-bırak.
2.  Dosya adı, boyutu ve analiz modu görüntülensin.
3.  Analizi başlat; ilerleme, geçen süre ve iptal seçeneği gösterilsin.
4.  Sonuç ekranında orijinal boyut, en iyi doğrulanmış deneysel boyut, tasarruf yüzdesi, kullanılan yöntem ve Zstd karşılaştırması gösterilsin.
5.  Blok detayları ve JSON raporu isteğe bağlı görüntülenebilsin veya dışa aktarılabilsin.

### Örnek sonuç ekranı (temsili, ölçülmüş sonuç değildir)

  Ölçüm                                     Sonuç
  ------------------------- ---------------------
  Orijinal boyut                  10.000.000 byte
  En iyi deneysel çıktı            3.200.000 byte
  Tasarruf                                    %68
  Zstd referans çıktısı            4.000.000 byte
  Zstd'ye göre ek küçülme                     %20
  En iyi yöntem               Stride delta + Zstd
  Kayıpsızlık                  Birebir doğrulandı

Grafik: orijinal / Zstd / deneysel çıktı. Entropi, en iyi stride, model dağılımı ve blok bazlı kazanç ayrı detay panelinde gösterilir.

## 3. Teknik mimari

-   Rust: bağımsız analiz ve deneysel codec kütüphanesi.
-   Tauri 2 + React + TypeScript: önerilen hafif masaüstü arayüzü; macOS öncelikli, Windows/Linux uyumlu tasarım.
-   CLI: arayüzden bağımsız test ve benchmark için.
-   Yerel çalışma; ağ, hesap, LLM veya sunucu gerektirmez.
-   Dosya bloklar halinde okunur; tüm dosya RAM'e yüklenmez.

``` text
File → Block Reader → Statistics → Geometry Explorer
     → Candidate Predictors → Residual Encoding
     → Cost Optimizer → Round-trip Verification → Report
```

## 4. Analiz motoru

### 4.1 Bloklama ve istatistik

Varsayılan blok 64 KiB; son blok gerçek uzunluğuyla işlenir. İleride 16/256 KiB ve 1 MiB karşılaştırılabilir. Byte histogramı, Shannon entropisi, sıfır oranı, tekrar oranı ve komşuluk farkları hesaplanır.

Shannon entropisi: H = -Σ p(x) log2 p(x). Bu değer yalnızca seçilen sembol modeline ilişkin teorik bir göstergedir; gerçek sıkıştırılmış boyut değildir.

### 4.2 Geometry Explorer

Binary akış sanal matrisler olarak yorumlanır. Fiziksel kopyalama gerekmez: index = y \* width + x.

-   İlk aday stride değerleri: 1--64, 128, 256, 512, 1024.
-   Her stride için eşitlik oranı, modüler fark histogramı ve XOR histogramı ölçülür.
-   Yatay, dikey ve çapraz komşuluklar test edilir; satır sınırları doğru ele alınır.
-   En iyi 4--8 aday gerçek kodlama aşamasına geçer; sıralama tahmini kodlama maliyetine dayanır.
-   3D ve daha yüksek boyutlar MVP sonrasına bırakılır. Boyutlandırma kendi başına sıkıştırma değildir.

### 4.3 İlk tersinir modeller

  Model                 Kodlama / tahmin
  --------------------- ---------------------------------------------
  RAW                   Orijinal byte dizisi
  DELTA                 r\[i\] = (b\[i\] - b\[i-1\]) mod 256
  STRIDE_DELTA          r\[i\] = (b\[i\] - b\[i-s\]) mod 256
  STRIDE_XOR            r\[i\] = b\[i\] XOR b\[i-s\]
  LINEAR_2              p\[i\] = (2\*b\[i-1\] - b\[i-2\]) mod 256
  SPATIAL               p = (left + above - upper_left) mod 256
  TRANSPOSE             Matris sütunlarını ardışık hale getir
  BIT_SHUFFLE           Byte bitlerini bit düzlemlerine ayır
  CONSTANT / PERIODIC   Sabit değer veya tam tekrar eden kısa desen

Tahmin tabanlı modellerde residual = (byte - prediction) mod 256. Decoder toplama işlemiyle orijinali geri üretir. XOR kendi tersidir. İlk s byte veya gerekli sınır değerleri aynen saklanır. Transpose ve bit-shuffle için uzunluk ve padding bilgisi korunur. MVP'de kayan noktalı tahmin ve yaklaşık eşleşme yoktur.

### 4.4 Gerçek boyut ölçümü

Her adayın residual veya dönüştürülmüş verisi Zstd ile kodlanır (varsayılan seviye 3). RAW ve dönüşümsüz Zstd de adaydır. Sabit/periyodik modeller tam eşleşmede doğrudan parametrelerle temsil edilebilir.

Toplam maliyet = dosya başlığı + blok başlıkları + model kimlikleri + parametreler + başlangıç değerleri + kodlanmış payload + bütünlük alanları.

MVP için geçici deneysel container gerçekten serialize edilir; yalnızca entropiden boyut tahmini yapılmaz. Önerilen little-endian format:

-   Dosya başlığı: magic (4 B), sürüm (1 B), orijinal uzunluk (u64), blok boyutu (u32), BLAKE3 özeti (32 B).
-   Her blok: model ID (u8), codec ID (u8), orijinal uzunluk (u32), parametre uzunluğu (u16), payload uzunluğu (u32), parametreler, payload.
-   Parametrelerde stride, desen veya sınır bilgileri gerektiği kadar saklanır. Başlangıç byte'ları payload içinde veya parametrelerde bulunur, iki kez sayılmaz.
-   En küçük gerçek maliyetli aday seçilir. Container maliyeti nedeniyle rastgele veride büyüme mümkündür; bu dürüstçe raporlanır.

Deneysel çıktı geçici dosyaya yazılıp decode edilir; orijinalle blok bazında byte-byte karşılaştırılır ve BLAKE3 doğrulanır. Başarısız aday sonuç olarak sunulmaz. Geçici dosya işlem sonunda temizlenir. MVP'de kullanıcıya arşiv kaydetme/açma özelliği sunulmaz.

## 5. Sonuç metrikleri

-   Tasarruf (%) = 100 × (1 - deneysel_boyut / orijinal_boyut).
-   Sıkıştırma oranı = orijinal_boyut / deneysel_boyut.
-   Zstd'ye göre kazanç (%) = 100 × (1 - deneysel_boyut / zstd_boyut).
-   Negatif tasarruf büyüme olarak gösterilir; sıfıra yuvarlanmaz.
-   Boş dosyada oranlar N/A gösterilir.
-   Entropi tahmini ile gerçek doğrulanmış boyut ayrı alanlardır.
-   Zstd referansı hem bütün dosya akışı hem aynı blok düzeniyle ölçülür; seviye ve framing farkları raporlanır.
-   Süre, throughput, seçilen model sayıları ve metadata maliyeti kaydedilir.

## 6. Proje dizini

``` text
monazip/
  Cargo.toml
  crates/
    core/src/
      lib.rs
      block.rs
      statistics.rs
      geometry.rs
      models.rs
      residual.rs
      codec.rs
      optimizer.rs
      verify.rs
      report.rs
    cli/src/main.rs
  app/                 # Tauri + React
  tests/
  benchmarks/
  docs/
```

Önerilen bağımlılıklar: zstd, blake3, serde, serde_json, clap; arayüz için Tauri ve dosya seçici. Paralellik ve SIMD ilk sürümde zorunlu değildir.

## 7. Arayüz / core sözleşmesi

Core, `analyze(path, options, progress, cancel) -> Result<AnalysisReport, AnalysisError>` benzeri bir API sunar. İşlem UI thread dışında yürütülür. Tauri komutları analiz başlatma, iptal ve rapor alma ile sınırlıdır.

Rapor alanları: original_bytes, experimental_bytes, metadata_bytes, zstd_stream_bytes, zstd_block_bytes, savings_percent, verified, elapsed_ms, block_count, model_counts, top_strides, entropy_bits_per_byte, options, warnings. Büyük dosyalarda blok detayları sınırlı veya sayfalı tutulur.

## 8. Test ve benchmark

Sentetik veri: sabit byte, rampalar, periyodik desen, stride ilişkileri, gürültülü matris, rastgele byte. Gerçek veri: CSV, JSON, SQLite, ham görüntü, sensör verisi, executable, JPEG, ZIP ve şifreli veri.

Her model için encode/decode round-trip, eksik son blok, stride \>= blok uzunluğu, boş dosya, bozuk başlık ve taşma testleri. Decoder boyut/parametre sınırlarını doğrulamalıdır. Dosya değişirse analiz iptal edilmeli veya hata vermelidir. Kaynak dosya salt okunur açılır.

Zstd seviye 3 ve 9 ile karşılaştır; ileride Brotli, LZMA ve veri türüne uygun tahmin tabanlı codec'ler ekle. Sentetik veri başarısı gerçek dünya başarısı olarak sunulmaz. Ayarları seçmek için kullanılan veri ile bağımsız test corpus'u ayrılır.

## 9. Geliştirme sırası ve kabul kriterleri

-   [ ] Rust core: blok okuma, histogram, entropi.
-   [ ] Geometry Explorer: stride taraması ve aday sıralama.
-   [ ] Tersinir modeller ve round-trip testleri.
-   [ ] Zstd referansı, deneysel container, gerçek maliyet seçimi.
-   [ ] CLI ve JSON raporu.
-   [ ] Masaüstü dosya seçimi, ilerleme, iptal ve sonuç ekranı.
-   [ ] Benchmark ve sonuçların belgelenmesi.

MVP tamamlanmış sayılır: herhangi bir okunabilir dosya seçilebiliyor; kaynak değişmeden analiz ediliyor; en iyi deneysel çıktı gerçekten kodlanıp doğrulanıyor; metadata dahil boyut ve Zstd karşılaştırması gösteriliyor; sıkıştırılamayan dosyalar doğru raporlanıyor; hata ve iptal yönetiliyor. Bellek dosya büyüklüğünden bağımsız, blok ve aday sayısıyla sınırlı tutuluyor.

## 10. MVP sonrası araştırma

Otomatik blok sınırları, çok boyutlu stride kombinasyonları, model zincirleri, modüler denklemler, sembolik regresyon, bit düzeyinde ilişkiler ve özel entropy coder (rANS) araştırılabilir. Her ek yöntem toplam byte maliyeti ve çalışma süresiyle değerlendirilir. Literatür taraması ve bağımsız benchmark yapılmadan yenilik veya üstünlük iddiasında bulunulmaz.

## 11. AI geliştirme ajanına talimat

Önce core ve CLI geliştir; GUI'yi çalışan ve test edilmiş motorun üzerine kur. Küçük modüller, açık tipler, deterministik sonuçlar, güvenli Rust ve birim testleri kullan. Sahte sonuç üretme; ölçülmeyen metrikleri tahmin olarak işaretle. Sıkıştırma avantajı göstermeyen modelleri sırf mimariyi büyütmek için ekleme. Bu belge başlangıç tasarımıdır; ölçümlerle değiştirilebilir.
