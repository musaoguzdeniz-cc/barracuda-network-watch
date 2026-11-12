# Barracuda Network Watch

Windows 10/11 x64 için Rust ile yazılmış masaüstü ağ izleyicisi.

[Windows EXE'yi indir](https://github.com/musaoguzdeniz-cc/barracuda-network-watch/releases/latest/download/BarracudaNetworkWatch.exe)

[Network Watch + IP Analizi ZIP'ini indir](https://github.com/musaoguzdeniz-cc/barracuda-network-watch/releases/latest/download/BarracudaNetworkWatch-IP-Analysis-win-x64.zip)

## Kullanım

1. Uygulamayı aç ve listeden bir TCP bağlantısı seç. Uygulama adı, uzak IP, port ve duruma ek olarak her satırda son örnekleme aralığındaki indirme ve yükleme hızı gösterilir.
2. Seçili bağlantının kartında izleme sırasında ölçülen toplam aktarımı ve son 60 saniyenin hız grafiğini gör. Windows sayaçları açmak için yetki istediğinde EXE'yi sağ tıklayıp **Yönetici olarak çalıştır** seçeneğini kullan.
3. **Rotayı ölç** düğmesi seçilen uzak IP'ye ayrı IPv4 veya IPv6 ICMP denemeleri gönderir. Yanıtlar geldikçe hop IP'si ve bilgisayardan gidiş-dönüş süresi gösterilir. **Ölçümü durdur** ile işlemi kesebilirsin.
4. **IP’yi incele** seçeneği için [Barracuda IP Intelligence](https://github.com/musaoguzdeniz-cc/barracuda-ip-intelligence) paketindeki `ipintel.exe` dosyasını bu EXE ile aynı klasöre koy. Araç, seçtiğin genel IP için sağlayıcı sorgularını başlatır ve JSON raporunu uygulamada gösterir.

Windows TCP EStats sayaçları TCP veri yükünü ölçer ve yeniden gönderilen veriyi de sayar. Hız, ardışık sayaç değerlerinden yaklaşık saniyede bir hesaplanır. Sayaçları okunamayan satırlarda **—** gösterilir. Bir ölçüm boşluğunda eksik örnekler toplam ve grafiğe dahil edilmez. UDP ve QUIC bu sürümün kapsamı dışındadır. Bağlantı listesi, sayaçlar ve grafik bellekte işlenir.

Rota paneli ICMP denemelerinde görülen yolu gösterir. Uygulamanın TCP trafiği başka bir yol izleyebilir. Bir hop yanıt vermese bile sonraki hop’lar ve hedef yanıt verebilir. Rota ölçümü yalnızca düğmeye basınca başlar.

## IP analizi paketi

Hazır ZIP’i bir klasöre çıkar ve `BarracudaNetworkWatch.exe` dosyasını aç. Paketteki `ipintel.exe`, **IP’yi incele** düğmesi tarafından aynı klasörden çalıştırılır. İki projeyi yan yana klonlayıp Release olarak derledikten sonra `./scripts/package-ip-analysis.ps1` ile ZIP’i oluşturabilirsin. Komut yalnızca iki EXE, kullanım metni ve lisansları paketler.

## Derleme

Windows x64 MSVC Rust 1.95 veya sonrası ve Visual Studio C++ Build Tools gerekir.

```powershell
cargo test --locked
cargo build --release --locked
.\target\release\BarracudaNetworkWatch.exe
```

## English

Barracuda Network Watch is a Windows x64 Rust desktop app for inspecting live IPv4 and IPv6 TCP connections, per-connection upload and download rates, measured byte totals, and a 60-second traffic chart. Run as administrator if Windows denies access to extended TCP statistics. Select a connection and use **Rotayı ölç** to run an on-demand ICMP trace. For IP intelligence reports, place the companion `ipintel.exe` next to the app.

[Download the Windows release](https://github.com/musaoguzdeniz-cc/barracuda-network-watch/releases/latest)

[MIT License](LICENSE)
