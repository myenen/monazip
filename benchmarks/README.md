# Benchmark sonuçları

## SQLite ve metin karşılaştırması

Apple Silicon macOS’ta dokuz SQLite ve metin dosyası karşılaştırıldı. MonaZip Dengeli modda Zstd 6, gzip ve ZIP DEFLATE 6, RAR ise RARLAB 7.23 ile tek iş parçacığında RAR5 seviye 3 kullandı. RAR sürelerine harici sürecin başlatılması dahildir. Dosya başına beş çalıştırmanın medyan süreleri toplandı. Sıkıştırma kaynak okuma ve geçici arşiv yazmayı, açma süresi kaynakla bayt karşılaştırmasını da kapsar.

| Biçim | Toplam arşiv boyutu | Sıkıştırma | Açma ve doğrulama |
|---|---:|---:|---:|
| MonaZip (.zst) | 962.242 B | 33,21 ms | 4,02 ms |
| gzip (.gz) | 1.108.902 B | 39,90 ms | 6,15 ms |
| ZIP (.zip) | 1.109.826 B | 39,49 ms | 6,31 ms |
| RAR (.rar) | 843.701 B | 434,26 ms | 51,94 ms |

MonaZip gzip’ten 8/9, ZIP’ten 9/9 dosyada daha küçük çıktı üretti. Chinook SQLite dosyası 328.078 B oldu. Aynı dosya gzip ile 421.797 B, ZIP ile 421.905 B oldu. gzip’in önde olduğu `cp.html` örneği 7.967 B’ye karşı 8.025 B çıktı. Bu dokuz dosya Dengeli mod için yapılan seviye karşılaştırmasında da kullanıldı. RAR toplamda MonaZip’ten %12,3 daha küçük çıktı. Chinook SQLite RAR ile 225.860 B oldu. Dört biçimdeki 180 arşiv açılarak kaynakla birebir doğrulandı.

SQL dökümü Chinook’un SQLite dosyasından üretildi. Diğer örnekler USGS CSV, npm JSON kilit dosyası ve Canterbury metinleri arasından seçildi. Sonuçlar `db-text-results.json` dosyasındadır. [Chinook](https://github.com/lerocha/chinook-database) · [Canterbury Corpus](https://corpus.canterbury.ac.nz/descriptions/) · [USGS katalog sorgusu](https://earthquake.usgs.gov/fdsnws/event/1/query?format=csv&starttime=2024-01-01&endtime=2024-03-01&minmagnitude=4.5)

```sh
python3 benchmarks/db_text.py dosya.sqlite dokum.sql rapor.csv
```

## Önceki deneysel blok kodlayıcı

Apple Silicon macOS üzerinde 64 KiB bloklarla 19 dosya ve iki Zstd seviyesi ölçüldü. 38 deneysel çıktı da açılarak bayt bayt ve BLAKE3 ile doğrulandı. Ölçümler bayt cinsindendir.

## Bağımsız dosyalar

| Dosya | Deneysel 3 | Zstd akış 3 | Deneysel 9 | Zstd akış 9 |
|---|---:|---:|---:|---:|
| USGS CSV | 66.974 | 64.367 | 62.750 | 58.627 |
| libjpeg-turbo JPEG | 5.571 | 5.606 | 5.557 | 5.601 |
| Chinook SQLite | 332.671 | 353.237 | 306.231 | 323.644 |
| MonaZip npm kilit dosyası | 9.575 | 9.514 | 9.024 | 9.027 |

Chinook SQLite’da deneysel kapsayıcı, Zstd akışından seviye 3’te %5,82 ve seviye 9’da %5,38 daha küçük çıktı. USGS CSV’de sırasıyla %4,05 ve %7,03 daha büyük çıktı.

Kullanılan kaynaklar [USGS katalog sorgusu](https://earthquake.usgs.gov/fdsnws/event/1/query?format=csv&starttime=2024-01-01&endtime=2024-03-01&minmagnitude=4.5), [libjpeg-turbo 3.1.2 JPEG örneği](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/3.1.2/testimages/testorig.jpg) ve [Chinook SQLite](https://github.com/lerocha/chinook-database/blob/master/ChinookDatabase/DataSources/Chinook_Sqlite.sqlite) dosyalarıdır.

## Seçilmiş sentetik örnekler

| Dosya | Kaynak | Deneysel 3 | Zstd akış 3 |
|---|---:|---:|---:|
| Rampa | 262.144 | 177 | 291 |
| 73 baytlık satır ilişkisi | 262.144 | 213 | 18.736 |
| Gürültülü matris | 262.144 | 129.928 | 221.479 |
| Rastgele baytlar | 262.144 | 262.241 | 262.159 |

Kapsayıcı ek yükü bazı dosyalarda kazancı tüketti. Bütün dosyayı tek akışta işleyen Zstd, blok sınırlarının ötesindeki tekrarları kullanabildi. Sentetik örnekler, belirli ilişkilere duyarlılığı gösterir. Bağımsız örnekler dört dosyayla sınırlıdır.

`results.json` bu çalıştırmanın SHA-256 değerlerini, sürelerini, baskın yöntemlerini ve Zstd blok referanslarını da içerir. `run.py --real` ile aynı ölçüm başka dosyalarda tekrarlanabilir.

## Otomatik stride ve blok boyutu

[Canterbury Corpus](https://corpus.canterbury.ac.nz/descriptions/) içindeki 11 dosya, Zstd 3 ile dört yapılandırmada ölçüldü. Sabit yapılandırma 64 KiB blokları ve başlangıçtaki stride listesini kullanır. Süreler dosya analiz sürelerinin toplamı, bellek ise koşudaki en yüksek süreç RSS değeridir.

| Yapılandırma | Kapsayıcı toplamı | Analiz süresi | Tepe RSS | Kaynaktan büyük |
|---|---:|---:|---:|---:|
| Sabit | 616.367 B | 0,94 sn | 11,1 MiB | 0/11 |
| Yalnızca otomatik stride | 616.367 B | 1,02 sn | 10,2 MiB | 0/11 |
| Yalnızca otomatik blok boyutu | 571.044 B | 4,62 sn | 24,5 MiB | 0/11 |
| İkisi de otomatik | 571.044 B | 5,07 sn | 24,4 MiB | 0/11 |

Tek akışlı Zstd toplamı 633.109 B oldu. İki özelliğin açık olduğu durumda sabit MonaZip ayarına göre %7,35, tek akışlı Zstd’ye göre %9,80 daha küçük çıktı üretildi. Dört yapılandırmadaki 44 çıktı birebir doğrulandı. Bu corpus’ta stride keşfi çıktı boyutunu değiştirmedi.

73 bayt arayla değişen 256 KiB’lik sentetik kontrolde, sabit ayar 259.049 B, yalnızca stride keşfi 69.525 B, iki otomatik seçim birlikte 67.453 B üretti. Üç sonuç da doğrulandı. Veri Python `random.Random(321)` ile başlatıldı. İlk 73 baytı izleyen her bayt, 73 konum önceki değere rastgele −1, 0 veya +1 eklenerek mod 256 üretildi.

`adaptive-results.json` dosya başına ölçümleri, seçilen blok boyutlarını ve kaynak SHA-256 değerlerini içerir. Corpus arşivini bir klasöre çıkardıktan sonra ölçümü tekrar çalıştırabilirsiniz.

```sh
python3 benchmarks/adaptive.py .setup/canterbury/files
```
