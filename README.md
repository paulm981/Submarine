<div align="center">
  <img src="src/assets/logo.png" alt="Submarine logo" width="120" />
  <h1>Submarine — SSH Client &amp; Session Manager</h1>
  <p><strong>A full-featured SSH and SFTP client and session manager for Windows, macOS and Linux.</strong></p>
  <p>Organize hundreds of servers, work on many at once, edit remote files, forward ports, and sync folders from one native window. Bring your sessions over from MobaXterm, PuTTY or an OpenSSH config. Built with Rust and Tauri.</p>
  <p>
    <a href="https://github.com/paulm981/Submarine/releases/latest"><img alt="download the latest release" src="https://img.shields.io/github/v/release/paulm981/Submarine?label=download&color=2ea44f" /></a>
    <a href="https://github.com/paulm981/Submarine/actions/workflows/build.yml"><img alt="build" src="https://img.shields.io/github/actions/workflow/status/paulm981/Submarine/build.yml?branch=main&label=build" /></a>
    <a href="https://github.com/paulm981/Submarine/releases/latest"><img alt="released" src="https://img.shields.io/github/release-date/paulm981/Submarine?label=released" /></a>
    <a href="https://github.com/paulm981/Submarine/releases"><img alt="downloads" src="https://img.shields.io/github/downloads/paulm981/Submarine/total" /></a>
    <img alt="platforms" src="https://img.shields.io/badge/platform-windows%20%7C%20macos%20%7C%20linux-blue" />
    <img alt="license" src="https://img.shields.io/badge/license-MIT-green" />
  </p>
</div>

> **This is a fork.** [paulm981/Submarine](https://github.com/paulm981/Submarine) builds on [Submarine](https://github.com/sinaxhpm/submarine) by Sina. It adds what a team needs to run a large, shared set of servers from one SSH client that works the same on Windows, macOS and Linux, and an easy way in for anyone coming from MobaXterm, PuTTY or an OpenSSH config. **Download** the [latest release](https://github.com/paulm981/Submarine/releases/latest) (on Windows, [`Submarine-windows-x64-setup.exe`](https://github.com/paulm981/Submarine/releases/latest/download/Submarine-windows-x64-setup.exe); see [Install](#install)). Releases, updates and issues for this fork live here.

## What this fork adds

The tour with more pictures is in [FORK.md](FORK.md), along with where the fork differs from the original project and how it keeps up with it. What changed in each build is in [CHANGELOG.md](CHANGELOG.md).

<table>
  <tr>
    <td width="50%" valign="top" align="center">
      <a href="FORK.md#work-on-many-servers-at-once"><img src="docs/screenshots/fork/wall-broadcast.png" alt="A Wall of six terminals behind one jump host, Broadcast on, one paused" /></a>
      <br/><sub><b>Walls</b>: type to many servers at once, with some paused</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <a href="FORK.md#organize-them"><img src="docs/screenshots/fork/highlighting-and-tree.png" alt="The session tree beside a terminal with highlighted log output" /></a>
      <br/><sub><b>Session tree and highlighting</b>: folders, servers and Remote Desktop links; errors, warnings and addresses colored</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top" align="center">
      <a href="FORK.md#a-better-terminal"><img src="docs/screenshots/fork/color-schemes.png" alt="Color schemes in Settings with a preview" /></a>
      <br/><sub><b>Color schemes</b>, one for the app and one per server if you like</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <a href="FORK.md#your-data-your-storage"><img src="docs/screenshots/fork/own-storage-sync.png" alt="Sync through a private git repository" /></a>
      <br/><sub><b>Sync through storage you own</b>: a synced folder or a private git repository</sub>
    </td>
  </tr>
</table>

**Bring your sessions with you**
- **Session import that keeps your setup**: from MobaXterm, nested folders, saved logins, jump hosts and their keys, startup commands such as `dzdo su -`, and Remote Desktop sessions come over as they were, and a result screen says what came in. PuTTY and OpenSSH config imports are there too.

**Work on many servers at once**
- **Walls with broadcast input**: terminals from many servers side by side. With Broadcast on, typing goes to all of them. Click into one to type to it alone, pause the ones to leave out (several at once: Ctrl-click their titles, or **Pause all** and resume the few you want), or paste or send a set of commands to all. A server that connects or reconnects partway through a line waits for the next one, so it never gets only the end of a command. Walls can be saved by name, and a whole folder opens as one.
- **Many tabs, easily**: **Ctrl+Shift+K** jumps to a tab by typing part of its name, **Ctrl+Tab** moves between tabs, and a tab's right-click menu closes the others, those to the right, the disconnected ones, or all of them.
- **Jump hosts at scale**: a hundred servers behind one jump host log in a few at a time (the limit is a setting), and Monitoring reaches servers through their jump host too.
- **Split**: a second shell on the same server beside the first, to watch a log in one and work in the other.

**Organize them**
- **A session tree** beside the terminal, with search: folders, servers and Remote Desktop links, managed by drag and drop, rename, move and delete. **Show only open** narrows it to the servers you have open.
- **Remote Desktop links** saved with your servers, opening in Remote Desktop Connection on Windows.
- **OS badges** on servers, and a look of their own for Remote Desktop links.

**AI help, without sharing keys** (a first test, off by default)
- **Claude Code from Home's menu**, already connected to Submarine. Share a server's tab with it, and it can ask to run commands there. Each one waits in the **AI panel** until you click Allow; the AI never gets a key or password, and the output it gets has known secrets hidden. Every request is logged. The design, and what comes next, is in [#32](https://github.com/paulm981/Submarine/issues/32).

**Root when you need it**
- **SFTP root mode and Info panel actions as root** through `sudo` or `dzdo` (Centrify / Delinea): without a password where the server allows it, including servers that only allow `dzdo su -`.

**A better terminal**
- **xterm.js 6** with color schemes (Campbell, One Half, Solarized, Dracula, Nord, Tokyo Night, Catppuccin, Gruvbox), and a scheme per server that syncs with it.
- **Syntax highlighting**: errors, warnings, OK, addresses, links and dates colored in the output, with sets for shell scripts, network devices and SQL, a set per server, and rules of your own that sync with the profile.
- **Fast output never freezes the window**: output is paced by what the screen has drawn, and floods of escape sequences that would lock it up are ignored.
- **A Home view** of terminals on your own computer: PowerShell, Command Prompt, Git Bash and each WSL distribution by name on Windows (the default one marked); the login shell and every other installed shell on macOS and Linux.
- The app's own dark right-click menus everywhere, and no right-click or F5 that reloads the app.

**Your data, your storage**
- **Sync through storage you own**: a folder that OneDrive, Dropbox, Nextcloud, Syncthing or a network share keeps the same on your computers, or a private git repository, with no third-party service. Profiles are encrypted with their password first, and Remote Desktop links sync too. The original project's hosted cloud stays off unless you turn it on.

**Ready to roll out to a team**
- **A release from every change**, numbered like `v0.4.0-fork.2` and shown in the title bar, with a notice when there's a newer one.
- **Troubleshooting**: a log on your computer, a watchdog that notes when the window froze and why, and **Copy diagnostics** with server, user and host names replaced, ready to send to whoever is helping. When a connect fails, Submarine says whether the name resolved and the port answered.
- **Hardening**: characters that disguise a file name are shown plainly, and a trusted server that shows a different kind of key is a warning, not a silent change. Live Edit opens a file in an editor and never runs it, and the Local pane asks before it opens a file that would run, on Windows, macOS and Linux. Downloads are marked as from the internet on Windows and macOS. As in the original project, a program on a server isn't opened from the file pane at all.
- **Close to the original project**: its changes come in through regular sync pull requests, and [FORK.md](FORK.md) lists where the fork differs and what it hasn't taken yet.

The original project's README follows.

---

<div align="center">
  <img src="docs/screenshots/hero.png" alt="Submarine main window — terminal session with dual-pane SFTP browser open on the right" width="900" />
</div>

> **TL;DR** — Submarine is a fast, free, open-source SSH and SFTP client for Windows, macOS, Linux, and Android. It replaces the typical PuTTY + WinSCP + tunnel-manager stack with one tabbed window per server: terminal, SFTP, port forwarding (SOCKS / HTTP / local / remote), folder mirror, and end-to-end encrypted profile sync. Built with Rust + Tauri. MIT licensed.

## At a glance

- **One window, many tools per server** — terminal, SFTP, tunnels, folder mirror, Docker, live server info, and a per-server command library, all as tabs around the same session
- **Watch many servers at once** — session **split view** to tile any two sessions side-by-side, plus **Walls**: grids of terminals from any servers, saved by name, a whole folder opened as one
- **Broadcast input** — on a Wall, type once and every terminal gets it; click into one to type to it alone (**Type to all** on the bar, or the ✕ on its tile, goes back), pause the ones to leave out, paste or send a set of commands to all (like tmux `synchronize-panes`, across servers)
- **Native and cross-platform** — Windows, macOS, Linux (`.deb` / `.rpm` / Arch / AppImage), and Android — same encrypted vault everywhere
- **Zero-knowledge cloud sync** — profiles are Argon2id + AES-256-GCM sealed on your device before upload; the server (and everyone else) only sees ciphertext. Browser dashboard at [`api.sinaxhpm.com/account`](https://api.sinaxhpm.com/account/) to manage your account and stored profiles
- **Docker manager** — containers, logs (live tail), stats, prune, and `docker exec` shells as first-class session tabs
- **Import from anywhere** — PuTTY `.reg`, MobaXterm `.mxtsessions`, OpenSSH config, and Submarine JSON exports all bulk-imported from the Servers page
- **Secure by default** — TOFU host keys with per-connection nonce binding, strict CSP, minimal Tauri permission ACL, zeroized master key

## Screenshots

<table>
  <tr>
    <td width="50%" valign="top" align="center">
      <a href="docs/screenshots/tunnels.png"><img src="docs/screenshots/tunnels.png" alt="Port forwarding panel: local, remote, and dynamic SOCKS tunnels with live connection counters" /></a>
      <br/><sub><b>Port forwarding</b> — local / remote / dynamic SOCKS tunnels, live active-connection counters, autostart with the app</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <a href="docs/screenshots/mirror.png"><img src="docs/screenshots/mirror.png" alt="Folder Mirror setup with local/remote paths, excludes, and conflict resolution" /></a>
      <br/><sub><b>Folder mirror</b> — two-way sync, excludes, per-mirror conflict resolution, soft delete to <code>.submarine-trash/</code></sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top" align="center">
      <a href="docs/screenshots/commands.png"><img src="docs/screenshots/commands.png" alt="Per-server command library with Run / Paste / Edit on each snippet, plus a Notes tab" /></a>
      <br/><sub><b>Quick commands &amp; notes</b> — per-server snippet library and a Notes tab for runbook / contacts</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <a href="docs/screenshots/logins.png"><img src="docs/screenshots/logins.png" alt="Logins panel: saved passwords and SSH keys with Add Password / Add Key / Generate Key" /></a>
      <br/><sub><b>Logins</b> — saved passwords and SSH keys, with built-in key generation; all credentials encrypted in the vault</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top" align="center">
      <a href="docs/screenshots/monitoring.png"><img src="docs/screenshots/monitoring.png" alt="Live monitoring grid: per-server CPU, memory, network-in and network-out sparklines" /></a>
      <br/><sub><b>Monitoring</b> — per-node SSH polling for CPU, RAM, and live network in/out with sparklines</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <a href="docs/screenshots/server-edit.png"><img src="docs/screenshots/server-edit.png" alt="Server details form: folder, tag colour, host/port, login method, proxy, tunnels, mirrors, autostart" /></a>
      <br/><sub><b>Server details</b> — folder, tag colour, host, auth, proxy, tunnels, mirrors, and autostart in one form</sub>
    </td>
  </tr>
</table>


## Looking for an alternative to…

If you're searching for one of these, Submarine is a direct fit:

- a free **PuTTY alternative for macOS and Linux** (and a tabbed PuTTY replacement on Windows)
- a **WinSCP alternative** that lives in the same window as your terminal
- a place to take your **MobaXterm** sessions that also runs on macOS and Linux — the import keeps folders, logins and jump hosts
- a free **Termius alternative** without subscription gating or telemetry
- a modern **Tabby / iTerm2 + SFTP** combo that's actually one app
- a cross-device **Termius-style sync** without a paid plan — your encrypted profiles travel from desktop to your **Android phone** with the same master password
- an **rsync GUI / Syncthing-lite** that mirrors a local folder to a remote SSH host with conflict resolution
- an **Android SSH client** (ConnectBot / JuiceSSH style) that shares saved servers with your desktop

## Why Submarine

Most SSH clients feel like they were built a decade ago and never updated. Submarine is a clean, modern alternative for developers and sysadmins who connect to remote servers every day.

- **Native and fast.** Built with Rust + Tauri — boots in under a second, uses around 80 MB of RAM with several sessions open. No Electron, no battery drain.
- **One window, everything you need.** Terminal, SFTP file browser, port tunnels, folder sync, and saved commands — all live as tabs around each server. No more juggling PuTTY plus WinSCP plus a tunnel app.
- **Secure by default.** Saved servers are encrypted on disk. Optional cloud sync only ever sees encrypted blobs — your passwords and keys can't be read by anyone but you.
- **Truly cross-platform.** Same app on Windows, macOS (Apple Silicon + Intel via Rosetta), and every major Linux distro — `.deb`, `.rpm`, Arch package, and AppImage.

## Key Features

### Terminal

- Tabbed sessions per server and multiple shells per session
- **Split** — the button next to + opens another shell on the same server beside the one you're in (watch a log in one, work in the other); right-click it to put an open tab beside this one or to stack them top and bottom
- **Fast output never freezes the app** — a busy `tail -f` or a `cat` of a huge file is sent as raw bytes and paced by what the window has drawn; when a stream stays faster than the screen can show, the oldest unseen part is skipped with a note in the terminal, and Ctrl+C still answers at once. Escape sequences with absurd counts (`ESC[999999999S`, which a binary file can contain by chance and which xterm.js would loop on for hours) are ignored
- Full xterm.js 6 — true colour, ANSI 256, mouse support, drawn with the GPU on Windows (WebGL; a switch in Settings, off by default on macOS and Linux), Unicode 11 widths so emoji and CJK line up
- **Color schemes** — Submarine's own plus Campbell, One Half, Solarized, Dracula, Nord, Tokyo Night, Catppuccin and Gruvbox (Settings › Terminal, with a live preview). A server can have **its own**, which syncs with it — e.g. a light scheme for production
- **Keep text readable** — text a program colors too close to the background is lifted to a 4.5:1 contrast, as in VS Code
- **Syntax highlighting** — words and addresses colored in what the terminal shows (Settings › Terminal › Highlighting, with a live preview and a box to paste your own output into):
  - **Standard** (the default): errors and failures in red, warnings in yellow, OK and success in green, IP and MAC addresses, links, dates and times, and "Last login"
  - **Shell script**, **Network devices** (interface names, up/down, permit/deny, config keywords; Cisco-style, Juniper names) and **SQL** add their own on top; **None** turns it off
  - **A set per server** in its edit panel, which syncs with it — e.g. Network devices for the switches
  - **Your own rules** — a whole word, text anywhere, or a regular expression, in a color with bold or underline. They sync with your profile and win where they and a set match the same text. An expression that would be slow enough to freeze the window is refused when you add it
  - Only the screen changes: what's copied, typed or sent is untouched. Full-screen programs (vim, less, top) and text a program colored itself are left alone, and in a flood (a `cat` of a huge file) highlighting stays out of the way so the output keeps up, while the screen it ends on is still highlighted
- **Clickable links** — URLs in the output and OSC 8 hyperlinks open in your browser after a prompt
- **Select to copy** — release the mouse over your selection and it's in the clipboard
- Right-click to paste
- Wrapped lines copy as a single line (no phantom blank rows)
- **Reconnect-safe scroll-back** — when a session drops and auto-reconnects, your previous output stays in the buffer to read and copy (a yellow `── reconnected ──` divider marks the boundary)
- **Container shells** — one click from the Docker tab opens a `docker exec` shell into any container, with `bash → sh → ash` auto-fallback so distroless / Alpine / Ubuntu all just work

### SFTP File Browser

- Dual-pane (local on top, remote on bottom) with drag-and-drop between them
- **Right-click the empty space** in a list for Refresh, New Folder, Select all and Copy folder path. Every right-click menu is the app's own, dark like the rest: text fields get Cut / Copy / Paste / Select all, selected text gets Copy, and the webview's menu (whose Reload would restart the whole app) never shows. F5 / Ctrl+R don't reload the app either
- **Tabs or Split layout** — toggle between stacked tabs (one pane visible, more screen real-estate) and the classic split, persisted per device
- **Per-pane filter input** — type to filter the listing live with an N/M counter, Esc to clear
- **Multi-select with Ctrl/Shift-click** plus **Select-All button and Ctrl+A** for bulk download, move, or delete
- **Overwrite confirmation** before clobbering an existing file (both uploads AND downloads — no more silent drag-drop clobber), with "apply to all" for batches
- **Live edit** — double-click any remote file to open it in your default editor; saves auto-upload back to the server
- **Direct download** to the local folder you already have open — no folder picker every time
- Permissions editor with a checkbox grid (chmod / chown)
- Drag files in from your OS file manager to upload

### Folder Mirror

Pick a local folder, pick a remote folder, and Submarine keeps them in sync.

- **Two-way initial sync.** Walks both sides, compares by size and content hash (not just timestamp), and merges them into the same state.
- **Live watcher.** Every save, create, rename, or delete on your local folder pushes to the server in real time.
- **Conflict resolution per mirror.** Pick what wins when a file differs on both sides: **Local wins** (default, like rsync), **Remote wins**, or **Newer wins**.
- **Soft delete.** Removed files move to `.submarine-trash/` on the server — a fat-fingered `rm` won't wipe out remote work.
- **Overwrite-risk advisory.** Before each Start, the dry-run preview calls out which files would be overwritten locally and warns that edits made during the sync window can be silently lost — so you know to save / close editors first.

### Server Info Panel

A read-only inspection pane for the active session — no terminal commands required.

- **Overview** — hostname, OS, kernel, uptime, load average, CPU count, RAM / swap meters, disk usage per mount
- **Network** — NICs (UP-first, with IPv4/IPv6 addresses and MAC), routing table, and **firewall rules** (`iptables` chains rendered per-chain with colored ACCEPT/DROP/REJECT badges; `nftables` raw view)
- **Ports** — listening sockets via `ss -tulnp` (or `netstat` fallback) with PID and process name, protocol + state filters
- **Services** — `systemctl` units with **Start / Stop / Restart** buttons
- **As root with sudo or dzdo** — a service action, a kill or firewall rules that need root run through `sudo` or `dzdo` (Centrify / Delinea). Without a password when the server allows it, else the panel asks once; the password is kept in memory for that tab only, and the tab's SFTP root mode shares it
- All tabs **lazy-load on click** so opening Info doesn't fire five probes at once
- sudo / dzdo errors (wrong password, not allowed, requiretty) are explained in plain words instead of a raw stderr dump

### Docker Manager

Manage Docker on any session host without typing a single `docker` command.

- **Containers** — list, **Start / Stop / Restart / Pause / Kill** inline, click a name for details
- **Logs** with adjustable tail (500 → 50 000) and a **Live stream** toggle that follows new output, smart auto-scroll
- **Stats** (CPU %, mem, net I/O, block I/O), **Inspect** (raw JSON with copy), parsed **Env / Mounts / Ports**
- **Resources** sub-tab for Images, Volumes, and Networks (lazy per-kind)
- **Compose viewer per container** — reads the compose file path from container labels and shows the YAML inline
- **Safe Prune** scopes only (containers / images / networks / builder / system) — never `--volumes`, never `-a` so nothing important gets eaten
- **Container terminal** — open an interactive `docker exec` shell as a new session tab
- Cross-platform (Linux / macOS / Windows hosts) — tries `docker` first, falls back to `sudo -n docker` on permission errors

### Port Forwarding

- Local (`-L`), Remote (`-R`), and Dynamic (`-D`) tunnels
- Dynamic supports **SOCKS4, SOCKS4a, SOCKS5, SOCKS5h, HTTP CONNECT, and plain HTTP proxy**
- Each tunnel starts and stops independently, saved with the server profile

### End-to-End Encrypted Profile Sync

- Save servers once, access them on every machine you own
- **Through storage you own** (this fork): a folder that OneDrive, Dropbox, Nextcloud, Syncthing or a network share keeps the same on your computers, or a private git repository on GitHub, GitLab or your own server. Set it up on the **Profile** tab; on another computer, choose **From storage** in the profile list and enter the profile's password.
- Everything is encrypted with the profile's password before it leaves your computer — names, hosts, logins and keys alike. Each computer writes only its own file, so a sync tool never has two versions of one file to choose between. A git branch is kept to a single commit that each sync replaces, so no history piles up; git signs in the way it does on that computer (an SSH key, or Git Credential Manager). Give it a private repository and a branch of its own (by default `submarine-sync`; a branch that holds other work is refused): whoever can read the repository can try passwords against it offline, slowed by the same Argon2id work that guards the vault.
- The original project's hosted cloud, with sign-in and sharing, is still there, off unless you turn it on under **Settings → Sync**. Its server only ever holds encrypted data too (see [Security](#security))
- No registration. No password recovery email. Your master password never leaves your device.

### Productivity

- **Quick commands**: per-server snippet library, surfaced inside any open session
- **Per-node notes**: a dedicated description / runbook / contacts field on every server, scoped only to that server
- **Autostart**: flag a server and it opens and connects when the app launches
- **Resource monitor**: live CPU, RAM, disk, and network sparklines per host
- **Quick connect**: one-off sessions without saving
- **TOFU host keys**: first-use prompt with fingerprint, pinned afterwards

## Install

Download from the **[latest release](https://github.com/paulm981/Submarine/releases/latest)** — no extra dependencies. GitHub makes a release from every change merged into `main`, and the file names stay the same from one release to the next, so a link to one always gets the newest:

| OS | File |
|---|---|
| Windows 10 / 11 | [`Submarine-windows-x64-setup.exe`](https://github.com/paulm981/Submarine/releases/latest/download/Submarine-windows-x64-setup.exe) — installs, or updates an installed copy (profiles and settings are kept). Or [`Submarine-windows-x64-portable.zip`](https://github.com/paulm981/Submarine/releases/latest/download/Submarine-windows-x64-portable.zip), no install — see [portable mode](#portable-mode-windows) |
| macOS (Apple Silicon, or Intel via Rosetta) | `Submarine-macos-arm64.dmg` or `.app.zip`, in the releases that include macOS (see [Builds on GitHub](#builds-on-github)), or [build it on your Mac](#builds-on-github) |
| Debian / Ubuntu / Mint | `Submarine-linux-amd64.deb` — `sudo apt install ./Submarine-linux-amd64.deb` |
| Fedora / RHEL / openSUSE | `Submarine-linux-x86_64.rpm` — `sudo dnf install ./Submarine-linux-x86_64.rpm` |
| Arch / Manjaro / EndeavourOS | `Submarine-linux-x86_64.pkg.tar.zst` — `sudo pacman -U Submarine-linux-x86_64.pkg.tar.zst` |
| Any Linux | `Submarine-linux-x86_64.AppImage` — `chmod +x` and double-click |

To update, run the newer setup `.exe` over the installed one. Submarine tells you when there is one: the profile screen shows *"New version v0.4.0-fork.3 available"*. Each release has the original project's version and the fork's count, `v0.4.0-fork.1`, `v0.4.0-fork.2` and so on, in the title bar and at the bottom of the profile screen. To install on many PCs, the setup `.exe` runs silently with `/S`.

Every release is on the [releases page](https://github.com/paulm981/Submarine/releases). The Android `.apk` comes from the [original project's releases](https://github.com/sinaxhpm/submarine/releases).

> Builds are currently **unsigned**. Windows SmartScreen will prompt — click "More info → Run anyway". Android sideloading needs "Install unknown apps" enabled for the installer source.

### First open on macOS

The Mac app is signed only by the build itself, not notarized by Apple, so the first time you open a downloaded copy macOS says *"Apple could not verify "Submarine.app" is free of malware…"* and offers only **Done**. To open it anyway:

1. Move **Submarine** into **Applications** (drag it out of the `.dmg`, or out of the folder the `.app.zip` unpacked to).
2. Open it once and click **Done** on that message.
3. Open **System Settings → Privacy & Security**, scroll down to *"Submarine.app was blocked to protect your Mac"*, click **Open Anyway** and enter your password. Then click **Open Anyway** once more when macOS asks.

Or, in Terminal, clear the download mark and open it normally:

```bash
xattr -dr com.apple.quarantine /Applications/Submarine.app
```

If macOS says the app *is damaged*, sign it again on your Mac and open it: `codesign --force --deep --sign - /Applications/Submarine.app`. Each new download needs this once.

### Portable mode (Windows)

Submarine can keep all of its data next to the executable instead of in your user profile — on a USB stick, say. Create an empty folder named `submarine-data` beside `submarine.exe` and start the app; the `-portable.zip` download is already laid out that way. Profiles, the cloud sign-in, the window position and the UI preferences then live in `submarine-data`, and **Settings → Maintenance** shows the folder in use.

A first portable launch starts with no profiles. To carry existing data over, close Submarine and copy two things into `submarine-data`:

- the contents of `%APPDATA%\com.submarine.app`, except `sync_device.json` — it identifies each install to sync, and the portable copy creates its own;
- the folder `%LOCALAPPDATA%\com.submarine.app\EBWebView`. It holds the app's settings, including **Auto-sync**. Without it Auto-sync starts out on, and a profile set up to sync (to your own storage, or to the cloud when that's on) syncs as soon as you open it — so if you skip this folder, turn Auto-sync off in Settings before opening one you mean to keep only on this computer.

`submarine-data` then holds your encrypted profiles and your cloud sign-in token, so keep it somewhere only you can read.

Not portable: the Microsoft Edge WebView2 runtime (a Windows system component, preinstalled on Windows 11 and current Windows 10) and the short-lived temp files used while editing or dragging remote files. If `submarine-data` can't be written to, Submarine falls back to the usual per-user folders and says so in Settings. (A plain, non-packaged Linux build works the same way — it just doesn't ship a prebuilt zip.)

### Android

Submarine on Android is a true native build of the same Rust core — same SSH stack, same encrypted vault, same profile sync. Reach a server from your phone with the same credentials you saved on your desktop.

- **Same encrypted vault.** Cloud sync drops your profiles onto the phone exactly as they were on desktop. Nothing re-typed.
- **Tabbed terminal optimised for touch.** Mobile soft keyboards don't have Ctrl / Alt / Shift / Esc / Tab — Submarine ships an inline **soft-key bar** between xterm and the OS keyboard with Termux-style sticky modifiers (tap to arm, double-tap to lock). Ctrl+letter, Alt+letter, and Shift+Tab all work as expected.
- **Auto-scroll on focus.** Tap the terminal and the prompt scrolls into view; the same fires when the OS keyboard opens so the cursor row never sits hidden behind the keyboard.
- **SFTP file browser** with multi-select, upload from the phone's storage, download into Downloads.
- **Port forwarding (SOCKS / local / remote)** runs as long as the app is open — handy for tunnelling a mobile browser through your home box.
- **Touch-friendly Info / Docker tabs.** Same server inspection and container manager as desktop, with ≥32 px touch targets and full-bleed modals.
- **Folder mirror is desktop-only for now** — Android's filesystem permissions don't map cleanly onto our watcher model.

Tested on Android 8.0+ (API 26+). Phones, tablets, and Android-on-ChromeOS.

## Security

Your data is encrypted on your machine before anything leaves it. Submarine and the sync server never see your passwords, private keys, or profile content.

**How it works:**

1. **Your master password becomes a vault key — on your device.** When you unlock, your password is run through Argon2id to derive a key. The password is wiped from memory the moment derivation finishes. It never goes anywhere.
2. **Profiles are sealed with AES-256-GCM — on your device.** Every saved server (credentials, keys, tunnels, notes, mirrors) is compressed and encrypted with that key. What hits disk is opaque ciphertext.
3. **Sync uploads that same ciphertext — byte for byte.** When you turn cloud sync on, the bytes uploaded are exactly the encrypted blob from your disk. The server stores ciphertext plus a random nonce. Nothing else.

**Why the sync server can't read your profiles:**

- No registration with a server-side password. No recovery email. No "forgot password" flow. **There is nothing on the server that could be used to derive your key.**
- Encryption happens before the upload call. By the time bytes hit the network they are already ciphertext with no key material attached.
- Even with a full server breach, an attacker gets the same opaque blob they'd see if you uploaded a random file to S3. Same threat model.

**Other defences:**

- Master key kept in zeroized memory, wiped on lock
- TOFU host keys with per-connection nonce binding (resists prompt-race attacks)
- Strict Content Security Policy, minimal Tauri permission ACL, no shell or filesystem plugins exposed to the UI

## Code signing policy

Windows installers will be signed. Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

Until the first signed release, the installers aren't signed, and [Install](#install) says how to open them.

**Team roles:**

- Committers and reviewers: [paulm981](https://github.com/paulm981)
- Approvers: [paulm981](https://github.com/paulm981)

Every release is built by GitHub Actions from this repository's `main` branch, and every change reaches `main` through a pull request whose checks have passed.

**Privacy policy:** This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

What Submarine connects to:

- the servers you connect to;
- the folder or git repository you sync profiles through, if you set one up;
- the original project's hosted sync, only if you turn it on.

On its own, it makes one call: when the profile screen opens, it asks GitHub's public API whether there's a newer release. That call carries the app's version, and nothing about you or your servers.

## FAQ

### Is Submarine free?

Yes. MIT-licensed. Use it personally, use it at work, fork it, redistribute it.

### Does it work offline?

Yes. Cloud sync is opt-in. All profiles, mirrors, and tunnels live locally and are encrypted at rest.

### I use PuTTY, WinSCP or MobaXterm today. What changes?

PuTTY and WinSCP are two separate apps you alt-tab between; Submarine puts terminal, SFTP, tunnels and folder mirror in one tabbed window per server. Coming from MobaXterm, import your sessions with their folders, saved logins and jump hosts, and keep working the way you do, on Windows, macOS or Linux, with one encrypted profile that syncs between them.

### Does it support keys with passphrases?

Yes. File-based keys and pasted PEM/OpenSSH keys, with or without passphrase.

### Can I sync profiles between Windows and macOS?

Yes. The encrypted vault is platform-portable. Enable sync on one machine, unlock on another with the same master password.

### Why Rust and Tauri instead of Electron?

Smaller installer (around 10 MB vs ~100 MB for an Electron equivalent), lower RAM, faster startup, native window controls, and no Chromium per app.

### Where are my profiles stored?

In a single encrypted file under your OS app-data directory — or in the `submarine-data` folder when you run in [portable mode](#portable-mode-windows). Nothing in plaintext. Nothing in a global Keychain or registry hive.

### Something went wrong — how do I find out what?

Settings → **Troubleshooting**. Submarine keeps a log on your computer (`submarine.log` in its log folder, at most 4 MB): connection steps, errors, crashes, and when the window froze, for how long and which terminal was receiving the most output then. **Copy diagnostics** puts the build, the system and the end of the log on the clipboard with server, folder and user names, host names and addresses replaced by placeholders, ready to paste to whoever is helping. **Open log folder** shows the files themselves.

### Does Submarine collect telemetry or analytics?

No. There's no analytics SDK, and no crash reporter that sends data home. The log under Troubleshooting stays on your computer unless you copy it yourself. Beyond your servers and the sync storage you set up, the app makes one call by itself: a check on GitHub for a newer release (see [Code signing policy](#code-signing-policy)). The original project's hosted sync is off unless you turn it on, and it only ever receives opaque ciphertext.

### Can I use Submarine on my Android phone with the same servers as my desktop?

Yes. Install the APK, unlock with the same master password you use on desktop, and the encrypted vault syncs. Every server, key, and saved tunnel appears on the phone. Folder mirror is desktop-only for now; everything else (terminal, SFTP, port forwarding) works on Android.

### Is Submarine on the Play Store?

No. Distribution is via sideloadable APK from the [releases page](https://github.com/sinaxhpm/submarine/releases). This keeps the same untouched binary running on every platform with no store-mandated changes.

## Build from Source

Requirements: Node 20+, Rust 1.90+, [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS. The SSH stack is pure Rust, so no OpenSSL or Perl is needed on any platform.

```bash
git clone https://github.com/paulm981/Submarine
cd Submarine
npm install
npm run tauri dev          # run in development
npm run tauri build        # build release bundle
```

### Builds on GitHub

[`.github/workflows/build.yml`](.github/workflows/build.yml) builds the installers on GitHub's Windows, macOS and Linux runners:

- **Every merge into `main`** is a [release](https://github.com/paulm981/Submarine/releases), tagged with the original project's version and the fork's count: `v0.4.0-fork.1`, `v0.4.0-fork.2` and so on, starting again at 1 when the original project's version changes. The count is one more than the newest tag of that version. If the Windows build fails, no release is made, and the next merge makes it. The workflow passes the version to `src-tauri/build.rs`, which stamps it into the app.
- **Run workflow** on the [Actions tab](https://github.com/paulm981/Submarine/actions/workflows/build.yml) builds a version you type in, as a test, and publishes nothing.

Each run's files are also kept for 90 days under **Artifacts** on the run's page (GitHub sign-in needed there). Android is built only when the repository variable `BUILD_ANDROID` is `true` and the signing secrets the workflow names are set.

A release includes macOS only when the repository variable `BUILD_MACOS` is `true`. A macOS minute uses ten of the account's GitHub Actions minutes, so releases are Windows and Linux unless macOS is asked for. **Run workflow** with **Build macOS too** ticked builds a Mac test copy, kept under the run's **Artifacts**; like any manual run, it publishes nothing. Pull request checks likewise run on Linux and Windows, and also on macOS when the pull request has the `macos` label.

**Building macOS on your own Mac:** install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Xcode's command line tools, Rust, Node 20+), then in a clone of the repository run `npm ci` and `SUBMARINE_VERSION=0.4.0-fork.N npm run tauri build -- --bundles app,dmg` with the release's number. The app and the `.dmg` land in `src-tauri/target/release/bundle/`.

### Android build

Extra requirements: Android SDK + Platform-Tools, NDK 27, and JDK 17. `scripts/android-env.ps1` sets `JAVA_HOME`, `ANDROID_HOME`, and `NDK_HOME` for the current PowerShell session. It finds the SDK through `ANDROID_HOME` or `ANDROID_SDK_ROOT`, or in Android Studio's default folder (`%LOCALAPPDATA%\Android\Sdk`).

```powershell
. .\scripts\android-env.ps1
npm run android:init       # one-time per checkout
npm run android:dev        # build + install on the connected device
npm run android:build      # produce a signed-with-debug-key APK
```

Device must be plugged in over USB with debugging enabled.

`npm run android:dev` runs `scripts/android-dev.ps1`, which:

1. Verifies `adb` is on PATH and at least one device is listed.
2. Applies `adb reverse tcp:1420 tcp:1420` and `tcp:1421 tcp:1421` (Vite dev + HMR) so the WebView reaches the host via USB loopback.
3. Runs `tauri android dev --host 127.0.0.1` so the WebView's `devUrl` bakes in `127.0.0.1`, matching what `adb reverse` exposes.

This path works whether Wi-Fi is on or off — only USB matters. If you still see `failed request for http://127.0.0.1:1420` while debugging, USB has dropped or the reverse mapping was cleared (re-plug, reboot, or `adb kill-server` all clear it). Re-run `npm run android:dev` and it'll re-apply the mapping. To bypass the helper, use `npm run android:dev:raw` (calls `tauri android dev` directly — Tauri then picks the host's LAN IP).

## Tech Stack

**Frontend** — React 18 · TypeScript · Tailwind CSS · xterm.js
**Backend** — Rust · Tauri 2 · russh · rusqlite · aes-gcm · argon2 · zstd

## Credits

Designed and built by [Sina](https://sinaxhpm.com) in collaboration with [Claude](https://claude.com/claude-code). Every line of code in this repo was written together with Anthropic's Claude.

## License

MIT — see [LICENSE](LICENSE).

The app ships with the licences of everything in it: every build lists the JavaScript packages and Rust crates it includes, with their licence texts (`scripts/notices.mjs`), and the fonts' and color schemes' licences are in `public/licenses/`. In the app: Settings › Open-source licenses.

## Also by the author: Echoes — network & developer toolkit

<a href="https://github.com/SinaXhpm/Echoes"><img src="https://raw.githubusercontent.com/SinaXhpm/Echoes/master/Echoes.Android/Resources/mipmap-xhdpi/icon.png" alt="Echoes — free, open-source network and developer toolkit for Windows, macOS, Linux and Android" width="64" align="left" /></a>

**[Echoes](https://github.com/SinaXhpm/Echoes)** is a [free, open-source network and developer toolkit for Windows, macOS, Linux, and Android](https://github.com/SinaXhpm/Echoes): ping, port scanner, SSH, DNS/WHOIS, cURL, IP/GeoIP, uptime monitor, encrypted notes, and string/JSON tools in one app. No accounts, no telemetry, MIT licensed.

<br clear="left" />

- **Network tools** — ping and traceroute, a TCP/UDP port scanner, an SSH terminal with SOCKS5, DNS lookup and WHOIS, IP lookup and GeoIP, and global host checks (ping / HTTP / TCP / DNS from servers around the world)
- **Web & API tools** — a cURL GUI and HTTP client, an HTTP file server for LAN file sharing, a SOCKS5 + HTTP proxy server, a Cloudflare DNS manager, and an SSL/TLS certificate inspector
- **Developer tools** — an uptime and latency monitor, encrypted notes and encrypted backup, a regex tester, a JSON formatter (JSON ↔ YAML / CSV), a hash identifier, a subnet / CIDR calculator, an epoch converter, and UUID and password generators
- **Built with** C# · .NET · Avalonia UI · Native AOT — a single self-contained binary on desktop, nothing to install

[Echoes on GitHub](https://github.com/SinaXhpm/Echoes) · [Download Echoes for Windows, macOS, Linux and Android](https://github.com/SinaXhpm/Echoes/releases) · [More projects by Sina Xhpm](https://sinaxhpm.com)

---

<sub>
<b>Keywords</b>: SSH client, SFTP client, terminal emulator, port forwarding, SOCKS proxy, HTTP proxy, folder mirror, folder sync, remote file edit, encrypted profile sync, zero-knowledge cloud, Argon2id, AES-256-GCM, TOFU host keys, Rust, Tauri, React, xterm.js, Windows SSH client, macOS SSH client, Linux SSH client, Android SSH client, open source SSH, MIT licensed, PuTTY alternative, WinSCP alternative, MobaXterm alternative, Bitvise alternative, Termius alternative, SecureCRT alternative, Royal TSX alternative, ConnectBot alternative, JuiceSSH alternative, rsync GUI, sshfs alternative, Tabby alternative.
</sub>

<sub><i>Bitvise, forever in my heart.</i></sub>
