# Changelog

This changelog lists the releases of [paulm981/Submarine](https://github.com/paulm981/Submarine), newest first.

- Each release is one merge to main, numbered with the original project's version and the fork's count, such as `v0.4.0-fork.2`. The title bar shows it.
- Before `v0.3.8-fork.1`, merges were numbered builds instead, down to build 236 and the summary below it.
- Sections called **From the original project** list what came in from [SinaXhpm/Submarine](https://github.com/SinaXhpm/submarine), with that project's pull request numbers.
- [FORK.md](FORK.md) explains what the fork does differently, and which of the original project's changes it hasn't taken yet.

## Next release

- **The host keys this app trusts always come first.** A key listed in `~/.ssh/known_hosts` only spares you the first-time question. If a server's key has changed, you get the warning even when known_hosts lists the new key. If known_hosts lists a different key for a server the app doesn't know yet, you get the changed-key warning instead of the first-time question. A server certificate can't stand in for a different key either: if the key inside it isn't the one the app trusts, the connection is refused. A key marked `@revoked` there is always refused, by Monitoring too. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **A key known_hosts vouches for is remembered.** The first time, it's saved to the app's trusted keys, as if you had approved it, so Monitoring can check that server too. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **Monitoring says when a certificate is all that vouches for a server.** Such a server (a Teleport node, say) isn't checked, and its card says why. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **An expired certificate from `~/.ssh/config` isn't tried.** The log names it and says to renew it (`tsh login`, for Teleport); its key is still tried on its own. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **HostKeyAlgorithms in `~/.ssh/config` doesn't make a server look changed.** Within its list, the kinds of key the app already trusts for the server are asked for first. A list that leaves them out is noted in the log. ([#43](https://github.com/paulm981/Submarine/issues/43))

### From the original project ([#41](https://github.com/paulm981/Submarine/issues/41))

- **Teleport and OpenSSH certificates, key files from `~/.ssh/config`, and your known_hosts** ([#170](https://github.com/SinaXhpm/submarine/pull/170)). With "Use settings from ~/.ssh/config" on, a server with no saved password or key logs in with the IdentityFile keys the config names, each with its certificate first (CertificateFile, or `<key>-cert.pub`). An encrypted key file asks for its passphrase once per tab. Host keys and certificate authorities in your known_hosts are used, hashed entries included, so Teleport nodes, which show only a certificate, now connect. Every jump host on the way gets the same. Nothing is written to `~/.ssh`.

## v0.4.0-fork.7

- **Every jump host waits its turn to log in, in a ProxyJump chain too.** **Settings › Logins at a time per jump host** now applies to each jump host on the way. A jump host the app dials itself has one line per address, shared by a saved jump host and the same machine in a chain from `~/.ssh/config`. One reached through another jump host has its own line for that way there, since the same address behind two gateways can be two machines. A jump host waits only while it logs in, so it never holds up the next one. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **Your saved jump host still comes first.** A server with its own saved jump host, SOCKS proxy or HTTP proxy goes that way, not through the ProxyJump in `~/.ssh/config`, and the log says so. A saved jump host that `~/.ssh/config` reaches through a ProxyJump is reached through it. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **A ProxyJump with `%` in it is refused, with a message.** OpenSSH fills in `%h`, `%r` and the like there; Submarine doesn't yet, so it asks you to write the jump hosts out in full instead of trying a host called `%h`. An IPv6 address with a zone, such as `[fe80::1%en0]`, is refused too, with its own message. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **Monitoring says when it can't check a server because of a ProxyJump.** A server whose jump host `~/.ssh/config` reaches through a ProxyJump isn't checked, and its card says why, as for a server behind a ProxyJump of its own. It used to go to the jump host directly, a different way from the terminal's. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **Each jump host gets 15 seconds to open the way to the next one.** A jump host that never answers ends the attempt, and the tab tries again, as it already did for the last jump host. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **When a connect through a ProxyJump fails, the check under the error looks at the first jump host**, the one the app dials, rather than the server behind the chain. ([#43](https://github.com/paulm981/Submarine/issues/43))

### From the original project ([#41](https://github.com/paulm981/Submarine/issues/41))

- **Servers are reached through the ProxyJump in `~/.ssh/config`** ([#169](https://github.com/SinaXhpm/submarine/pull/169)). A chain of jump hosts, as with `ssh -J`. Each jump host uses its own `~/.ssh/config` entry and logs in with an ssh-agent key or a password it asks for once per tab. Each one's host key is checked, and its own password is never sent to another. More than 8 jump hosts (a loop in the config) ends the attempt. With "Use settings from ~/.ssh/config" off, nothing changes.

## v0.4.0-fork.6

- **Closing a Wall can remember what to do with the tabs Broadcast took.** The question now has a **Remember my answer** box, and **Settings › Walls** has the same choice: **Ask each time**, **Put them back as tabs** or **Close them all**. It's kept on this computer only. **Close them all** still asks when one of those servers is busy: a file uploading or downloading, an archive being made or unpacked on the server, a mirror running, a Live Edit file with changes not sent yet, or an AI command running or waiting for you. **Back to tabs** on the Wall's bar still puts them back in one click. ([#101](https://github.com/paulm981/Submarine/issues/101))
- **An AI tab can sit beside your server.** A server tab shared with an AI tab shows that AI on the right, beside the terminal, so you don't have to go back to Home to talk to it. It's the conversation from Home itself, not a copy, and it goes back there when you go to Home. A Wall with a server shared with it shows it too. ([#100](https://github.com/paulm981/Submarine/issues/100))
- **Fold it to a thin bar** with a click or **Ctrl+Shift+L**, and drag its left edge to make it wider. It stays open or folded as you left it when you change tabs, and keeps its width. ([#100](https://github.com/paulm981/Submarine/issues/100))
- **Show it on any tab** with the new button beside the title bar's AI button, or **Ctrl+Shift+L**. On a tab that isn't shared it stays until you close it or leave the tab. ([#100](https://github.com/paulm981/Submarine/issues/100))
- **Its header** says which AI tab it is (pick another when there's more than one) and whether this tab is shared with it, with **Share** and **Unshare**. Unsharing takes it off the tab; the AI goes on in Home. Its requests have **Allow** and **Deny**, with the same care as in the AI panel: one click, never a key, not just after the card moves. **Back to Home** goes to the AI's own tab. ([#100](https://github.com/paulm981/Submarine/issues/100))
- **"Claude Code beside this"** in a server tab's **+** menu (right-click the **+**) starts Claude Code, shares the tab with it, and shows it beside the tab. ([#100](https://github.com/paulm981/Submarine/issues/100))

## v0.4.0-fork.5

- **Opening a Live Edit file again keeps your unsent changes.** If the copy on this computer has changes that never reached the server, it's copied to **Submarine unsent edits** in your Downloads folder before the server's version is opened, and the window says so. A file opened twice is also uploaded once per save, not twice. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Quitting the app keeps unsent Live Edit changes too**, in the same folder, as disconnecting and locking already did. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Kept copies are private.** On macOS and Linux, a copy kept in **Submarine unsent edits** can be read only by you, and so can the folder. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Two downloads to the same name don't replace each other.** If a file of that name was saved there while a download ran, you're asked before it's replaced, as when it was there at the start. In a folder download it's left as it was, and the card says how many. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **On Windows and macOS, a folder download keeps files whose names differ only in case apart.** Those systems treat `Readme.md` and `README.md` as one name, so the second used to replace the first. It's now left out, and the card says how many. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **A mirror never puts a server's file in a folder that runs programs at login**, such as the Startup folder on Windows or `~/.config/autostart` on Linux. Folder downloads already left such files out; a mirror now does too, and its log says why. A link to such a folder is seen through, in both. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **A mirror keeps a file's permissions on the server.** Sending a changed file used to replace it with one anyone on the server could read, and a script lost its permission to run. The new copy now has the old one's permissions, owner and group (where the server allows it), and a new file has the one on your computer's. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **On macOS and Linux, the Local pane asks before it opens any file marked as executable**, not only ones whose name says they run. A program or script without an extension used to run straight away. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Files unpacked from a downloaded archive are marked as downloaded too**, on Windows and macOS, when the Local pane extracts an archive that's marked that way. The system then asks before a program from it runs, as it does for the archive. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Characters that disguise a file name are left out in more places**: the transfer cards, the name shown while you drag, the question before a file is replaced, and the notice about kept Live Edit changes. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Old temporary folders are cleared at startup.** Live Edit and drag copies left behind by a run that crashed more than a day ago stayed in the system's temp folder; the check for them never matched. Live Edit copies in them are moved to **Submarine unsent edits** first, since nobody can tell any more whether they were uploaded. Another copy of Submarine that's still running keeps its folder. ([#93](https://github.com/paulm981/Submarine/issues/93))
- **Locking the profile or turning AI tools off clears the AI panel's list of what ran.** The commands and their whole output used to stay in the panel after a lock, where the next profile could open them. ([#97](https://github.com/paulm981/Submarine/issues/97))
- **An AI command you allow is in `ai-audit.log` straight away,** and gets a second line when it ends, with its exit code. Before, nothing was written until it ended, so a command still running when Submarine quit or crashed was never logged. ([#97](https://github.com/paulm981/Submarine/issues/97))
- **The AI is given fewer passwords from config files.** Output it gets now also hides the password in Cisco lines (`enable secret 5 …`, `enable password level 15 …`, `password 7 …`, `username … secret …`), Redis's `requirepass`, `.netrc`, `.pgpass` (spaces and all), Juniper's `$9$…` and `encrypted-password`, Docker's `config.json`, Wi-Fi `psk=` lines and Debian's `wpa-psk`. ([#97](https://github.com/paulm981/Submarine/issues/97))
- **An AI request's Allow and Deny also wait half a second after the AI panel scrolls,** as they already did after the card moved. A request scrolled under the pointer can't be decided by a click meant for something else. ([#97](https://github.com/paulm981/Submarine/issues/97))
- **Only one program at a time can connect as an AI tab.** If another one tries with the same access, it's refused and the AI panel shows a warning on that tab. Claude Code reconnecting still works. Submarine's helper also checks that it's talking to Submarine itself before it sends anything. ([#97](https://github.com/paulm981/Submarine/issues/97))
- **SFTP or forwarding on its own connection fails at once when the jump host asks for a code.** It used to wait two minutes for a code nobody was asked for, and other servers behind that jump host waited with it. The server's own tab still asks for the code. ([#92](https://github.com/paulm981/Submarine/issues/92))
- **Monitors log in to a jump host a few at a time,** like terminals, so starting them all doesn't log in to it a hundred times at once. Waiting for a turn doesn't count as the server being down. ([#92](https://github.com/paulm981/Submarine/issues/92))
- **Monitoring finds a jump host by its `~/.ssh/config` name,** as terminals do: its HostName, Port and User. If the config reaches the jump host through a ProxyCommand, the server isn't checked, and its card says why. A ProxyJump given to the jump host isn't used yet, by Monitoring or by terminals: both dial the jump host directly. ([#92](https://github.com/paulm981/Submarine/issues/92))
- **A reconnect doesn't bring back a forward deleted on another computer.** It starts the forwards that are saved now. ([#92](https://github.com/paulm981/Submarine/issues/92))
- **A reconnect can't leave a hidden connection open.** A login from before the reconnect that finished at just the wrong moment could keep a connection, and its jump host, open with nothing watching it. ([#92](https://github.com/paulm981/Submarine/issues/92))
- **A jump host saved with a port above 65535 is refused** with "Port … isn't a valid port", instead of being reached on another port. So is such a server or jump host in Monitoring. ([#92](https://github.com/paulm981/Submarine/issues/92))
- **Send to all leaves out control characters**, as a paste does. A Library command someone shared, or text typed into **Send** on a Wall, can't press keys you didn't see. Tab and line breaks stay, and the Wall says when anything was left out. ([#98](https://github.com/paulm981/Submarine/issues/98))
- **On-connect commands leave out control characters too.** When any were left out, the terminal says so before the commands are typed. ([#98](https://github.com/paulm981/Submarine/issues/98))
- **Closing a terminal while it opens closes its shell on the server.** It used to keep running there, unseen. This goes for a container's terminal too. ([#98](https://github.com/paulm981/Submarine/issues/98))
- **Turning Broadcast off on a Wall says where your typing goes.** Nothing is typed until you click a terminal, and the Wall says so until you do. The terminal under the mouse doesn't get the keys, so the rest of a line typed to all can't go to one server by itself. ([#98](https://github.com/paulm981/Submarine/issues/98))
- **A terminal on a Wall has its Find box (Ctrl+F), Saved password button and Copied and Pasted notes**, as in its own tab. Ctrl+R opens command history there too. ([#98](https://github.com/paulm981/Submarine/issues/98))
- **All tabs (Ctrl+Shift+K) shows a renamed tab by its new name**, and finds it by that name or the server's. So does a Wall's **Add**. ([#98](https://github.com/paulm981/Submarine/issues/98))
- **Ctrl+V, Ctrl+Shift+V and ⌘V ask "Paste N lines?" too**, as a right-click does, when the lines would run one by one. ([#98](https://github.com/paulm981/Submarine/issues/98))

## v0.4.0-fork.4

- **Importing thousands of servers takes a moment.** Checking which ones are already saved used to take seconds for 10,000 rows, with the app waiting. ([#94](https://github.com/paulm981/Submarine/issues/94))
- **The `~/.ssh/config` import reads settings as `ssh` does.** A `Host *` block, and other wildcard blocks that match a host, give it their settings as defaults. When a setting appears twice, the first one counts. A `Match` block's lines are left out instead of being added to the host above them. ([#94](https://github.com/paulm981/Submarine/issues/94))
- **The PuTTY import brings in telnet sessions** as telnet servers, and leaves out "Default Settings" and other sessions with no host. Serial, raw and rlogin sessions are listed as not imported instead of coming in as SSH servers. ([#94](https://github.com/paulm981/Submarine/issues/94))
- **Two imported servers with the same name are picked one at a time.** Ticking one used to tick both, and **Select all** didn't count as all. ([#94](https://github.com/paulm981/Submarine/issues/94))
- **Switching a server from SSH or RDP to VNC or telnet empties its password,** and the form says so. Saving clears the saved one, so that password is never sent over VNC or telnet. VNC needs its own password anyway. ([#94](https://github.com/paulm981/Submarine/issues/94))
- **An error in a side panel goes when you close it,** and doesn't show up in the next panel you open. A command with no title or nothing to run says so in the panel. ([#94](https://github.com/paulm981/Submarine/issues/94))
- **Smaller fixes:** ([#94](https://github.com/paulm981/Submarine/issues/94))
  - deleting servers from the session tree also forgets their SFTP bookmarks;
  - a duplicated RDP server keeps its window and sharing options on this computer;
  - the Remote Desktop link dialog closes with Escape and keeps Tab inside it;
  - an empty Port field in the server form asks for a port instead of saving none;
  - a command for servers that were all deleted says "For servers that were deleted".
- **A server switched from RDP to telnet or VNC by another computer waits for your OK too**, in a shared profile, as one switched from SSH does. It stays an RDP server here until you allow the switch, or keep it RDP. Before, switching it to RDP first got round the hold. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **Allowing on-connect commands counts only for the commands you were shown.** In a shared profile, if someone changes them again while you're looking at them, Allow is refused and you're shown the new ones to check. Before, the OK went to whatever had come in last. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **Stopping, starting or renaming a forward no longer deletes forwards that came by sync.** With a tab open, a forward someone added on another computer was dropped everywhere the next time you changed one in the Ports panel, and in a shared profile it stopped waiting for your OK. The panel now shows the synced forwards and keeps them. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **Replace the cloud copy with this device uploads everything again.** After an earlier sync, it uploaded only recent changes, so the cloud copy could end up missing most of the profile and the key other devices unlock it with. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **A restore or import that fails no longer deletes a profile of the same name** that another window made or opened while it ran. It's refused instead, and only what the failed restore made itself is cleaned up. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **Deleting a profile deletes the copies a sync kept of it too**, the `<name>.submarine.bak-<time>` files next to it. They hold the same servers, logins and keys. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **A forward you stopped stays stopped until you start it.** In a shared profile, someone else starting it again now waits for your OK, like a forward they add. A forward that arrives stopped doesn't need your OK, since it runs nothing. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **A token given as the user name in a git address** (`https://TOKEN@host/…`) is no longer on git's command line, where other users of the computer could read it. It's handed to git the way a password in the address already is. ([#95](https://github.com/paulm981/Submarine/issues/95))
- **Root with `dzdo su -` or `sudo su -` that asks for your password.** Where sudo or dzdo lets you in only through a root shell, and wants your password for it, the app now gets root that way once you enter it: in SFTP root mode, the Info panel and the Docker tab. ([#96](https://github.com/paulm981/Submarine/issues/96))
- **Root works when your login shell is csh or tcsh.** SFTP root mode, the Info panel's actions and Docker as root used to fail on such accounts. ([#96](https://github.com/paulm981/Submarine/issues/96))
- **A root password is never tried twice by mistake, so it can't lock your account.** A wrong one costs one failed attempt, even when several actions were waiting for it or the server is slow to answer. One that stopped working is forgotten for the whole tab, SFTP root mode included, and asked for again. Each action signs in once. ([#96](https://github.com/paulm981/Submarine/issues/96))
- **Your password goes only to sudo or dzdo.** It's checked on its own before anything runs, whatever prompt your server shows (Kerberos included). A command that runs without a password never gets it, and a command's own output is never mistaken for sudo's answer. ([#96](https://github.com/paulm981/Submarine/issues/96))
- **A container's terminal asks for the root password** when it needs one, as the Docker tab does, instead of showing `[SUDO] NEED_PASSWORD`. ([#96](https://github.com/paulm981/Submarine/issues/96))
- **A server's saved password is typed only into that server's own terminals.** ([#96](https://github.com/paulm981/Submarine/issues/96))

### From the original project ([#41](https://github.com/paulm981/Submarine/issues/41))

- **The last column of a full line shows whole** ([#227](https://github.com/SinaXhpm/submarine/pull/227)). Its right edge was sometimes cut off when the terminal draws without the GPU. On a Wall too.
- **Delete your Submarine Cloud account from the app** ([#225](https://github.com/SinaXhpm/submarine/pull/225)). If you turned Submarine Cloud on, its panel has **Delete Submarine Cloud account…**, confirmed with the account password. It deletes the account and what the server holds for it. The profiles on your computer, and sync through your own folder or git repository, stay as they are.
- **A privacy policy** ([#225](https://github.com/SinaXhpm/submarine/pull/225)). **Privacy** at the bottom of the profile screen opens it. It's the original project's, with a note on where this fork differs.
- **Submarine Cloud's new address** ([#228](https://github.com/SinaXhpm/submarine/pull/228)). When it's on, the app asks the sync service at its new address, under `/api`, as the original project's app now does. Nobody is signed out, and no profile has to move.
- **Test builds** ([#226](https://github.com/SinaXhpm/submarine/pull/226)). A build started by hand now has a version Android accepts.
- **The original project's website** ([#229](https://github.com/SinaXhpm/submarine/pull/229)) comes with its source, in `website/`. It isn't part of the app, and this fork doesn't publish it.

## v0.4.0-fork.3

- **Search for the jump host.** In a server's form, **Jump via** is now a search box: type part of a name, address or folder, and pick with the arrow keys and Enter or a click. Servers already used as a jump host are listed first. ([#102](https://github.com/paulm981/Submarine/issues/102))

## v0.4.0-fork.2

- **Enter in a question does what the focused button says.** Tab to Cancel (or Skip, or ✕) and press Enter, and that's what happens. Before, Enter always took the main action, so it could delete a folder or overwrite a file you meant to keep. With no button focused, Enter still takes the main action. ([#94](https://github.com/paulm981/Submarine/issues/94))

## v0.4.0-fork.1

- **Files from a server don't run on macOS and Linux either.** Live Edit opens a file in your default text editor on macOS, and in the text editor set for plain text on Linux. The Local pane asks before it opens a file that could run (on macOS a `.terminal` or `.command` file too, and Windows programs, which Wine can run), and downloads on macOS are marked as from the internet. ([#84](https://github.com/paulm981/Submarine/issues/84))
- **A jump host that stops answering no longer keeps a tab connecting.** Reaching the server behind it has 15 seconds, and logging in to it 5 minutes, prompts included. Other servers behind it stop waiting for their turn. ([#84](https://github.com/paulm981/Submarine/issues/84))
- **Home terminals and AI tabs** leave out escape sequences that would freeze the window, as SSH terminals do. So does a container's terminal opened as root. ([#84](https://github.com/paulm981/Submarine/issues/84))
- **Two windows syncing through the same folder or git repository take turns**, now that each profile can have a window of its own. A file in that storage that's far too big is skipped, and the log says why. ([#84](https://github.com/paulm981/Submarine/issues/84))

### From the original project ([#41](https://github.com/paulm981/Submarine/issues/41))

- **Version 0.4.0** ([#220](https://github.com/SinaXhpm/submarine/pull/220)). The original project is at 0.4.0, so the fork's releases are now numbered `v0.4.0-fork.1` and on.
- **One window per profile** ([#204](https://github.com/SinaXhpm/submarine/pull/204)). A profile open in one Submarine window can't be opened in a second one, so two windows no longer save over each other's changes. Other profiles still open, and a profile open elsewhere can't be deleted.
- **Locking a profile locks sharing too** ([#223](https://github.com/SinaXhpm/submarine/pull/223)). The sharing passphrase is asked for again after a lock. Locking also ends the AI tabs' access to the profile's servers.
- **A copy before a sync deletes** ([#224](https://github.com/SinaXhpm/submarine/pull/224)). Before a sync removes items that were deleted elsewhere, the profile file is copied next to itself as `<name>.submarine.bak-<time>`: at most one copy every 10 minutes, the newest five kept. The log says how many items went and what the copy is called. This works for sync through your own folder or git repository too.
- **Live Edit keeps syncing** ([#218](https://github.com/SinaXhpm/submarine/pull/218)). It stops only after 12 hours without a save, and says so. A copy with edits that weren't uploaded is kept, and you're told. Editors that save by replacing the file (vim, gedit, JetBrains) are now seen on every save. A Live Edit save and an upload to the same file take turns.
- **Tag colours** ([#219](https://github.com/SinaXhpm/submarine/pull/219)). The server form has 20 colours, and a last swatch opens the system's colour picker for any other.
- **The built-in RDP client needs NLA** ([#222](https://github.com/SinaXhpm/submarine/pull/222)). A server that doesn't ask for Network Level Authentication is stopped before anything is sent, and the tab offers **Allow without NLA** for that server. The server form has the same switch, kept on this computer. The system's RDP app isn't affected.
- **Monitoring** ([#216](https://github.com/SinaXhpm/submarine/pull/216)). Locking a profile stops every monitor. A server that takes the connection and then stops answering now counts as down; through a jump host it gets twice as long.
- **Imports** ([#213](https://github.com/SinaXhpm/submarine/pull/213)). A MobaXterm export with thousands of sessions of the same name imports in a moment, not minutes. PuTTY session names with accents or in other scripts come out as typed.
- **Smaller fixes** ([#214](https://github.com/SinaXhpm/submarine/pull/214)):
  - a `~/.ssh/config` with thousands of bad lines no longer slows every connect;
  - a ProxyCommand to an IPv6 address in brackets works;
  - a service whose name has a backslash in it can be started and stopped;
  - renaming or moving in the Local pane never replaces a file or folder that's there.
- **Security fixes**:
  - a program that prints a huge number in an escape sequence can't freeze the window ([#202](https://github.com/SinaXhpm/submarine/pull/202));
  - VNC and RDP tabs hold new frames back while the window is behind, so a slow window or a hostile server can't run the app out of memory ([#215](https://github.com/SinaXhpm/submarine/pull/215));
  - a paste leaves out control characters (tab and line breaks stay), so pasted text can't end a bracketed paste and run what follows. Broadcast on a Wall too ([#217](https://github.com/SinaXhpm/submarine/pull/217));
  - a program on a server isn't opened from the file pane; you're told to download it instead ([#212](https://github.com/SinaXhpm/submarine/pull/212));
  - an oversized answer from the sync server or the update check can't run the app out of memory ([#203](https://github.com/SinaXhpm/submarine/pull/203));
  - a synced server whose forwards or mirrors aren't valid no longer brings up the error screen when it's edited ([#205](https://github.com/SinaXhpm/submarine/pull/205));
  - an encrypted PuTTY or PKCS#8 key that asks for far more work to unlock than a real key is refused ([#206](https://github.com/SinaXhpm/submarine/pull/206));
  - profile files are read with a size limit, an import never replaces a profile that's there, and an export can't write over the profile's own file ([#207](https://github.com/SinaXhpm/submarine/pull/207));
  - a profile file that's busy for a moment no longer turns off Windows Hello unlock ([#208](https://github.com/SinaXhpm/submarine/pull/208));
  - a hostile server can't keep a login, a telnet or an RDP connect going forever, or slip a second request into an HTTP proxy's ([#209](https://github.com/SinaXhpm/submarine/pull/209));
  - a mirror's folder on this computer can't be a network share ([#210](https://github.com/SinaXhpm/submarine/pull/210));
  - a copy of a server keeps waiting for your OK where the original was waiting. The fork already did this ([#211](https://github.com/SinaXhpm/submarine/pull/211)).
- **Release builds** ([#221](https://github.com/SinaXhpm/submarine/pull/221)). The actions they use are pinned to exact versions, and the Android app, when it's built, is signed by a separate job that runs nothing else.
- **A mirror's folder must be on a drive letter**, on Windows. Some ways of writing a network share still got past the check from #210 and were looked up on the network. ([#41](https://github.com/paulm981/Submarine/issues/41))
- **Paste in the Library works like a paste.** A command's or note's **Paste** leaves out control characters, keeps several lines together where the program takes a paste as one block, and asks first where each line would run. **Run** works as before. ([#41](https://github.com/paulm981/Submarine/issues/41))
- **Unsent Live Edit changes are kept when you disconnect or lock the profile.** A copy with changes that never reached the server is moved to **Submarine unsent edits** in your Downloads folder, and the window says so, with a button to show it. ([#41](https://github.com/paulm981/Submarine/issues/41))

## v0.3.8-fork.11

### From the original project ([#41](https://github.com/paulm981/Submarine/issues/41))

- **RDP servers** ([#173](https://github.com/SinaXhpm/submarine/pull/173)). Add server has a **Type**: SSH, RDP, Telnet or VNC. An RDP server opens in this computer's Remote Desktop app (Remote Desktop Connection on Windows, Remmina or FreeRDP on Linux), straight or through an SSH server. The app asks for the password itself. Window size and what's shared are kept on this computer. Your Remote Desktop links stay as they are, beside RDP servers; joining the two comes next ([#42](https://github.com/paulm981/Submarine/issues/42)).
- **A built-in RDP client, experimental** ([#199](https://github.com/SinaXhpm/submarine/pull/199)). An RDP server can instead show its desktop in its own tab: choose **Open in** on the server. Your password and the server's certificate check stay in the app; the certificate is remembered like an SSH host key. It's off unless you choose it.
- **Telnet servers** ([#192](https://github.com/SinaXhpm/submarine/pull/192)) open in a terminal tab, for older network devices. Telnet isn't encrypted: anything you type, passwords too, crosses the network as plain text, and the form, the list and the tab say so. Going through an SSH server keeps the part up to that server encrypted.
- **VNC servers** ([#193](https://github.com/SinaXhpm/submarine/pull/193)) show their screen in the tab, straight or through an SSH server, with touch, zoom and a keyboard on a phone. Plain VNC isn't encrypted either.
- **A VNC server never uses a saved login.** VNC sends its password with weak protection and doesn't check the server's identity, so a login you also use elsewhere could be read on the way. Save the VNC password on the server itself, or type it when the tab asks. ([#41](https://github.com/paulm981/Submarine/issues/41))
- **A server another computer switches from SSH to telnet or VNC stays SSH here until you choose**, in a shared profile. Both would send its saved password where it could be read. The switch waits in **Waiting for your OK**, with the other held changes. Until then the server keeps its old port here too.
  - **Allow** makes the switch here as well.
  - **Keep it SSH** changes it back to SSH on the other computers too. ([#41](https://github.com/paulm981/Submarine/issues/41))
- **RDP servers and Remote Desktop links look different.** In the session tree, RDP, telnet and VNC servers show their type, as in the server list, and a telnet server its not-encrypted mark. Remote Desktop links say **LINK**, in the tree and the list. ([#41](https://github.com/paulm981/Submarine/issues/41))
- RDP, telnet and VNC servers don't go on Walls or into Broadcast, and can't be shared with AI tools: those work with SSH terminals.
- **Unlock a profile with Windows Hello** ([#195](https://github.com/SinaXhpm/submarine/pull/195)), or a fingerprint on Android. Turn it on for a profile in the profile panel, with its password once. The password isn't kept: a key locked by Windows Hello is, on this computer only, and it never syncs. Not offered on macOS or Linux.
- **Security updates** ([#198](https://github.com/SinaXhpm/submarine/pull/198)): the TLS library behind sync and the update check, with four advisories fixed, and the zip library.
- **The Android build**, when this fork's builds turn it on, uses the NDK it installs ([#196](https://github.com/SinaXhpm/submarine/pull/196)).

## v0.3.8-fork.10

### From the original project ([#41](https://github.com/paulm981/Submarine/issues/41))

- **A saved password at a password prompt** ([#182](https://github.com/SinaXhpm/submarine/pull/182)). When a terminal stops at a password prompt (sudo, dzdo, su, ssh), a **Saved password** button types a saved password there, without pressing Enter. It goes from your profile straight to the server; the window never sees it.
  - It offers this server's own password and its saved login. Another saved login is offered only when the prompt asks for that login's user by name, so a prompt can't fish for a password it has nothing to do with.
- **A color scheme of your own** ([#183](https://github.com/SinaXhpm/submarine/pull/183)). Settings › Terminal › Colors takes a Windows Terminal color scheme pasted as JSON. It stays on this computer; a server's own scheme is still one of the built-in ones, so it syncs. Also there: **Bold text in bright colors**, on by default.
- **Library commands for some servers** ([#185](https://github.com/SinaXhpm/submarine/pull/185)). A command can be for all servers, or only for the servers and folders you tick. A server's Library shows its own commands first, then the ones for all, and hides the rest. The choice syncs with the command.
- **Compare folders** ([#184](https://github.com/SinaXhpm/submarine/pull/184)). **Compare** in the file browser marks, in both panes, what is only on one side, a different size or kind, or newer or older. **Hide files that are the same** narrows the lists to the differences.
- **The path bar** ([#181](https://github.com/SinaXhpm/submarine/pull/181)): each folder in it opens with a click. The rest of the bar still edits the path as text.
- **Walls: choose the rows** ([#190](https://github.com/SinaXhpm/submarine/pull/190)). Beside **Cols**, **Rows** fits one to four rows on screen (more terminals scroll), or **Auto**. Rows now fit the Wall's height, so a Wall no longer scrolls when its terminals would fit. The choice isn't saved with a saved Wall yet.
- **Terminals:**
  - the shell gets a new size once it has held for a moment, so turning a phone or opening a panel no longer garbles the screen ([#188](https://github.com/SinaXhpm/submarine/pull/188));
  - a terminal at its bottom stays there when its box changes size ([#191](https://github.com/SinaXhpm/submarine/pull/191)).
- **Older network devices connect:** the SHA-1 group-exchange key exchange is offered, after all the others ([#189](https://github.com/SinaXhpm/submarine/pull/189)). The fork already offered the NIST curves, and still leaves out the weak 1024-bit group 1.
- **Windows:** calls between the window and the app take the faster path again ([#186](https://github.com/SinaXhpm/submarine/pull/186)).
- **Window buttons** show at any window width on the PC, and never on phones ([#187](https://github.com/SinaXhpm/submarine/pull/187)).

Not taken: [#180](https://github.com/SinaXhpm/submarine/pull/180) gives a terminal pinned to a Wall its own size back. The fork's Walls show the terminal itself rather than a second copy, so it already has one size.

## v0.3.8-fork.9

- **AI tools, safer** ([#85](https://github.com/paulm981/Submarine/issues/85)), from an independent security review:
  - **Stop ends the command on the server.** Stopping, unsharing, closing the AI tab or turning AI tools off no longer only stops Submarine waiting for the command.
  - **Allow can't be clicked by accident.** A double-click can no longer Allow the next request: Allow and Deny take a single click, and wait half a second after a request appears or moves.
  - **Hidden characters show.** The AI panel shows every hidden or invisible character in a command, on waiting, running and finished requests.
  - **More secrets are hidden from the AI:**
    - passwords on command lines (`--password`, `mysql -p…`, `sshpass -p`) and `DB_PASS=` and similar;
    - Apache password hashes and web tokens;
    - private keys in base64, including certificate-and-key bundles.
  - **The AI can't flush the audit log or tie up memory.** Every request counts toward the limit, commands are logged at most 4 KB with a fingerprint, three older logs are kept, and what one AI connection can make Submarine hold is capped.

## v0.3.8-fork.8

- **Two uploads to the same file:** the second one now asks before overwriting what the first one put there ([#84](https://github.com/paulm981/Submarine/issues/84)).
- **Duplicating a server that fails** no longer leaves a "deleted" record that syncs to your other devices.
- **Servers that open on launch** have their operating system checked on the port they connected to.
- **The check under a failed connection** ([#84](https://github.com/paulm981/Submarine/issues/84)):
  - a proxy saved without a port is checked at its usual port (1080 or 8080);
  - a jump host reached through a ProxyCommand isn't probed directly;
  - a name that doesn't resolve shows the reason the system gave;
  - lines from an old check no longer appear after the next attempt has started.

  The connect and the check now choose the route the same way.
- **Session tree:** with **Show only open** and a search, empty folders no longer show. Right-click › Open goes to an already-open server instead of opening it again.
- **Ctrl+Tab** to a tab that's part of a split keeps the split, as clicking it does.

## v0.3.8-fork.7

- **Broadcast takes your tabs into its Wall** ([#73](https://github.com/paulm981/Submarine/issues/73)). The title bar shows the Wall instead of a copy of every server as well. **Back to tabs** on the Wall's bar puts them back where they were, and closing the Wall asks whether to put them back as tabs or close them all. Connections stay up either way.
  - A terminal's ✕ on that Wall gives it its tab back instead of closing it.
- **A dot on a tab with new output** ([#70](https://github.com/paulm981/Submarine/issues/70)). A terminal you aren't looking at shows a dot on its tab, and on its server's tab (or its Wall's tab), when it prints something. The dot goes away when you look at it. Prompt redraws and the first second after you leave a tab don't count.
- **Drag a tab to move it** ([#71](https://github.com/paulm981/Submarine/issues/71)), in the title bar and among a server's terminals. Ctrl+Tab follows the new order. Esc puts it back. Without a mouse, use **Move left** and **Move right** in the tab's right-click menu.
- **Compact tabs** ([#71](https://github.com/paulm981/Submarine/issues/71)): a setting under Settings › UI Customization for flat tabs that fit more across (Home and Wall tabs too), with long names cut short and shown in full on hover.

- **Picking several terminals on a Wall** works as in the session tree ([#84](https://github.com/paulm981/Submarine/issues/84)). Shift-click picks the run from the last terminal you clicked, and a second Shift-click can make it shorter as well as longer. Turning Broadcast off, Clear, or pausing what you picked starts afresh.

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
