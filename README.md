# Brutal Awake

A minimal Windows app that keeps your laptop awake — no sleep, no display-off, **even with the lid closed** — until you toggle it off or shut down. No admin rights, no UAC prompt.

Built with Tauri v2 (Rust + WebView2). One window, one switch, Swiss-style paper UI.

## How it works

- Keep-awake starts **ON at launch**. Toggle OFF to allow sleep again.
- Uses `SetThreadExecutionState` (system + display + away-mode), re-asserted every 25 seconds, so it overrides whatever your power plan says.
- While ON, the lid-close action is set to *Do Nothing* (battery and plugged-in) via `powercfg`, and your original setting is **restored** when you toggle off or close the app.
- Single instance — launching it twice won't disturb the running copy or your saved settings.

## Install

Download the installer from the [Releases](https://github.com/vishalx0707/brutal-awake/releases) page and run it. Per-user install, no admin needed. (Windows SmartScreen may warn on first run since the binary is unsigned — choose "More info" → "Run anyway".)

## Build from source

Requires [Rust](https://rustup.rs/) (with the MSVC toolchain), Node.js, and the Microsoft C++ Build Tools.

```powershell
npm install
npx tauri build
```

Output: `src-tauri/target/release/brutal-awake.exe` and an NSIS installer under `src-tauri/target/release/bundle/nsis/`.

## Caveat

If the app is force-killed (Task Manager "End task"), it can't run its cleanup, so the lid-close action may stay at *Do Nothing*. Launch the app and close it normally to restore your setting.

## License

MIT
