# About this fork

[paulm981/Submarine](https://github.com/paulm981/Submarine) is a fork of [Submarine](https://github.com/SinaXhpm/submarine) by Sina. It aims to be the SSH client and session manager a team needs for a large, shared set of servers, on Windows, macOS and Linux alike, while staying as close to the original project as it can.

This page covers:

- [What this fork adds](#what-this-fork-adds), with pictures
- [Where it behaves differently](#where-it-behaves-differently) from the original
- [Changes in the original project not taken yet](#changes-in-the-original-project-not-taken-yet), and why
- [How it keeps up with the original project](#keeping-up-with-the-original-project), and how versions and builds are numbered

What changed in each build is in [CHANGELOG.md](CHANGELOG.md).

The fork is based on the original project's **0.3.8**, with every change up to [#173](https://github.com/SinaXhpm/submarine/pull/173) brought in or recorded as not taken yet.

## What this fork adds

### Work on many servers at once

<img src="docs/screenshots/fork/wall-broadcast.png" alt="A Wall of six terminals behind one jump host, Broadcast on, typing to five of six with one paused" width="900" />

**Walls with broadcast input.** Terminals from many servers sit side by side. With Broadcast on, what you type goes to all of them. You can:

- click into one terminal to type to it alone, and use **Type to all** to go back;
- pause the terminals to leave out;
- paste, or **Send** a set of commands, to all of them.

A server that connects or reconnects partway through a line waits for the next line, so it never gets only the end of a command. Walls can be saved by name, and a whole folder opens as one.

- **Jump hosts at scale.** A hundred servers behind one jump host log in a few at a time; the number is a setting. Monitoring reaches servers through their jump host too.
- **Split.** A second shell on the same server opens beside the first, so you can watch a log in one and work in the other.

### Organize them

<img src="docs/screenshots/fork/highlighting-and-tree.png" alt="The session tree beside a terminal: folders, servers with OS badges, a Remote Desktop link, and highlighted log output" width="900" />

- **A session tree** beside the terminal, with search. It holds folders, servers and Remote Desktop links, and you manage it with drag and drop, rename, move and delete.
- **Remote Desktop links** are saved with your servers and open in Remote Desktop Connection on Windows.
- **OS badges** on servers, and a look of their own for Remote Desktop links.

### Bring your sessions with you

**Session import that keeps your setup.** The MobaXterm import keeps everything below as it was:

- nested folders;
- saved logins;
- jump hosts and their keys;
- startup commands such as `dzdo su -`;
- Remote Desktop sessions.

A result screen says what came in. PuTTY and OpenSSH config imports are there too.

### Root when you need it

Root works through `sudo` or `dzdo` (Centrify / Delinea), without a password where the server allows it, including servers that only allow `dzdo su -`. Where a password is needed, the app asks for it. It works in three places:

- **SFTP root mode**: browse and edit files as root;
- **the Info panel**: start, stop and restart services, end processes, and read firewall rules as root;
- **the Docker tab**: where only root may use Docker, containers, logs, live logs and container terminals work through root.

### A better terminal

<table>
  <tr>
    <td width="50%" valign="top" align="center">
      <img src="docs/screenshots/fork/color-schemes.png" alt="Color schemes in Settings with a preview" />
      <br/><sub><b>Color schemes</b>: Campbell, One Half, Solarized, Dracula, Nord, Tokyo Night, Catppuccin, Gruvbox, with a preview. A server can have its own, and it syncs with it.</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <img src="docs/screenshots/fork/highlight-settings.png" alt="Syntax highlighting settings: sets, a preview, and a rule of your own" />
      <br/><sub><b>Syntax highlighting</b>: errors, warnings, OK, addresses, links and dates colored in the output. There are sets for shell scripts, network devices and SQL, a set per server, and rules of your own that sync with your profile.</sub>
    </td>
  </tr>
</table>

<img src="docs/screenshots/fork/home-shells.png" alt="The Home view's shell menu: the login shell marked as default, and the other installed shells" width="600" />

**A Home view** of terminals on your own computer:

- on Windows: PowerShell, Command Prompt, Git Bash, and each WSL distribution by name, with the default one marked;
- on macOS and Linux: your login shell and every other installed shell.

Also:

- **xterm.js 6**, drawn on the GPU.
- **Fast output never freezes the window.** Output is paced by what the screen has drawn, and floods of escape sequences that would lock it up are ignored.
- **The app's own dark right-click menus** everywhere. No right-click or F5 reloads the app.

### Your data, your storage

<img src="docs/screenshots/fork/own-storage-sync.png" alt="Sync through a private git repository: Sync now, auto-sync, check interval, and the computers syncing this profile" width="900" />

**Sync through storage you own.** The storage can be:

- a folder that OneDrive, Dropbox, Nextcloud, Syncthing or a network share keeps the same on your computers;
- a private git repository.

No third-party service is involved. Profiles are encrypted with their password before they leave your computer, and Remote Desktop links sync too. The original project's hosted cloud stays off unless you turn it on.

### Ready to roll out to a team

<img src="docs/screenshots/fork/troubleshooting.png" alt="Troubleshooting in Settings: Copy diagnostics and Open log folder" width="600" />

- **Troubleshooting.** The app keeps a log on your computer, and a watchdog notes when the window froze and why. **Copy diagnostics** copies the log with server, user and host names replaced, ready to send to whoever is helping.
- **A build from every change**, with a build number in the title bar and a "Build N is out" notice when there's a newer one.
- **Hardening.** On every system:
  - characters that disguise a file name are shown plainly;
  - a trusted server that shows a different kind of key is a warning, not a silent change.

  On Windows:
  - Live Edit opens a file in an editor and never runs it;
  - the Local pane asks before it runs a program;
  - downloads are marked as from the internet.

  macOS and Linux get the Live Edit and Local pane protections next.

## Where it behaves differently

These are choices the fork makes on purpose. Each was weighed against staying close to the original project.

| Area | The original project | This fork |
|---|---|---|
| Profile sync | Its hosted cloud, `api.sinaxhpm.com` | A folder or git repository you own. The hosted cloud is off unless you turn it on. |
| Updates | Releases of the original project | Builds of this fork, numbered, from "Latest build" |
| Live Edit (Windows) | Opens the file with its default app | Opens the file in an editor, never runs it |
| Root | `sudo` | `sudo` or `dzdo`, and `dzdo su -`. A server without an sftp-server uses the original project's shell file access, under `sudo` only. |
| Docker | As the user | As the user, or through root where only root may use Docker |
| Remote Desktop | RDP servers ([#173](https://github.com/SinaXhpm/submarine/pull/173), not taken yet) | Remote Desktop links, imported from MobaXterm and synced |
| Paste | The webview's clipboard | Read by the app itself, since the webviews on Windows, macOS and Linux refuse the page |
| Text fields on macOS | Capitalize and correct what's typed | Keep exactly what's typed |
| Closing a connected session | Always asks | Asks, unless you turn that off in Settings |

## Changes in the original project not taken yet

| Change | Why not yet | What's next |
|---|---|---|
| [#169](https://github.com/SinaXhpm/submarine/pull/169) ProxyJump from `~/.ssh/config` | It changes the connect path that the fork's jump hosts are built on: their login queue, and Monitoring through a jump host. | A careful merge that keeps both. |
| [#170](https://github.com/SinaXhpm/submarine/pull/170) Teleport and OpenSSH certificates; trusting `known_hosts` | It builds on #169, and changes how host keys are trusted. | With #169. |
| [#173](https://github.com/SinaXhpm/submarine/pull/173) RDP servers | The fork has its own Remote Desktop links, which import from MobaXterm and sync. | Decide whether to keep one or join the two. |

## Keeping up with the original project

**Seeing what's new.** In Settings › Updates, turn on **Watch the original project**. The profile screen then says when the original project has changes the fork doesn't have yet, and links to GitHub's view of them. Don't use GitHub's **Sync fork** button: the fork has commits of its own, and Sync fork offers to discard them.

**Bringing it in.** A sync pull request, the same way every time:

1. On a branch, merge the original project's main: `git fetch upstream && git merge upstream/main`. Merging, not cherry-picking, keeps the history saying what came from where. It also leaves the original project's changes as they were, so the next merge has less to sort out.
2. Resolve conflicts by keeping both sides. Where the fork does something differently, keep the fork's way and say so in the table above.
3. Anything not taken goes in the table above with the reason, and gets a backlog item. Record it in git so the next merge doesn't bring it in unseen: `git merge -s ours <its commit>`, with a message saying why. That takes no changes, and the update check stops counting it. To take it later, cherry-pick it.
4. Check it:
   - the tests, clippy and the TypeScript check;
   - the app itself, against a test server: terminal, SFTP and SFTP root mode, a Wall behind a jump host, and Monitoring;
   - lines in [CHANGELOG.md](CHANGELOG.md).
5. Merge it. GitHub builds it as the next "Latest build".

**Versions and build numbers.**

- **The version** is the original project's (0.3.8 now). The fork doesn't change it, so it always says which of the original project's releases a build is based on.
- **The build number** is the fork's own. It goes up by one with every merge to main, and the title bar shows it, as in `v0.3.8 · build 240`. A higher build is newer.
- When the original project releases 0.3.9 and it's merged, the version becomes 0.3.9, and the build numbers keep rising.

**Giving back.** Fixes that apply to the original project too go back to it as pull requests.
