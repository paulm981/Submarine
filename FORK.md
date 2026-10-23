# About this fork

[paulm981/Submarine](https://github.com/paulm981/Submarine) is a fork of [Submarine](https://github.com/SinaXhpm/submarine) by Sina. It aims to be the SSH client and session manager a team needs for a large, shared set of servers, on Windows, macOS and Linux alike, while staying as close to the original project as it can.

This page covers:

- [What this fork adds](#what-this-fork-adds), with pictures
- [Where it behaves differently](#where-it-behaves-differently) from the original
- [Changes in the original project not taken yet](#changes-in-the-original-project-not-taken-yet), and why
- [How it keeps up with the original project](#keeping-up-with-the-original-project), and how releases are numbered

What changed in each release is in [CHANGELOG.md](CHANGELOG.md).

The fork is based on the original project's **0.4.0**, with every change up to [#229](https://github.com/SinaXhpm/submarine/pull/229) brought in.

## What this fork adds

### Work on many servers at once

<img src="docs/screenshots/fork/wall-broadcast.png" alt="A Wall of six terminals behind one jump host, Broadcast on, typing to five of six with one paused" width="900" />

**Walls with broadcast input.** Terminals from many servers sit side by side. With Broadcast on, what you type goes to all of them. You can:

- click into one terminal to type to it alone, and use **Type to all** to go back;
- pause the terminals to leave out: one with its chip, several by Ctrl-clicking (⌘-clicking) their titles or Shift-clicking a run, or **Pause all** and then resume the few you want;
- paste, or **Send** a set of commands, to all of them.

A server that connects or reconnects partway through a line waits for the next line, so it never gets only the end of a command. Walls can be saved by name, and a whole folder opens as one.

Closing a Wall that took your tabs asks whether to put them back or close them. Tick **Remember my answer**, or choose in **Settings › Walls**: **Ask each time**, **Put them back as tabs** or **Close them all**. The choice stays on this computer. **Close them all** still asks first when one of those servers is busy: a file uploading or downloading, an archive being made or unpacked on the server, a mirror running, a Live Edit file with changes not sent yet, or an AI command running or waiting for you.

- **Jump hosts at scale.** A hundred servers behind one jump host log in a few at a time, in terminals and in Monitoring; the number is a setting. Monitoring reaches servers through their jump host too, by the same `~/.ssh/config` name as the terminal.
- **Split.** A second shell on the same server opens beside the first, so you can watch a log in one and work in the other.
- **Many tabs.** **Ctrl+Shift+K** opens All tabs with its search ready: type part of a name and press Enter. **Ctrl+Tab** and **Ctrl+Shift+Tab** move between tabs. A tab's right-click menu closes the other tabs, the ones to its right, the disconnected ones, or all of them.

### Organize them

<img src="docs/screenshots/fork/highlighting-and-tree.png" alt="The session tree beside a terminal: folders, servers with OS badges, a Remote Desktop link, and highlighted log output" width="900" />

- **A session tree** beside the terminal, with search. It holds folders, servers and Remote Desktop links, and you manage it with drag and drop, rename, move and delete. **Show only open** (the eye button) narrows it to the servers you have open.
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

### Jump hosts and your SSH config

With **Use settings from ~/.ssh/config** on (**Settings › Authentication**), servers connect the way `ssh` would:

- **ProxyJump.** A server behind a chain of jump hosts connects, as with `ssh -J`. Each jump host waits its turn to log in (**Logins at a time per jump host**, in **Settings › Servers**), so a hundred servers don't all log in to it at once. A server's own saved jump host or proxy still comes first.
- **Key files and certificates.** The IdentityFile keys the config names are tried, each with its certificate (CertificateFile, or `<key>-cert.pub`), so Teleport nodes connect. An expired certificate is skipped, and the log says so.
- **known_hosts.** A host key listed there skips the first-time question, and is saved to Submarine's own trusted keys. Those always come first: a key that changed gets the warning, even when known_hosts lists the new one. A key marked `@revoked` is always refused.
- **Host certificates.** A server's certificate is trusted through an `@cert-authority` line in known_hosts, checked as OpenSSH checks it. If the key in it isn't the one Submarine trusts for the server, as after a server is reinstalled, you get the changed-key warning and decide. A certificate that doesn't check out is refused.

Nothing is written to `~/.ssh`. With the setting off, none of these files is read.

**Jump via** in a server's form is a search box: type part of a name, address or folder, then pick with the arrow keys and Enter. Servers already used as a jump host come first.

The details, and where this differs from the original project, are in the [table below](#where-it-behaves-differently).

### AI help, without sharing keys

A first test, off by default. To try it, install Claude Code, turn on **Settings › AI tools**, then start Claude Code from Home's menu (the arrow beside **+**, then **AI tools**).

- **Claude Code from Home's menu** opens already connected to Submarine through a local channel only your account can use. Nothing listens on the network.
- **Share with AI** on a server's tab lets that AI tab ask to run commands there. Unless you've said otherwise (below), each one waits in the **AI panel** (the title bar's AI button) until you click Allow or Deny; typing can't approve anything. Its card says which server it runs on and as whom ("on **web-01** as **root**"). A tab logged in as root, or a command with sudo, dzdo, su, doas or pkexec, is marked as root. Where those need no password, nothing asks first, and Submarine can't always tell. **Deny with a note** tells the AI why, so it can try another way. When a command waits for you and Submarine isn't in front, its taskbar button flashes (you can turn that off).
- **How much it may run without asking is up to you, per server.** **Ask every time** is the default. **Run my allowed commands, ask for the rest** runs the commands on your allow list straight away. **Run anything, just log it** runs every command, sudo included where the server doesn't ask for a password. Set it on a server or a folder from the session tree's right-click menu (**AI approval…**), or for every server in **Settings › AI tools**; a server's own setting wins over its folder's, and a folder's over the default.
  - **Allow rules** match one whole, simple command: `uptime` exactly, `systemctl status *` with any arguments, `cat /var/log/*` with the rest of that one word (no second file, no `..`). A command with `;`, `&&`, `|`, `$(`, a backtick, a redirect, a new line, a hidden or full-width character never matches, nor `%` or `^` on a Windows server. sudo, su, doas, dzdo, pkexec, sudoedit and run0 always ask unless the server runs anything, however they're written. **Always allow this command here** on a card adds one; a pattern for a command that runs other commands (`timeout *`, `bash *`, `find *`) says so by name and takes one more click.
  - **The always-ask list** beats every allow rule and **Run anything**. It finds its words anywhere in a command, read twice: plainly, and with what can hide a word taken out (quotes, `$@`, `${…}`, empty `$()`, brace groups, cmd.exe's `^` and `%…%`, PowerShell's backticks, a path or `.exe`, globs). `rm -rf *` catches its options in any order, split or long. A command that builds the name as it runs can still get past it. It starts with `reboot`, `shutdown`, `rm -rf *`, `dd *`, `format`, `Restart-Computer` and a few more.
  - **Allow everything here until this AI tab closes**, on a card, runs the rest on that server for that AI tab, but for the always-ask list, until the AI tab closes, the server stops being shared or its tab reconnects, AI tools are turned off, the profile is locked, you end it (the AI pane and panel have a button), or you set a level that reaches the server.
  - **You can always see it.** A shared server's tab and the AI pane say "allowed commands run" or "runs without asking", with a click to change it; commands that ran without asking are marked so in the AI panel; the audit log records how each was allowed. At most four commands from one AI tab run at once on a server.
  - **Levels and rules stay on this computer.** They never sync, so a shared profile can't change them, and the AI can't either. They don't travel in a profile file: opened by another install, a vault keeps only its always-ask list.
  - **A teammate's move is held.** A server moved by someone else, or with a folder above it moved, somewhere the AI may do more here waits in **Waiting for your OK**; until then every command on it asks. **Keep as it was** keeps it at **Ask every time** here.
- **One question per command.** Claude Code starts with Submarine's tools allowed, so only Submarine asks before a server command. Claude Code still asks before anything it does on your computer.
- **The AI side pane** shows an AI tab beside a server tab or Wall shared with it: the same terminal as in Home, open or folded to a bar (**Ctrl+Shift+L**), with Share, Unshare, Allow and Deny in its header. The keyboard goes into it only when you click in its terminal; opening or folding it leaves the keyboard where it was. The terminal with the keyboard is bright, and the other dims. A server tab's **+** menu (right-click the **+**) has **Claude Code beside this**, which starts one, shares the tab with it and shows it there.
- **Commands run on Submarine's own connection**, so the AI never gets a key or password. Each one runs without a terminal, with a time limit and at most 1 MB of output. You see all of it; the AI gets it with known secrets hidden, and you see how many.
- **Every request and decision goes in `ai-audit.log`** in the log folder. A command you allow is logged at once, and again when it ends.
- **One program per AI tab.** A second program that tries to connect as the same AI tab is refused, and the AI panel warns you.
- **Access ends** when you unshare, close the AI tab or the server's tab, the server's tab reconnects, or Submarine quits. Anything waiting is cancelled, and anything running is stopped on the server too. Locking the profile or turning AI tools off also clears the AI panel's list of what ran.

AI tabs are kept apart from each other, from other accounts on the computer, and from the network. They aren't kept apart from a hostile program running as you: it could read an AI tab's access, edit the log, or click Allow for you.

Claude Code starts in a folder of its own (`ai-workspace` in Submarine's data folder), not your home folder, where it would read files without asking. Its `SSH_AUTH_SOCK` points where no agent is (on Windows too, where Windows OpenSSH would otherwise go to its own agent), and the usual cloud and GitHub logins are taken out of its environment.

It still runs as you, though. `ssh` run by it can still log in with an agent named by `IdentityAgent` in `~/.ssh/config` (1Password, Secretive, Bitwarden, Pageant), a connection your own terminal keeps open (`ControlMaster`), or a key file with no passphrase or with its passphrase in the macOS Keychain. What it runs can also look for an agent itself, read your files and use the network. To keep Claude Code out of your keys, deny reading them in `~/.claude/settings.json`, both for its Read tool and in its sandbox, which by default can read them:

```json
{
  "permissions": { "deny": ["Read(~/.ssh/**)", "Read(~/.aws/**)", "Read(~/.config/gcloud/**)", "Read(~/.azure/**)"] },
  "sandbox": {
    "enabled": true,
    "credentials": { "files": [
      { "path": "~/.ssh", "mode": "deny" }, { "path": "~/.aws", "mode": "deny" },
      { "path": "~/.config/gcloud", "mode": "deny" }, { "path": "~/.azure", "mode": "deny" }
    ] }
  }
}
```

The sandbox runs on macOS, Linux and WSL2, not on Windows itself: for production servers on Windows, run Submarine and Claude Code in WSL2 or a virtual machine.

What comes next is in [#32](https://github.com/paulm981/Submarine/issues/32): first, any AI tool from a list, not only Claude Code ([#117](https://github.com/paulm981/Submarine/issues/117)).

### Root when you need it

Root works through `sudo` or `dzdo` (Centrify / Delinea), without a password where the server allows it, including servers that only allow `dzdo su -` or `sudo su -`, with or without your password. Any login shell works, csh and tcsh included.

Where a password is needed, the app asks for it once per tab:

- **It goes only to sudo or dzdo,** checked on its own before anything runs, whatever prompt your server shows. A command that runs without a password never gets it.
- **Each password you type is tried once.** A wrong one costs one failed attempt, even when several actions were waiting for it, so it can't lock your account. One that stops working is forgotten for the whole tab and asked for again.

It works in three places:

- **SFTP root mode**: browse and edit files as root;
- **the Info panel**: start, stop and restart services, end processes, and read firewall rules as root;
- **the Docker tab**: where only root may use Docker, containers, logs, live logs and container terminals work through root.

### Files

- **Live Edit keeps your unsent changes.** If a copy on this computer has changes that never reached the server, it's kept in **Submarine unsent edits** in your Downloads folder when you open the file again, disconnect, lock the profile or quit. The window says so. On macOS and Linux, only you can read that folder.
- **Live Edit opens a file in an editor and never runs it:** the one picked in **Settings › Live Edit**, or else Notepad on Windows, the default text editor on macOS, and on Linux the text editor set for plain text.
- **Downloads never replace each other.** If a file of the same name appears while a download runs, you're asked before it's replaced. In a folder download it's left as it was, and the card says how many. On Windows and macOS, which treat `Readme.md` and `README.md` as one name, the second of two such files is left out too, rather than replacing the first.
- **Two uploads to the same file**, or an upload and a Live Edit save, wait their turn instead of mixing and corrupting it.
- **A mirror keeps a file's permissions on the server**, with its owner and group where the server allows it, so a script keeps its permission to run. A new file gets the permissions of the one on your computer. A mirror never puts a server's file in a folder that runs programs at login, such as the Startup folder on Windows or `~/.config/autostart` on Linux.

**What may run, on each system.** A file from a server is never run by opening it:

- a program on a server (`.exe`, `.msi`, `.jar`, `.AppImage`, …) isn't opened from the file pane at all, as in the original project ([#212](https://github.com/SinaXhpm/submarine/pull/212));
- the Local pane asks before it opens a file that would run: a program, a script or a shortcut; on macOS a `.terminal` or `.command` file too; on macOS and Linux, any file marked as executable;
- downloads are marked as from the internet on Windows and macOS, and so is what the Local pane unpacks from an archive marked that way, so the system asks before a program from them runs.

### Shared profiles

A profile shared through Submarine Cloud (the original project's hosted sync, when you turn it on) is edited by other people too. A change of theirs that would make the app act on its own here waits in **Waiting for your OK**, above the server list, until you allow it:

- on-connect commands, which would be typed into your terminal;
- autostart, which would connect when you unlock the profile;
- port forwards, new ones or ones you had stopped;
- Monitoring's own commands, run on the server at every check;
- a switch to telnet or VNC, which would send the server's saved password where it could be read. Until you choose, the server stays SSH (or RDP) here, on its old port. **Allow** makes the switch here too. **Keep it SSH** (or **Keep it RDP**) changes it back on the other computers as well.

Allowing counts for this computer only. Allowing on-connect commands counts only for the commands you were shown: if they change while you look, you're shown the new ones. A copy of a server keeps waiting where the original was.

Two more things wait for you:

- **Deleted by someone else.** When one sync would delete much of the profile at once, or any SSH key, the items stay until you choose **Delete here too** or **Keep them**.
- **Use the new key?** When the profile's key is replaced, usually because its owner removed someone, compare its fingerprint with the one in the owner's Profile panel before you use it.

### A better terminal

<table>
  <tr>
    <td width="50%" valign="top" align="center">
      <img src="docs/screenshots/fork/color-schemes.png" alt="Color schemes in Settings with a preview" />
      <br/><sub><b>Color schemes</b>: Campbell, One Half, Solarized, Dracula, Nord, Tokyo Night, Catppuccin, Gruvbox, with a preview, or one of your own from Windows Terminal. A server can have its own, and it syncs with it.</sub>
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
- **A failed connect says why:** under the error, whether the name resolved and to what, and whether the port answered, through the jump host or proxy when there is one.
- **A release from every change**, numbered like `v0.4.0-fork.8` and shown in the title bar, with a notice when there's a newer one.
- **Hardening.** On every system, characters that disguise a file name are shown plainly, and a trusted server that shows a different kind of key is a warning, not a silent change. What a file from a server may run is under [Files](#files).

## Where it behaves differently

These are choices the fork makes on purpose. Each was weighed against staying close to the original project.

| Area | The original project | This fork |
|---|---|---|
| Profile sync | Its hosted cloud, `submarine.sinaxhpm.com` | A folder or git repository you own. The hosted cloud is off unless you turn it on. |
| Updates | Releases of the original project | Releases of this fork, one for every change: `v0.4.0-fork.1`, `v0.4.0-fork.2`, … |
| Live Edit | Refuses a program; on Windows opens a script in Notepad; opens anything else with its default app ([#212](https://github.com/SinaXhpm/submarine/pull/212)) | Refuses a program, and opens anything else in an editor: the one picked in Settings, or Notepad on Windows, the default text editor on macOS, and on Linux the text editor set for plain text. It never runs a file |
| Root | `sudo` | `sudo` or `dzdo`, and `dzdo su -`. A server without an sftp-server uses the original project's shell file access, under `sudo` only. |
| Docker | As the user | As the user, or through root where only root may use Docker |
| Remote Desktop | RDP servers ([#173](https://github.com/SinaXhpm/submarine/pull/173)), opened in the system's RDP app or, experimentally, in the tab ([#199](https://github.com/SinaXhpm/submarine/pull/199)) | The same RDP servers, each with its **Open in** choice of the built-in client or the system's app, per device. Beside them, the fork's Remote Desktop links, imported from MobaXterm and synced, and marked LINK so they aren't taken for RDP servers. Joining Remote Desktop links and RDP servers is next ([#42](https://github.com/paulm981/Submarine/issues/42)) |
| VNC password | The server's own, or its saved login's | The server's own only. VNC sends its password with weak protection and doesn't check the server's identity, so a saved login, shared with other servers, is never sent there |
| A server switched to telnet or VNC by another computer | Switched at once | In a shared profile, held on this computer, like synced on-connect commands: it stays what it was (SSH, or RDP) on its old port here until you allow the switch, or keep it as it was, which changes it back everywhere. Telnet and VNC would send its saved password where it could be read. A held switch stays held if you leave the share |
| RDP, telnet and VNC servers on Walls | Not pinned to a Wall | Not on a Wall either, even when their folder or a saved Wall is opened as one, so a screen never gets Broadcast keys |
| Paste | The webview's clipboard | Read by the app itself, since the webviews on Windows, macOS and Linux refuse the page |
| Text fields on macOS | Capitalize and correct what's typed | Keep exactly what's typed |
| Closing a connected session | Always asks | Asks, unless you turn that off in Settings |
| On-connect commands | Typed once the terminal is open, on the first connect ([#175](https://github.com/SinaXhpm/submarine/pull/175)) | Typed once the shell has printed its login banner, on the first connect and after a reconnect |
| Walls | One Wall of pinned copies of terminals; a copy sets the terminal's size while it's open ([#180](https://github.com/SinaXhpm/submarine/pull/180)) | Walls of their own, each showing the terminals themselves, so a terminal has one size. Broadcast, saved Walls, and Rows ([#190](https://github.com/SinaXhpm/submarine/pull/190)), which aren't saved with a saved Wall yet |
| Saved password at a prompt ([#182](https://github.com/SinaXhpm/submarine/pull/182)) | Offers this server's password and every saved login | Offers this server's password and its own login; another saved login only when the prompt names its user. Not in Home's local shells |
| Terminal colors ([#183](https://github.com/SinaXhpm/submarine/pull/183)) | One scheme per device, or one of your own | The fork's schemes, per device or per server (synced). A scheme of your own is for this device only. "Keep text readable" is on by default |
| Key exchange ([#189](https://github.com/SinaXhpm/submarine/pull/189)) | Offers the 1024-bit `diffie-hellman-group1-sha1` last | Never offers it (Logjam). Group exchange over SHA-1 is offered, with groups of 2048 bits or more |
| Privacy policy ([#225](https://github.com/SinaXhpm/submarine/pull/225)) | The **Privacy** links open the policy on its website | They open this fork's [PRIVACY.md](PRIVACY.md): the same policy, with a note on where the fork differs. **Delete Submarine Cloud account** only deletes the hosted cloud account; profiles on the computer and sync through your own storage stay |
| Logins to a jump host ([#169](https://github.com/SinaXhpm/submarine/pull/169)) | No queue: every server opened behind a jump host logs in to it at once | Each jump host on the way waits its turn (**Settings › Logins at a time per jump host**), one line per jump host address, shared by a saved jump host and the same machine in a chain; a jump host reached through another has its own line for that way there. A jump host holds its turn only while it logs in ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| A saved jump host and `~/.ssh/config` ([#169](https://github.com/SinaXhpm/submarine/pull/169)) | A server's own jump host or proxy wins over its config's ProxyJump; a saved jump host's own ProxyJump is followed | The same. A saved jump host's own saved jump host still isn't followed, so chains only ever come from this computer's `~/.ssh/config`, never from synced servers ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Jump hosts in one route ([#169](https://github.com/SinaXhpm/submarine/pull/169)) | At most 8 levels of ProxyJump lists, so lists inside lists can add up to many more jump hosts | At most 8 jump hosts in all, counting every list and a saved jump host ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| `%` in a ProxyJump | Taken as written | Refused with a message: Submarine doesn't fill in `%h`, `%r` and the like there yet ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Monitoring and ProxyJump | A server behind a config ProxyJump isn't checked; monitoring has no jump hosts | Monitoring goes through saved jump hosts. A server or a jump host that `~/.ssh/config` reaches through a ProxyJump isn't checked, and the card says why: monitoring can't ask for the jump hosts' passwords ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Host keys in `~/.ssh/known_hosts` ([#170](https://github.com/SinaXhpm/submarine/pull/170)) | A key listed there is trusted, before the app's own trusted keys are looked at | The app's own trusted keys (in your vault) come first. A key listed in known_hosts only skips the first-time question: a key that changed still gets the warning, even if known_hosts lists the new one. If known_hosts lists a different key for the server, you get the changed-key warning. A host certificate whose key isn't the one the app trusts gets the same changed-key warning; a certificate that doesn't check out is refused. A key marked `@revoked` is always refused, by Monitoring too ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Keys known_hosts vouches for ([#170](https://github.com/SinaXhpm/submarine/pull/170)) | Trusted each time, never saved | Saved to the app's trusted keys the first time, as if you had approved it, so Monitoring can check the server and a later change gets the warning. A host certificate isn't saved unless you accept its key at the changed-key warning; Monitoring says it can't check a server only a certificate vouches for, until its key is saved ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Key files and certificates from `~/.ssh/config` ([#170](https://github.com/SinaXhpm/submarine/pull/170)) | IdentityFile and CertificateFile are offered, certificates whatever their dates | The same, with "Use settings from ~/.ssh/config" on. A certificate that has expired or isn't valid yet is skipped, and the log says so. As with ssh-agent keys, they're offered to every server the setting applies to, including servers from a shared profile. A key file goes through the same cost limit as a saved key ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Turning known_hosts on and off ([#170](https://github.com/SinaXhpm/submarine/pull/170)) | Follows "Use settings from ~/.ssh/config" | The same, with no separate switch: off, known_hosts and the config's key files aren't read at all ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| `HostKeyAlgorithms` in `~/.ssh/config` ([#170](https://github.com/SinaXhpm/submarine/pull/170)) | Its list decides which host keys are asked for first | Within its list, the kinds of key the app already trusts for the server are still asked for first, so a server with several keys doesn't look changed ([#43](https://github.com/paulm981/Submarine/issues/43)) |
| Website ([#229](https://github.com/SinaXhpm/submarine/pull/229)) | `website/` is published at `submarine.sinaxhpm.com` | The folder comes along with merges, unchanged, but the fork doesn't publish it: no workflow builds or deploys it |

## Changes in the original project not taken yet

None: every change in the original project up to [#229](https://github.com/SinaXhpm/submarine/pull/229) is taken. Where the fork takes one differently, the table above says so. When a change isn't taken, it's listed here with the reason.

## Keeping up with the original project

**Seeing what's new.** In Settings › Updates, turn on **Watch the original project**. The profile screen then says when the original project has changes the fork doesn't have yet, and links to GitHub's view of them. Nothing checks on a schedule: when the maintainer wants a rundown, with a recommendation for each change, they ask for one, and nothing is merged until they say so. Don't use GitHub's **Sync fork** button: the fork has commits of its own, and Sync fork offers to discard them.

**When.** At the start of every batch of work, before anything else, so the fork never falls far behind. A small sync each time is easier than a big one later.

**Bringing it in.** A sync pull request, the same way every time:

1. **Look before merging.** `git fetch upstream`, then `git merge-tree --write-tree HEAD upstream/main` shows which files would conflict without touching anything. `git log HEAD..upstream/main` lists what's new.
2. **Decide first when a change collides with the fork.** If one of the original project's changes touches something the fork does its own way (security above all), write the choices down in an issue and settle them before writing code, as [#43](https://github.com/paulm981/Submarine/issues/43) did for ProxyJump and known_hosts (D1 to D8). Then bring such changes in one pull request each, not all at once.
3. **Merge, don't cherry-pick:** `git merge upstream/main` on a branch. That keeps the history saying what came from where, and leaves the original project's changes as they were, so the next merge has less to sort out. Record a change you leave out with `git merge -s ours <its commit>` and a message saying why. Take it later with `git cherry-pick -x <its commit>`, so the commit says where it came from.
4. **Resolve conflicts toward the original project's text** as it is on its main now. Then put the fork's behavior back the way that changes their code least:
   - **Fork rules live in fork-only files,** called from one-line hooks in the shared code: `hop_policy.rs` (jump hosts), `trust_policy.rs` (host keys), `remote_sh.rs` (root), `busy.rs` (what's still running). A conflict then touches one line, not the rule.
   - **Code waiting on the original project** gets a comment, `// fork: until upstream #NNN`, so it's easy to find and drop once they take the fix.
   - **A safety fix the merge itself needs** goes in the first commit, so no commit in the history is unsafe.
5. **Say what's different.** Anything taken differently goes in the [table above](#where-it-behaves-differently), anything not taken in [not taken yet](#changes-in-the-original-project-not-taken-yet) with a backlog item, and the "up to #N" line at the top moves on.
6. **Check it:**
   - the pull request's checks: the tests on Linux and Windows, clippy, and the TypeScript check and page build. They run by themselves, and main only takes a pull request that passes them;
   - `src-tauri/src/lib.rs` keeps its Windows line endings (CRLF) on every line, as the original project has it; a file that changes them conflicts on every line at the next merge;
   - a security review of the merged result, its findings fixed before merging;
   - the app itself, against a test server: terminal, SFTP and SFTP root mode, a Wall behind a jump host, and Monitoring. Whatever wasn't tried is said plainly in the pull request;
   - lines in [CHANGELOG.md](CHANGELOG.md), under "From the original project".
7. **Merge it.** GitHub publishes it as the next release.

**Versions.**

- **Every merge to main is a release.** GitHub builds it and publishes it, tagged with the original project's version and the fork's count: `v0.4.0-fork.1`, `v0.4.0-fork.2`, and so on. A higher count is newer. There's nothing to tag by hand.
- **The title bar shows it,** as in `v0.4.0-fork.8`, and the profile screen says when there's a newer release.
- **When the original project releases a new version** and it's merged, the count starts again. Releases on 0.3.8 ran from `v0.3.8-fork.1` to `v0.3.8-fork.11`; with 0.4.0 in, they're `v0.4.0-fork.1` and on.
- **The installers keep the same names** in every release, so a link to the latest release's `Submarine-windows-x64-setup.exe` always gets the newest. On Windows the installer itself can only say `0.4.0`; the app still knows it's `0.4.0-fork.8`.
- **Windows and Linux are in every release; macOS only when asked for,** since a macOS minute costs ten of the account's GitHub Actions minutes. The repository variable `BUILD_MACOS` set to `true` adds it to every release, and **Run workflow** with **Build macOS too** ticked builds a Mac test copy. Or build it on your own Mac: [Builds on GitHub](README.md#builds-on-github) says how.
- **Before `v0.3.8-fork.1`,** the fork numbered its builds instead (build 244 was the last). Those builds are told about the new releases.

**Giving back.** Fixes that apply to the original project too go back to it as pull requests.
