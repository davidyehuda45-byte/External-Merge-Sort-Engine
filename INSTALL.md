# Install — External Merge Sort Engine

## Windows (30 detik)
1. Extract ZIP release
2. Double-click **`Start-GUI.bat`** → browser terbuka di `http://127.0.0.1:8080/`
3. Klik **Generate demo**, lalu **Urutkan sekarang**, lalu **Download**. Selesai.

> `Start-GUI.bat` hanya tersedia di ZIP installer, bukan di repo.
> Dari repo, build dulu (`cargo build --release`) lalu jalankan
> `.\target\release\mergesort.exe --gui 127.0.0.1:8080`.

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

### Catatan build Windows
Binary release memakai static CRT (`/MT`) agar berjalan tanpa instalasi
Visual C++ Redistributable tambahan. Build standar `cargo build --release`
sudah cukup; tidak perlu flag khusus.

## Linux / macOS (native)
```bash
# prasyarat: Rust 1.88+ (edition 2024), lihat https://rustup.rs
cargo build --release --locked
./target/release/mergesort --help
./target/release/mergesort --input data.csv --output sorted.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --header --verify
# atau via script:
./build.sh
```

## CLI (power user)
```powershell
.\mergesort.exe --input data.csv --output sorted.csv --format csv --key-column 0 --key-type numeric --header --verify
# --max-memory opsional (default 60% RAM); --backup sebelum timpa; --ignore-case/--nulls/--key-dir untuk sort semantik
```

## Daemon / service (gratis, tanpa MSI)
- Linux: `sudo cp deploy/mergesort.service /etc/systemd/system/ && sudo systemctl enable --now mergesort` (sesuaikan path di file).
- Windows: `powershell -ExecutionPolicy Bypass -File deploy/install-watch-task.ps1` (task logon: inbox `%USERPROFILE%\mergesort-inbox`).
- MSI: roadmap `deploy/MergeSort.wxs` (`wix build`). Signing berbayar: lihat `SIGNING.md`.

## Uninstall
- Windows ZIP: jalankan `Uninstall.bat` (hapus exe + `%APPDATA%\MergeSort` + `.temp_sort`), lalu hapus folder.
- Linux/mac: hapus binary + `~/.mergesort` + temp dir (`--temp-dir` bila dipakai).

## Catatan
- Offline 100%, tidak perlu internet. Jangan expose `--gui`/`--dashboard` ke internet publik (bind `127.0.0.1` saja).

## Docker
```bash
docker build -t mergesort .
docker run --rm -v "$PWD:/data" mergesort --input /data/in.csv --output /data/out.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --verify
# dengan live dashboard (port 8080 sudah EXPOSE di image):
docker run --rm -v "$PWD:/data" -p 8080:8080 mergesort --input /data/in.csv --output /data/out.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --verify --dashboard 0.0.0.0:8080
```

## Uninstall
1. Hapus folder aplikasi (hasil extract ZIP atau clone repo) — tidak ada entri registry/services.
2. Hapus data kerja/user jika ada: `%APPDATA%\MergeSort` (Windows) atau `~/.mergesort` (Linux/macOS).
3. Hapus temp sisa jika ada: `<workdir>/.temp_sort` dan folder `--temp-dir` kustom yang pernah dipakai.
