# Install — External Merge Sort Engine

## Windows (30 seconds)

1. Extract the release ZIP file.
2. Double-click **`Start-GUI.bat`**. Your browser will open:
   `http://127.0.0.1:8080/`
3. Click **Generate demo**, then **Sort now**, and finally **Download**.

That's it. No complicated setup required.

### Seeing the blue "Windows protected your PC" warning?

This can happen because the application is new and does not have a paid code-signing certificate yet.

**It does not automatically mean the application is a virus.**

To continue:

1. Click **"More info"** in the warning window.
2. Click **"Run anyway"**.
3. The warning normally only appears once for each file on each computer.

If Windows Defender or another antivirus program quarantines the file:

1. Right-click the downloaded **ZIP or EXE** file.
2. Select **Properties**.
3. Check **"Unblock"** at the bottom of the window.
4. Click **OK**.
5. Extract the ZIP file again.

Windows may mark files downloaded from the internet as coming from an external source. This warning by itself does not mean the file is malicious.

## CLI (For Advanced Users)

If you prefer using the command line:

```powershell
.\mergesort.exe --input data.csv --output sorted.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --header --verify
```

## Important Notes

* The engine works **100% offline** and does not require an internet connection.
* Do **not** expose `--gui` or `--dashboard` directly to the public internet.
* For local use, bind them to `127.0.0.1`.

## Docker

You can also run the engine with Docker:

```bash
docker build -t mergesort .
docker run --rm -v "$PWD:/data" mergesort --input /data/in.csv --output /data/out.csv --max-memory 1GB --format csv --key-column 0 --verify
```
