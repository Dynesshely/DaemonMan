<p align="center">
  <img src="daemon-man/src-tauri/icons/icon.png" width="112" alt="DaemonMan logo" />
</p>

<h1 align="center">DaemonMan</h1>

<p align="center">
  <strong>Workflow-driven Windows startup & daemon manager.</strong><br />
  Replace fragile startup scripts with a visual pipeline.
</p>

<p align="center">
  <img alt="Windows" src="https://img.shields.io/badge/platform-Windows%2010%2F11-0078D4?style=flat-square&logo=windows11&logoColor=white" />
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-000000?style=flat-square&logo=rust&logoColor=white" />
  <img alt="Vue 3" src="https://img.shields.io/badge/Vue-3-42B883?style=flat-square&logo=vuedotjs&logoColor=white" />
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178C6?style=flat-square&logo=typescript&logoColor=white" />
  <img alt="Version" src="https://img.shields.io/github/v/tag/Dynesshely/DaemonMan?style=flat-square&label=version&color=4f46e5" />
  <img alt="License" src="https://img.shields.io/badge/license-GPL--3.0-4f46e5?style=flat-square" />
  <img alt="Stars" src="https://img.shields.io/github/stars/Dynesshely/DaemonMan?style=flat-square&color=f59e0b" />
</p>

DaemonMan launches applications in the right order, branches on their results, skips processes that are already running, and takes over Windows autostart entries. Built with Tauri 2, Rust, and Vue 3.

## Features

- **Visual workflow editor** — sequential, parallel, and edge-driven groups; drag to reorder; snap-to-grid; auto-layout.
- **Smart startup** — skip processes that are already running, including browser-installed PWAs and multiple browser profiles.
- **Autostart takeover** — manage registry Run/RunOnce, Startup folders, scheduled tasks, and services from one place.
- **Runtime insight** — live run status, skip reasons, process tracking, and structured logs.

## Install

Windows 10/11 (x64). Download the latest installer from [Releases](https://github.com/Dynesshely/DaemonMan/releases/latest) and run it.

> Requires WebView2 Runtime. Windows 11 includes it; Windows 10 may need the Evergreen Runtime.

Silent install (no desktop shortcut):

```powershell
DaemonMan_<version>_x64-setup.exe /S /NS
```

## How to Dev

Prerequisites: Node.js 20+, pnpm, Rust (MSVC toolchain), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```powershell
git clone git@github.com:Dynesshely/DaemonMan.git
cd DaemonMan\daemon-man
pnpm install
pnpm tauri dev
```

Build the production installer:

```powershell
pnpm tauri build --bundles nsis
```

Checks:

```powershell
pnpm build
pnpm test:rust
```

Dev builds use `%USERPROFILE%\.dev-daemonMan`; installed builds use `%USERPROFILE%\.daemonMan`.

## Structure

```text
DaemonMan/
└─ daemon-man/                 # desktop application
   ├─ src/                     # Vue 3 + TypeScript UI
   ├─ src-tauri/               # Rust backend
   │  ├─ src/commands/         # IPC commands
   │  ├─ src/engine/           # workflow execution
   │  ├─ src/platform/         # Windows integration
   │  └─ tauri.conf.json
   └─ docs/ARCHITECTURE.md     # architecture notes
```

## Star History

<p align="center">
  <a href="https://star-history.com/#Dynesshely/DaemonMan&Date">
    <img src="https://api.star-history.com/svg?repos=Dynesshely/DaemonMan&type=Date" alt="Star History Chart" />
  </a>
</p>

## License

[GPL-3.0](LICENSE) © 2025-2026 Dynesshely
