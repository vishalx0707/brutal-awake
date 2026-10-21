# Brutal Awake

One Windows toggle. ON prevents idle sleep and display timeout. OFF allows your normal Windows behavior. It starts ON; minimizing keeps it working, and closing it exits completely. Closing the lid and choosing Sleep still follow Windows settings.

The paper background, status word and switch are drawn directly by Windows. No browser, Node, .NET, service, tray worker, updater, network access, telemetry, or settings files. One executable, one UI thread; Windows may create its own helper threads. There is no polling or animation loop.

## Run or replace your installed copy

Run `dist\BrutalAwake.exe`. No installer or administrator access is needed.

To update the copy you already installed, close the old app **normally** so it can restore its power settings. Copy `dist\BrutalAwake.exe` into `%LOCALAPPDATA%\Brutal Awake\` and rename the copied file to **`brutal-awake.exe`**, replacing the old file. Keep the hyphen so your existing shortcut opens the new version. The installed copy is not automatically changed by edits in this repository.

The new toggle never changes your sleep, hibernate or lid settings. It does not repair settings previously left changed by a crashed old version; check Windows Power Options if that happened.

## Build

Requires the Windows MSVC Rust toolchain and Microsoft C++ Build Tools. No npm install or browser runtime is required.

```powershell
.\build.ps1
```

Only `dist\BrutalAwake.exe` is needed at runtime. `target\` contains disposable compiler output; `cargo clean` removes it after building.

## Verify

```powershell
cargo test --release --locked
.\tests\smoke.ps1
```

The native test checks the Windows execution flags on the thread that owns them. The smoke check launches its own copy, tests startup, duplicate prevention, 500 toggles, resize, minimize, idle CPU/memory, unchanged power settings, and complete exit. It saves screenshots and measurements under `docs\`. Close other copies before running the smoke check.

See [CHANGES.md](CHANGES.md) for the audit and plain-language explanations. MIT license.
