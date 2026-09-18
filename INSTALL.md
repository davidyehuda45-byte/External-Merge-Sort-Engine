# Install — External Merge Sort Engine

## Windows (30 detik)
1. Extract ZIP release
2. Double-click **`Start-GUI.bat`** → browser terbuka di `http://127.0.0.1:8080/`
3. Klik **Generate demo**, lalu **Urutkan sekarang**, lalu **Download**. Selesai.

### Muncul peringatan biru "Windows protected your PC"?
Ini normal untuk aplikasi baru yang belum beli sertifikat digital berbayar —
**bukan tanda virus**. Cara lanjut:
1. Klik tulisan kecil **"More info"** di dalam kotak peringatan.
2. Tombol **"Run anyway"** akan muncul di bawahnya — klik itu.
3. Peringatan ini hanya muncul sekali per file per komputer.

Kalau antivirus (Windows Defender/lainnya) mengkarantina file: klik kanan file
ZIP/exe → **Properties** → centang **"Unblock"** di bagian bawah → OK, lalu
extract ulang. Ini karena file diunduh dari internet (Windows menandai semua
file unduhan), bukan karena filenya berbahaya.

## CLI (power user)
```powershell
.\mergesort.exe --input data.csv --output sorted.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --header --verify
```

## Catatan
- Offline 100%, tidak perlu internet. Jangan expose `--gui`/`--dashboard` ke internet publik (bind `127.0.0.1` saja).

## Docker
```bash
docker build -t mergesort .
docker run --rm -v "$PWD:/data" mergesort --input /data/in.csv --output /data/out.csv --max-memory 1GB --format csv --key-column 0 --verify
```
