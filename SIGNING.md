# Signing & Distribusi — MergeSort Pro

Binary saat ini **unsigned**. Windows SmartScreen / Defender akan flag
`More info → Run anyway` + `Unblock` di Properties. Itu normal untuk
binary baru, bukan virus. Lihat `INSTALL.md`.

## Cara sign Windows (bila mau hilang warning)

1. Beli sertifikat Code Signing (EV untuk SmartScreen reputasi instan,
   OV lebih murah tapi butuh bangun reputasi). Estimasi: OV ~$100-300/thn,
   EV ~$300-600/thn + token USB.
2. Atau Azure Trusted Signing (pay-as-you-go, tanpa token fisik).
3. Sign:
   ```powershell
   signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /a dist/MergeSortPro-*/mergesort.exe
   signtool verify /pa dist/MergeSortPro-*/mergesort.exe
   ```
4. Verifikasi hash tetap cocok dengan `*.sha256` di ZIP.

## macOS (bila build mac)

```bash
codesign --sign "Developer ID Application: ..." target/release/mergesort
xcrun notarytool submit dist/*.zip --wait
```

## MSI roadmap (belum disediakan)

Contoh WiX v4 minimal:

```powershell
dotnet tool install --global wix
wix init -o MergeSort.wxs
# isi: File mergesort.exe, StartMenu shortcut Start-GUI.bat, Uninstall
wix build MergeSort.wxs -o dist/MergeSortPro.msi
```

Untuk sekarang kirim ZIP + `Uninstall.bat` (hapus exe +
`%APPDATA%\MergeSort` + `.temp_sort`).
