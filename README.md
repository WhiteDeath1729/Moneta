# Moneta
A special, context aware, automatic tagging, Bookmarking system!!

## Windows release build

Build the Tauri release executable and NSIS installer with:

```powershell
.\build.ps1
```

Quit any running Moneta instance (including the tray app) before rebuilding.
The Tauri app under `src-tauri` is the release application. The script copies
its release executable to both `Moneta.exe` and `dist\Moneta.exe`; these are
matching convenience copies. Installing the NSIS package registers the
Ctrl+Shift shortcuts in the Windows Start Menu, so Windows can launch Moneta
with the selected action even when it is not already running. If Moneta is
already open, the action is forwarded to that instance.

The script also creates the downloadable, fully offline setup at
`dist\Moneta-Setup.exe`. It bundles the Qwen and BGE local AI models, the Qwen
tokenizer, and the WebView2 offline runtime; expect a download of roughly
650 MB. On a clean build, it downloads the pinned public model files and
verifies their integrity before compiling. Quit any running Moneta instance
before building.
