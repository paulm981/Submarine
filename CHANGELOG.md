# Changelog

This changelog lists the builds of [paulm981/Submarine](https://github.com/paulm981/Submarine), newest first.

- Each build is one merge to main, and its number is in the title bar.
- Sections called **From the original project** list what came in from [SinaXhpm/Submarine](https://github.com/SinaXhpm/submarine), with that project's pull request numbers.
- [FORK.md](FORK.md) explains what the fork does differently, and which of the original project's changes it hasn't taken yet.
- A fork release, such as `v0.3.8-fork.1`, gets a heading of its own above the builds it covers.

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
