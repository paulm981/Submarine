# Changelog

This changelog lists the releases of [paulm981/Submarine](https://github.com/paulm981/Submarine), newest first.

- Each release is one merge to main, numbered with the original project's version and the fork's count, such as `v0.3.8-fork.2`. The title bar shows it.
- Before `v0.3.8-fork.1`, merges were numbered builds instead, down to build 236 and the summary below it.
- Sections called **From the original project** list what came in from [SinaXhpm/Submarine](https://github.com/SinaXhpm/submarine), with that project's pull request numbers.
- [FORK.md](FORK.md) explains what the fork does differently, and which of the original project's changes it hasn't taken yet.

## Next release

- **Broadcast takes your tabs into its Wall** ([#73](https://github.com/paulm981/Submarine/issues/73)). The title bar shows the Wall instead of a copy of every server as well. **Back to tabs** on the Wall's bar puts them back where they were, and closing the Wall asks whether to put them back as tabs or close them all. Connections stay up either way.
  - A terminal's ✕ on that Wall gives it its tab back instead of closing it.
- **A dot on a tab with new output** ([#70](https://github.com/paulm981/Submarine/issues/70)). A terminal you aren't looking at shows a dot on its tab, and on its server's tab (or its Wall's tab), when it prints something. The dot goes away when you look at it. Prompt redraws and the first second after you leave a tab don't count.
- **Drag a tab to move it** ([#71](https://github.com/paulm981/Submarine/issues/71)), in the title bar and among a server's terminals. Ctrl+Tab follows the new order. Esc puts it back. Without a mouse, use **Move left** and **Move right** in the tab's right-click menu.
- **Compact tabs** ([#71](https://github.com/paulm981/Submarine/issues/71)): a setting under Settings › UI Customization for flat tabs that fit more across (Home and Wall tabs too), with long names cut short and shown in full on hover.

## v0.3.8-fork.6

- **AI tools, first test** ([#32](https://github.com/paulm981/Submarine/issues/32), steps 1 and 2 of the MVP). Off by default: Settings › AI tools.
  - On, Home's menu lists **Claude Code** under AI tools when it's installed. It opens in a Home tab already connected to Submarine, through `submarine --mcp` over a local channel only: a socket only you can open on macOS and Linux, a named pipe locked to your account on Windows, and a token for each AI tab.
  - **Share with AI** on a server tab's toolbar lets that AI tab ask to run commands there. Each one waits in the **AI panel** (the title bar's AI button) until you click Allow or Deny; the keyboard can't approve anything. A command with sudo, dzdo, su, doas or pkexec is marked as asking for root.
  - Submarine runs it on its own SSH channel, with no terminal, a timeout and a 1 MB output cap. The AI gets the output with known secrets hidden; you see all of it, and how much was hidden.
  - Every request and decision goes in `ai-audit.log` in the log folder.
  - Sharing ends when you stop it, or when the AI tab or the server's tab closes or reconnects.

## v0.3.8-fork.5

- **Getting around many tabs** ([#75](https://github.com/paulm981/Submarine/issues/75)):
  - **Ctrl+Shift+K** opens All tabs with its search ready: type part of a name and press Enter to go there. Up and Down move through the matches.
  - **Ctrl+Tab** and **Ctrl+Shift+Tab** go to the next and previous tab, in the order the title bar shows them.
  - **Show only open** (the eye button in the session tree) hides the servers you don't have open, keeping their folders. In that view, double-clicking a server takes you to its tab, or to the Wall it's on.
- **A failed connect says why** ([#35](https://github.com/paulm981/Submarine/issues/35)). When a connect fails in a way that waiting can fix, Submarine checks whether the name resolves and the port answers, and says what it found under the error. Behind a jump host or a proxy it checks that instead; a ProxyCommand can't be checked. It runs beside the retry countdown, and not for a rejected login or host key.

## v0.3.8-fork.4

- **Two uploads to the same file no longer corrupt it** ([#45](https://github.com/paulm981/Submarine/issues/45)). An upload to a server file that another upload is still writing waits for the first to finish, at 0% until its turn. Cancel still stops it while it waits.
- **Small fixes** ([#53](https://github.com/paulm981/Submarine/issues/53)):
  - A server's OS badge no longer shows the old system after you change its port.
  - The Servers page goes back to the top when the folder you were in is deleted, here or on another computer.
  - **Save & open** on a saved Wall that is already open adds the servers you just added to it.
  - When the window asking about a new host key goes away, the other connections waiting on it still get the full 90 seconds to answer.
  - Duplicating a server in a shared profile no longer lets the copy run on-connect commands or port forwards that were still waiting for your approval on the original. The copy waits for its own approval.

## v0.3.8-fork.3

- **Pause several terminals on a Wall at once** ([#73](https://github.com/paulm981/Submarine/issues/73)). With Broadcast on, Ctrl-click (⌘-click on a Mac) terminal titles to pick several, or Shift-click for a run of them. The bar then offers **Pause** and **Resume** for the ones picked.
  - **Pause all** sits beside **Resume all**, so you can pause everything and resume just the few you want.
- **Close many tabs at once** from a tab's right-click menu ([#74](https://github.com/paulm981/Submarine/issues/74)): **Close other tabs**, **Close tabs to the right**, **Close disconnected tabs** and **Close all tabs**. With Walls open, the last one closes them too. One question first, saying how many are connected.

## v0.3.8-fork.2

- **Every pull request is checked** before it can be merged: the Rust tests on Linux and Windows, clippy (no new warnings), and the TypeScript check and page build. Since every merge is a release, a change that breaks any of them doesn't reach the team.

## v0.3.8-fork.1

- **Releases instead of build numbers.** Every change merged into main is now a release, numbered `v0.3.8-fork.1`, `v0.3.8-fork.2` and so on, shown in the title bar.
  - When the original project's version changes, the count starts again, as in `v0.3.9-fork.1`.
  - The profile screen says when there's a newer release. Builds up to 244 are told about it too.
  - The installers keep the same names in every release, so links to the latest release's files always get the newest.
- **No `.msi` any more.** Windows won't install an MSI over one with the same version, and every release of 0.3.8 has that version. The setup `.exe` installs over any earlier one, and runs silently with `/S` for installing on many PCs.
- **The "Latest build" page** isn't updated any more; it points to the releases.

## Build 244

### From the original project

- **Connecting.** A connect that times out, or a server that can't be reached yet, is tried again every 5 seconds, the first connect too ([#176](https://github.com/SinaXhpm/submarine/pull/176)).
  - The connect screen counts down to the next try, and **Cancel** stops it.
  - A rejected login, or a host key that wasn't accepted, isn't tried again.
  - Behind one of the fork's jump hosts, it tries again when the jump host couldn't be reached, not when logging in to it failed.
- **Phones.** The reconnect banner stays on one line ([#176](https://github.com/SinaXhpm/submarine/pull/176)).
- **Android.** `/` and `-` on the key bar ([#177](https://github.com/SinaXhpm/submarine/pull/177)).
- **README.** A section about Echoes ([#160](https://github.com/SinaXhpm/submarine/pull/160)).

Not taken: [#175](https://github.com/SinaXhpm/submarine/pull/175) types the on-connect commands once the terminal is open. The fork already waits until the shell has printed its login banner, which is later, so it keeps its own way.

## Build 243

- **Title bar.** The Home tab is a house icon, without the word "Home". Hovering over it still says what it is.

## Build 242

- **Servers grid.** The **Add folder** button is back, at the top level and inside a folder. A merge of the original project had dropped it.
- **Monitoring.** It refuses connections a server tries to open back to your computer, as terminals and file transfers already did.
- **SFTP:**
  - File times show seconds, so two saves in the same minute look different.
  - A line under the file list counts the folders and files and adds up their sizes. With a selection, it also shows the selection's size. Only files count towards a size, since a listing doesn't say what a folder holds.

## Build 241

- **Fork releases.** A tag such as `v0.3.8-fork.1` publishes a release with its build number in the title. [FORK.md](FORK.md#keeping-up-with-the-original-project) says how to make one.
- **Update notices** for releases go by build number:
  - Someone on a release is told about a newer release.
  - Someone on a later Latest build isn't told to "update" to an older release.
  - On Windows, a release shows and checks its whole version, `0.3.8-fork.1`, not the installer's `0.3.8`.
- **Pull requests** get a short checklist: a changelog line, and whether the change differs from the original project or could go back to it.

## Build 240

The version is 0.3.8. This build has everything from the original project up to #173, except three changes that aren't taken yet (see the end of this section).

### From the original project

- **Servers without SFTP.** Files work over shell commands on servers that have no SFTP ([#162](https://github.com/SinaXhpm/submarine/pull/162)). In SFTP root mode, such a server works through `sudo`.
- **`~/.ssh/config`:**
  - `Include` and `Match` are read, and host aliases are followed ([#164](https://github.com/SinaXhpm/submarine/pull/164)).
  - Connections go through `ProxyCommand` ([#165](https://github.com/SinaXhpm/submarine/pull/165)).
  - The fork's jump hosts work as before. A jump host that `~/.ssh/config` says to reach through `ProxyCommand` is reached that way, and still logs servers in a few at a time.
- **Connecting.** A connection that is set up quickly no longer misses its own result ([#167](https://github.com/SinaXhpm/submarine/pull/167)).
- **Terminal.** A scrollback setting, 10,000 lines by default ([#171](https://github.com/SinaXhpm/submarine/pull/171)).
- **Window.** A smaller minimum size, so window managers can tile it ([#172](https://github.com/SinaXhpm/submarine/pull/172)).
- **Android.** Export goes through the system picker, and the app stays out of Recents ([#158](https://github.com/SinaXhpm/submarine/pull/158)).

Not taken yet, each to be decided on its own:

- [#169](https://github.com/SinaXhpm/submarine/pull/169) ProxyJump;
- [#170](https://github.com/SinaXhpm/submarine/pull/170) certificates and `known_hosts`;
- [#173](https://github.com/SinaXhpm/submarine/pull/173) RDP servers.

### Fork

- **New documents.** [FORK.md](FORK.md) and this changelog, with pictures of what the fork adds. The README links to both.
- **Upstream notice.** "The original project has changes" now opens GitHub's view of those changes. It used to open the fork's page, where Sync fork would offer to discard the fork's own commits.

## Build 239

- **Docker tab:**
  - It works where only root may use Docker, through `sudo` or `dzdo`, and asks for the password where one is needed.
  - Live logs and container terminals work through root too.
  - Confirmations say what they will do, and "(as root)" when they run as root.
- **Compose.** Compose commands work with the `docker compose` plugin.
- **`sudo` passwords.** Some servers have `NOPASSWD` sudo rules for some commands but not others. There, a command that needs a password now asks for it, and a typed password is used.
- **macOS.** Open log folder works.

## Build 238

- **macOS paste.** Paste reads the clipboard; it used to say "clipboard read denied".
- **macOS typing.** Text fields no longer capitalize or correct what's typed, such as server names.

## Build 237

- **README.** It presents the fork as a cross-platform SSH client and session manager, lists what the fork adds, and has steps for the first open on macOS.

## Build 236

- **Walls:**
  - A line typed to all goes only to the terminals that were taking input when it began.
  - A terminal that starts partway through a line joins at the next one, because it just connected, reconnected or was resumed. It never gets only the end of a command. Its tile says "Next line".
  - Paste and Send follow the same rule.
  - Lines that continue onto the next one count as one line.
- **Terminal.** Floods of escape sequences that kept the window busy for minutes are ignored. They take under a second now.
- **Highlighting.** Rules written as regular expressions are off for now; one slow expression could freeze the window on every computer it synced to. Such rules are kept and listed as off.
- **Monitoring.** Resume all goes on past a server it can't reach.
- **Live Edit.** A remote file named like a Windows device, such as `COM1` or `nul.txt`, is refused.

## Before build 236

This is a summary of fork pull requests #1 to #22, October 10 to 15, 2026, by area.

- **Import.**
  - From MobaXterm, the import keeps these as they were:
    - nested folders;
    - saved logins;
    - jump hosts and their keys;
    - startup commands such as `dzdo su -`;
    - Remote Desktop sessions.
  - You choose the file, and a result screen says what came in.
- **Session tree.**
  - A tree with search beside the terminal.
  - Folders, servers and Remote Desktop links, managed by drag and drop, rename, move and delete.
  - OS badges.
- **Remote Desktop links.** Saved with your servers, and opened in Remote Desktop Connection on Windows.
- **Walls.**
  - Broadcast input: type to all, click into one, pause some.
  - Paste and Send to all.
  - Walls saved by name, and a folder opened as a Wall.
  - **Type to all** to go back after clicking into one terminal.
  - Jump hosts log in a few servers at a time.
- **Monitoring.** Servers behind a jump host can be monitored.
- **Root.** SFTP root mode and Info panel actions work through `sudo` or `dzdo`, without a password where `dzdo` allows only `su -`.
- **Home view.** Terminals on your own computer, with one grouped list of shells on every system and each WSL distribution by name.
- **Terminal.**
  - xterm.js 6, drawn on the GPU.
  - Color schemes, and a scheme per server.
  - Syntax highlighting, with sets per kind of device and per server, and rules of your own.
  - Output paced to what the window has drawn.
  - The app's own dark right-click menus, and no right-click or F5 that reloads the app.
- **Sync.** Profiles sync through a folder or git repository you own. The original project's hosted cloud is off by default.
- **Builds and updates.** A build from every merge to main, with a build number. Update notices point at this fork.
- **Troubleshooting.** A log on your computer, a freeze watchdog, and Copy diagnostics with names replaced.
- **Security, on Windows:**
  - Live Edit opens an editor and never runs the file.
  - The Local pane asks before it runs a program.
  - Downloads are marked as from the internet.
- **Security, everywhere:**
  - File names can't hide behind lookalike characters.
  - A server that shows a different kind of key is a warning.
- **Merges.** The original project merged in up to its #157 ([fork PR #16](https://github.com/paulm981/Submarine/pull/16)).
- **Settings.**
  - Stop asking before closing a connected session.
  - Paste on Windows and Linux reads the clipboard through the app.
