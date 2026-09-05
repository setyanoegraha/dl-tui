# dl-tui

### Dashboard no oficial de DockerLabs — unofficial DockerLabs terminal dashboard

**dl-tui** is an interactive terminal dashboard for [DockerLabs](https://dockerlabs.es): browse the machine catalog, download machines, read and submit community writeups, rate machines, track your progress and generate your completion certificates — all without leaving the terminal.

Running `dl` opens the dashboard. Written in pure **Rust** (ratatui), shipped as a single static binary. The UI is in **Spanish**, staying true to the original platform.

> **Login required**: the dashboard starts with an in-app login popup (your password lives in the OS vault — never in plain text), because most of the platform's features are account-bound: progress, completed machines, ratings, writeup submissions and certificates.

---

## Features

* **Máquinas** — the full catalog (name, difficulty with the official colors, category, author, date, description) with instant `/` filtering and `s` sorting (site order → name → date → difficulty).
* **Downloads** — machines stream directly from DockerLabs' public storage: up to **2 in parallel** (extras queue), live gauges in the Downloads overlay, `.part` staging so partial files never look complete, `c` cancels.
* **Writeups** — per-machine community writeups popup (`w`): articles 📝 and videos 🎥, `Enter` opens them in the browser, `u` submits your own URL.
* **Valoraciones** — per-machine rating averages across 4 criteria (`v`), plus submitting your own scores (1–5).
* **Progreso** — your completion gauges per difficulty, statistics, the list of machines you marked done (`m` toggles), and certificate generation (`c`) with public `DL-XXXXXX` verification (`V`).
* **Rankings** — machine creators and writeup authors, side by side.
* **Account management in-app** (`a`) — switch account or logout; running downloads are never affected.
* Nord-themed interface.

---

## Prerequisites

* **OS**: Linux (developed and tested on Arch Linux); macOS and Windows release binaries are provided.
* A [DockerLabs](https://dockerlabs.es) account.
* A Secret Service provider on Linux (e.g. `sudo pacman -S gnome-keyring`) for credential storage.

---

## Installation

### 1. From a release binary

Grab the archive for your platform from the [Releases](https://github.com/setyanoegraha/dl-tui/releases) page:

| Platform | Archive |
| :--- | :--- |
| Linux x86_64 | `dl-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `dl-v0.1.0-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `dl-v0.1.0-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `dl-v0.1.0-x86_64-pc-windows-msvc.zip` |

```bash
tar xzf dl-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
install -m 755 dl ~/.local/bin/dl
```

### 2. From source

```bash
git clone https://github.com/setyanoegraha/dl-tui.git
cd dl-tui
cargo install --path .
```

### 3. From git directly

```bash
cargo install --git https://github.com/setyanoegraha/dl-tui.git
```

> Requires the Rust toolchain (1.85+): https://rustup.rs

---

## First Setup

Just run:

```bash
dl
```

The dashboard opens straight into the **Configurar DockerLabs** popup: type your username, `Tab`, your password (hidden), `Enter`. Credentials are validated with a real login before anything is stored. A failed login reopens the popup with your username kept; `Esc` quits.

---

## Usage Guide

Three keyboard-driven tabs — **Máquinas**, **Progreso** and **Rankings**:

| Keys | Action |
| :--- | :--- |
| `Tab` / `←` `→` | Switch tabs |
| `↑` `↓` / `j` `k` | Move selection |
| `g` / `Home` | Jump to the top of the list |
| `/` | Filter the current list (type to narrow, `Enter` keeps it, `Esc` clears & exits) |
| `s` | **Máquinas** — cycle sort: site order → nombre → fecha → dificultad. **Rankings** — toggle autores ↔ writeups |
| `d` | **Máquinas** — download popup: pick the destination folder (remembered across sessions), live progress in the Descargas overlay |
| `w` | **Máquinas** — community writeups popup: `j`/`k` select, `Enter` opens the link, `u` submits your own writeup |
| `v` | **Máquinas** — rating averages popup; `Enter` opens the form to send your 4 scores (1–5) |
| `m` | **Máquinas** — toggle the machine as completed (the platform's solved mechanic) |
| `i` / `Enter` | **Máquinas** — description popup with the full machine details |
| `c` | **Progreso** — generate the certificate of the selected completed machine (PDF opens in the browser) |
| `V` | Verify any certificate id (`DL-XXXXXX`) |
| `a` | Account popup: `Enter` switch account, `l` logout |
| `o` | Toggle the Descargas overlay (downloads keep running when closed) |
| `c` | In the Descargas overlay — cancel the most recent active download |
| `r` | Re-fetch all data |
| `q` / `Esc` / `Ctrl-C` | Quit (with active downloads, the first `q` lists them — press again to abort) |

Result popups stay until dismissed (`✓ ENVIADO`, `✗ REJECTED`, ...) and trigger a data refresh on close when your progress changed.

### Where Your Data Lives

- `~/.dl-tui/config.json` — your username and the last download folder. Nothing else.
- System vault — your password, under the `dl-tui` service. Never on disk in plain text.

---

## Updating

```bash
cargo install --git https://github.com/setyanoegraha/dl-tui.git --force
```

or grab the latest release binary from the [Releases](https://github.com/setyanoegraha/dl-tui/releases) page.

### Uninstallation & Cleanup

```bash
cargo uninstall dl
```

Delete `~/.dl-tui/` (and the `dl-tui` vault entry) to clear all local data.

---

## Disclaimer

dl-tui is an **unofficial** community tool and is not affiliated with DockerLabs. It only uses the platform's documented public API plus the same pages the web app uses, at human speed — be kind to the service.

---

## Acknowledgements

Thanks to [El Pingüino de Mario](https://github.com/Maalfer) and the DockerLabs community for the platform, and to the [ratatui](https://github.com/ratatui/ratatui) team for the toolkit.

Sibling project: [hmv-tui](https://github.com/setyanoegraha/hmv-tui) — the same dashboard concept for HackMyVM.

---

Made with ❤️ by [Ouba](https://github.com/setyanoegraha).

*¡Feliz hacking en DockerLabs!*
