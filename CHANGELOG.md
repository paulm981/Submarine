# Changelog

This changelog lists the releases of [paulm981/Submarine](https://github.com/paulm981/Submarine), newest first.

- Each release is one merge to main, numbered with the original project's version and the fork's count, such as `v0.4.0-fork.2`. The title bar shows it.
- Before `v0.3.8-fork.1`, merges were numbered builds instead, down to build 236 and the summary below it.
- Sections called **From the original project** list what came in from [SinaXhpm/Submarine](https://github.com/SinaXhpm/submarine), with that project's pull request numbers.
- [FORK.md](FORK.md) explains what the fork does differently, and which of the original project's changes it hasn't taken yet.

## Next release

## v0.4.0-fork.74

- **Connections can be left alone.** Settings › Servers & sessions has **Send keepalives** and **Reconnect automatically**, both on as before; a server can have its own choice in its form. With reconnecting off, a dropped session stays down until you press Reconnect, and a kept session is waiting for you when you do. A server that says why it closed the connection has its reason in the banner. (Their [#310](https://github.com/SinaXhpm/submarine/pull/310); [#227](https://github.com/paulm981/Submarine/issues/227))
- **Forms ask before losing your changes.** Closing the server, login, SSH key, command or note form with unsaved changes asks Save, Discard or Keep editing; with none it closes at once. A session tab's right-click menu has **Edit server**, and a double-click can no longer answer a question by accident. (Their [#311](https://github.com/SinaXhpm/submarine/pull/311) and [#312](https://github.com/SinaXhpm/submarine/pull/312); [#227](https://github.com/paulm981/Submarine/issues/227))
- **Typing in the terminal is steadier.** Keys typed fast, or while the shell is still opening, arrive in order instead of being lost. Ctrl+- and Ctrl+/ undo in bash, zsh and Emacs. A held arrow key in nano no longer runs on, the terminal takes the keyboard back when a tool panel closes, and Option+drag selects inside mouse programs on a Mac. (Their [#306](https://github.com/SinaXhpm/submarine/pull/306), [#307](https://github.com/SinaXhpm/submarine/pull/307), [#308](https://github.com/SinaXhpm/submarine/pull/308), [#313](https://github.com/SinaXhpm/submarine/pull/313) and [#315](https://github.com/SinaXhpm/submarine/pull/315); [#227](https://github.com/paulm981/Submarine/issues/227))
- **Security keys named in `~/.ssh/config` log in** through the ssh-agent, and the log says when the key wants a touch or its PIN. (Their [#309](https://github.com/SinaXhpm/submarine/pull/309); [#227](https://github.com/paulm981/Submarine/issues/227))
- **Safer and easier on a phone.** The vault's temporary data stays in memory, only real web addresses open in the browser, and the controls on a phone have bigger touch areas. (Their [#300](https://github.com/SinaXhpm/submarine/pull/300) and [#302](https://github.com/SinaXhpm/submarine/pull/302); [#227](https://github.com/paulm981/Submarine/issues/227))

## v0.4.0-fork.73

- **Broadcast is one tab.** While Broadcast is on, its servers' tabs fold into one Broadcast tab. A tile's "Open this session in its own tab" pops that server out of Broadcast, back into its own tab. The ✕ on the Broadcast tab ends Broadcast and brings every tab back; the sessions stay connected. ([#224](https://github.com/paulm981/Submarine/issues/224))

## v0.4.0-fork.72

- **Broadcast to a folder.** Right-click a folder in the session tree and pick Broadcast to all in this folder, or select servers and pick Broadcast to N selected: they open side by side on the Wall, and what you type goes to all of them. A folder is the saved group: no separate list to keep. ([#218](https://github.com/paulm981/Submarine/issues/218))

## v0.4.0-fork.71

- **Broadcast: Send a set of commands to all.** Send… on Broadcast's bar opens a box for a set of commands, typed into every server line by line, with Enter after the last one unless you untick it. It says how many servers get it and who's left out (paused, not connected, waiting for the next line) before anything goes, and keeps control keys and escape sequences out. ([#218](https://github.com/paulm981/Submarine/issues/218))

## v0.4.0-fork.70

- **Broadcast: a server that joins partway through a command waits for the next line.** Tick a server, or resume one, while a command is half typed, and it doesn't get the rest of that command: its tile says Next line until you press Enter (or Ctrl+C), and the bar counts it. So `rm -rf /tmp/x` never reaches a server as `/x`. ([#218](https://github.com/paulm981/Submarine/issues/218))

## v0.4.0-fork.69

- **Broadcast: type to one server, or pause one.** On Broadcast's Wall, the target button on a tile types to that server alone, until you press Type to all. The pause button (or Ctrl+Shift+P) leaves a server out until you resume it, and its tile says Paused. Ctrl-click or Shift-click pause buttons to pause or resume several at once. The bar says who you're typing to. ([#218](https://github.com/paulm981/Submarine/issues/218))

## v0.4.0-fork.68

- **Broadcast is one screen for many servers again.** Tick servers in the Broadcast menu, or All, and their terminals open side by side on the Wall; what you type in any of them goes to all of them. Ticking is turning it on, so there's no separate switch to forget after a restart, and unticking them all ends it and takes you back where you were. ([#218](https://github.com/paulm981/Submarine/issues/218))

## v0.4.0-fork.65

- **When the AI asks for a server, Stop still wins while it connects.** After you click Share, the card stays and says it's connecting; Don't share, Stop, or the AI withdrawing before the tab is up means it isn't shared. The AI side pane shows these cards and counts them, and only SSH servers are offered (not RDP, VNC or telnet), found by name ignoring case in any alphabet. ([#210](https://github.com/paulm981/Submarine/issues/210))
- **Editing a highlighting rule no longer turns it back on, or changes its color, behind your back.** A rule you turned off on another computer stays off when you edit it here, and a color from a newer version is kept unless you pick another. On Gruvbox Dark, "Bright blue" rules show in bright blue again. Highlighting also leaves alone a few more kinds of text a program colored itself. ([#211](https://github.com/paulm981/Submarine/issues/211))

## v0.4.0-fork.64

- **Right-click on a file name in a terminal edits the file shown, on the server shown.** The name is read as the terminal shows it, past wide characters (CJK, emoji) and across a long path that wraps onto the next row. Once your shell has gone on to another host with `ssh`, Edit locally isn't offered until it's back, and the menu says which server it edits on. A right-click on your prompt pastes again, and names in any alphabet get the menu. ([#209](https://github.com/paulm981/Submarine/issues/209))
- **Open terminal here goes to the folder after the server's on-connect commands,** so a `cd` or `sudo -i` among them no longer takes the terminal elsewhere or gets the `cd` as a password; and with WSL as your Home shell, it opens in the folder you picked. ([#209](https://github.com/paulm981/Submarine/issues/209))

## v0.4.0-fork.63

- **Broadcast: one slow server no longer holds up typing to the others, and a big paste reaches every server.** A server that falls behind, such as one still taking a large paste on a slow link, gets its keys on its own, in order, until it catches up, while the rest keep up. A paste over 1 MB used to reach only the server you were typing in; it now goes to all of them. Terminals waiting to open also open while the window is minimized. ([#205](https://github.com/paulm981/Submarine/issues/205))

## v0.4.0-fork.62

- **"Run commands that only read" closes holes that let a command write or reach a device.** `uniq - FILE` could overwrite a file; a path through `/proc/…/root` could open a device; a glob such as `/etc/shado?` got past the check for files that hold secrets; and `date -f`, `getent hosts`, `rpm -qp` and `journalctl --image` could set the clock, reach out or attach a disk. Each of these now asks. The level also runs commands by itself only where Submarine knows the server's OS and it isn't Windows; elsewhere only your allow list runs, and the server's marker says "allowed commands run". ([#206](https://github.com/paulm981/Submarine/issues/206))

## v0.4.0-fork.61

- **Regular-expression highlighting rules work again.** Your own rules can be regular expressions once more, run on RE2, which doesn't backtrack: an expression too big to run on every line is refused when you add it, and one that's still slow on real output is turned off rather than freezing the window. `^` and `$` mean the start and end of the terminal's line, even where a program colors part of it. An expression RE2 can't run (with a back-reference `\1` or a lookaround) says why when you add it, and a synced rule this computer can't use says so in the list. ([#49](https://github.com/paulm981/Submarine/issues/49))

## v0.4.0-fork.60

- **Highlighting follows terminal programs more closely, and keeps its colors on Solarized.** Text after a program saves and restores its colors (as `less` and `vim` do on exit), or after some rarer escape sequences, is highlighted or left alone correctly. On Solarized, whose bright colors are grays, bright highlights show in their normal color. ([#49](https://github.com/paulm981/Submarine/issues/49))
- **Your highlighting rules no longer lose edits.** Turning a rule on or off changes only that, not an edit made on another computer; Enter can't add a rule twice; a rule deleted on another computer isn't shown, and saving it says it was deleted. ([#49](https://github.com/paulm981/Submarine/issues/49))

## v0.4.0-fork.59

- **Right-click a file name in a terminal** to copy its path or edit it in your editor (Live Edit). Paste is still the first entry, and right-click anywhere else pastes as before. ([#40](https://github.com/paulm981/Submarine/issues/40))

## v0.4.0-fork.58

- **Typing with Broadcast on to many servers sends each key once.** Every key you type went to the app's backend once per server: 36 servers, 36 calls for each key. Now each key goes once, to all of them together, in order, so typing keeps up on Windows. ([#127](https://github.com/paulm981/Submarine/issues/127))

## v0.4.0-fork.57

- **Open terminal here.** Right-click a folder in SFTP to open a terminal in it: on the server, a new terminal that goes to that folder; on your computer, a Home terminal that starts there. ([#72](https://github.com/paulm981/Submarine/issues/72))

## v0.4.0-fork.56

- **The AI can ask for a server by name.** Say "check staging-api" while it isn't shared, and a card asks you **Share it?**: Share opens its tab and shares it once it's connected, under its own approval level. The AI never gets a list of your servers, and can't tell a name you don't have from one you said no to. ([#196](https://github.com/paulm981/Submarine/issues/196))

## v0.4.0-fork.55

- **The AI knows each server's OS,** from the badge Submarine already shows, so it writes commands for the right system. ([#193](https://github.com/paulm981/Submarine/issues/193))
- **"Try on web1 first" on a request for several servers.** It runs on one server, and you see its result before you allow the rest. ([#193](https://github.com/paulm981/Submarine/issues/193))

## v0.4.0-fork.54

- **Many terminals open without freezing the window.** Opening a Wall of 36 terminals, or starting a profile with many servers, held the window for seconds. Terminals now open a few at a time, the one you're looking at first, and share what they measure of the font, so the window keeps answering while the rest catch up. ([#127](https://github.com/paulm981/Submarine/issues/127))

## v0.4.0-fork.53

- **A new AI level: Run commands that only read, ask before changes.** The AI can look around a server without asking: `ps aux | grep nginx`, `df -h`, `journalctl -u nginx`, `systemctl status`, `docker logs`. Anything that changes things still asks, and so do root, copies, anything that reaches out (`curl`, `ping`) and reading files that usually hold secrets, such as `.env` or `.ssh`. Submarine decides from a list of programs it knows only read, never the AI. **Run anything** stays as it is, for lab servers. ([#190](https://github.com/paulm981/Submarine/issues/190))

## v0.4.0-fork.49

- **The AI tools' connection speaks the newest version of MCP, the protocol AI tools use (2026-07-28), and still the older ones.** Nothing changes in how you use it: AI tools built for the new version connect without the old opening exchange, and get the same tools and guidance. ([#177](https://github.com/paulm981/Submarine/issues/177))

## v0.4.0-fork.48

- **The AI can follow a log while it works.** It asks once to follow a command such as `journalctl -u nginx -f`, on the same card as any command, and then reads what's new every few seconds instead of waiting for the command to time out. The AI panel shows each follow while it runs, with Stop. It also stops after 30 minutes the AI doesn't read it, after 2 hours, or when access ends, and the AI gets the output with known secrets hidden. ([#179](https://github.com/paulm981/Submarine/issues/179))

## v0.4.0-fork.47

- **The AI knows up front what it may do on each server.** When it asks Submarine what's shared, it now hears, for each server, whether you're asked each time, only your allowed commands run, or anything runs, along with the folders it may copy into and the limits. It no longer has to find out by being refused, and it's told to ask again after a refusal, since you can change settings while it works. It never hears your allow rules. ([#176](https://github.com/paulm981/Submarine/issues/176))

## v0.4.0-fork.46

- **What a server prints reaches the AI marked as the server's words, not instructions.** A log line that tells the AI to run something is now clearly data to it: the output comes between two tags the server can't fake, with a line saying not to follow it, and Submarine's own lines, such as the exit code, stay outside. At **Run anything** and **Allow everything here**, Submarine now says plainly that text a server prints can steer the AI. ([#178](https://github.com/paulm981/Submarine/issues/178))

## v0.4.0-fork.44

- **The AI can copy files to and from many servers at once.** It asks once, on one card that shows what goes where, how many files and how big, and whether files already there are replaced. Files come to this computer only into a folder you add in Settings › AI tools, each server's in a folder of its own, and never replace what's there. ([#159](https://github.com/paulm981/Submarine/issues/159))

## v0.4.0-fork.42

- **Push and pull files on many servers at once.** Right-click a tab, or servers in the session tree, and pick Push files to servers… or Pull files from servers…. A summary shows what goes where and what's already there before anything moves: skip those or replace them, once for the whole run. Four servers copy at a time, each with its own progress row, and Stop stops them all. ([#159](https://github.com/paulm981/Submarine/issues/159))

## v0.4.0-fork.39

- **Settings is easier to find your way around.** It has tabs now: General, Terminal, Servers & sessions, AI tools, Sync and Help, and it opens on the one you used last. Each setting says what it does in one line; the longer explanations are behind Learn more. No setting works differently. ([#168](https://github.com/paulm981/Submarine/issues/168))

## v0.4.0-fork.38

- **Your sessions can survive a reconnect.** Turn on Keep sessions between connections in Settings › Servers & sessions (or for one server in its form): a terminal then runs inside tmux, or screen, on the server, so after a dropped connection, a sleep or a reconnect you're back where you were, with what was running still running. Nothing asks when you close a tab: the session just keeps running. Type exit to end it, or use Sessions… in the server's right-click menu to see, open or end them. Nothing is ever installed on a server; one without tmux or screen gets a plain shell and a note. ([#126](https://github.com/paulm981/Submarine/issues/126))

## v0.4.0-fork.36

- **Broadcast works as in the original project again.** The Broadcast button in the title bar opens a list: tick the sessions, turn it on, and what you type in any of them goes to all of them. The Wall is the original project's pinboard again: right-click a tab and pin its terminal there. The fork's own Walls are gone (saved walls, pausing a server, paste and Send to all, Ctrl+Shift+B), which keeps syncing with the original project simple. Walls you saved stay in your profile, unused. ([#162](https://github.com/paulm981/Submarine/issues/162))

## v0.4.0-fork.34

- **The AI can run one command on many servers, and you allow it once.** In a Broadcast of 12 servers, asking the AI to check disk space everywhere now shows one card, Allow on 12 servers, instead of 12. It runs without asking only where every server's settings say it may; otherwise every server waits on the one card. Allow everything on these servers until this AI tab closes covers them all. ([#158](https://github.com/paulm981/Submarine/issues/158))

## v0.4.0-fork.33

- **Remote Desktop works as in the original project again.** The fork's own Remote Desktop changes are gone, so syncing with the original project stays simple. Text copied on the server reaches your clipboard at once, a shared folder follows the server, and Remote Desktop Connection may get a saved login's password, as in the original project. Opening an RDP server through an SSH server works as before. ([#160](https://github.com/paulm981/Submarine/issues/160))

## v0.4.0-fork.32

- **Codex starts from Home's menu, beside Claude Code.** With AI tools on in Settings, Home's menu lists each AI tool you have installed. Codex opens already connected to Submarine, the same way: it never gets a key or a password, and Submarine asks you before each command it runs on a server. Settings › AI tools shows which are installed. ([#117](https://github.com/paulm981/Submarine/issues/117))

## v0.4.0-fork.31

- **Shared folders in Remote Desktop can't be left through a link, on macOS and Linux.** Every file the server opens, lists, renames or deletes is now reached from the shared folder one folder at a time, refusing any link on the way. Before, a link that appeared just after Submarine's check (from something else writing in the folder) would have been followed. ([#132](https://github.com/paulm981/Submarine/issues/132))

## v0.4.0-fork.30

- **A Wall of many terminals opens without freezing the window.** Going into Broadcast or a Wall used to fit every terminal to its tile in one go, which held the window for seconds with 36 servers. They now settle a few at a time, so the window keeps answering while the tiles fill in, usually within a second. ([#127](https://github.com/paulm981/Submarine/issues/127))

## v0.4.0-fork.23

- **Remote Desktop links are now RDP servers.** Each link becomes an RDP server in the same folder, with its name, address, user name and notes, and opens in your Remote Desktop app as before. ([#42](https://github.com/paulm981/Submarine/issues/42))
  - RDP servers can do more: open in the tab with the built-in client, share the clipboard or a folder, or go through an SSH server.
  - On Windows, the window now follows the server's RDP options (full screen unless you change them), not Remote Desktop's Default.rdp.
  - The MobaXterm import brings Remote Desktop sessions in as RDP servers, still without their saved login.
  - It syncs as a server. A computer still on an older version gets the RDP server in place of its link.

## v0.4.0-fork.22

### From the original project

- **Safer against a hostile server.** A server, or someone on the way to it, can no longer:
  - put files outside the folder you download to, or stop the app with a huge folder listing (their [#283](https://github.com/SinaXhpm/submarine/pull/283));
  - freeze the window with a few kilobytes of terminal output, or hold a login forever (their [#284](https://github.com/SinaXhpm/submarine/pull/284));
  - flood the app from a Remote Desktop or VNC tab (their [#286](https://github.com/SinaXhpm/submarine/pull/286));
  - take the window down with an odd value in its monitoring answers (their [#289](https://github.com/SinaXhpm/submarine/pull/289)).
  - ([#142](https://github.com/paulm981/Submarine/issues/142))
- **Shared profiles hold more for your OK.** A server someone else moved to another address asks before it connects, and its commands and forwards wait for your OK again. A sync that would empty a saved password or key waits for you too. A profile file imported from elsewhere comes in with everything held, and with no trusted host keys. (Their [#285](https://github.com/SinaXhpm/submarine/pull/285) and [#287](https://github.com/SinaXhpm/submarine/pull/287); [#142](https://github.com/paulm981/Submarine/issues/142))
- **Lock clears more.** Locking a profile now also empties forms that showed a password, and forgets what was kept for its tabs. (Their [#288](https://github.com/SinaXhpm/submarine/pull/288); [#142](https://github.com/paulm981/Submarine/issues/142))
- **Terminals survive a failed reconnect and a reload,** and a tool that fails shows an error in its own pane instead of taking the tab down. (Their [#288](https://github.com/SinaXhpm/submarine/pull/288); [#142](https://github.com/paulm981/Submarine/issues/142))
- **Faster:** hidden terminals rest, transfers no longer redraw the file lists, uploads keep more on the way, and a folder download lists several folders at once. (Their [#291](https://github.com/SinaXhpm/submarine/pull/291) and [#292](https://github.com/SinaXhpm/submarine/pull/292); [#142](https://github.com/paulm981/Submarine/issues/142))
- **Smaller fixes:** a terminal tab's close button no longer covers its name (their [#290](https://github.com/SinaXhpm/submarine/pull/290)), and the macOS icon has the rounded tile (their [#293](https://github.com/SinaXhpm/submarine/pull/293)). ([#142](https://github.com/paulm981/Submarine/issues/142))

## v0.4.0-fork.21

### From the original project

- **Open a profile automatically on Windows,** with no password or Windows Hello, when Submarine starts. It's a switch of its own under **Unlock with Windows Hello**, off unless you turn it on. Anyone using your Windows account can then open that profile, so it's for a computer that's yours alone. After **Lock** it takes the password again until Submarine restarts. (Their [#275](https://github.com/SinaXhpm/submarine/pull/275) and [#281](https://github.com/SinaXhpm/submarine/pull/281); [#142](https://github.com/paulm981/Submarine/issues/142))
  - **In the fork, never for a shared profile:** anyone using that Windows account would get everyone's saved logins. It can't be turned on for one, and sharing a profile turns it off. Windows Hello still works.
- **Security updates** for the libraries the app is built on. (Their [#273](https://github.com/SinaXhpm/submarine/pull/273) and [#282](https://github.com/SinaXhpm/submarine/pull/282); [#142](https://github.com/paulm981/Submarine/issues/142))

## v0.4.0-fork.20

### From the original project

- **Big bursts of output no longer bog down the window.** A command that prints faster than the terminal can draw (`cat` of a huge file, a binary by mistake) now waits in the app, about a second of drawing at a time. If it falls far behind, the oldest part is skipped, with a note in the terminal saying how much. **Ctrl+C** clears what's waiting, so the prompt comes back quickly. (Their [#263](https://github.com/SinaXhpm/submarine/pull/263); [#142](https://github.com/paulm981/Submarine/issues/142))
- **On-connect commands can run in every terminal of a tab.** A new box under a server's on-connect commands, **Also run them in each new terminal you open**, types them into every terminal of the tab, not only the first. In a shared profile, turning it on counts as a change you're asked about first, and you're asked once for the whole tab. (Their [#265](https://github.com/SinaXhpm/submarine/pull/265); [#142](https://github.com/paulm981/Submarine/issues/142))
- **SFTP Follow.** The file pane can follow the folder your terminal is in. (Their [#255](https://github.com/SinaXhpm/submarine/pull/255); [#142](https://github.com/paulm981/Submarine/issues/142))
- **Folders upload and download a few files at a time,** so a folder of many small files is much faster. Nothing on your computer is replaced unless you chose to, as before. (Their [#261](https://github.com/SinaXhpm/submarine/pull/261); [#142](https://github.com/paulm981/Submarine/issues/142))
- **An arrow beside the terminal tabs' "+"** opens its menu (split, a terminal with its own login), which used to need a right-click. (Their [#253](https://github.com/SinaXhpm/submarine/pull/253); [#142](https://github.com/paulm981/Submarine/issues/142))
- **The app does less while you're not using it.** The cursor stops blinking 10 seconds after you last typed or clicked, and the status dots stop pulsing after a few seconds. (Their [#262](https://github.com/SinaXhpm/submarine/pull/262); [#142](https://github.com/paulm981/Submarine/issues/142))
- **Smaller fixes:**
  - Selected text in a terminal is visible again. (Their [#252](https://github.com/SinaXhpm/submarine/pull/252))
  - A slow port forward or tunnel no longer holds up a new terminal or file transfer on the same server. (Their [#260](https://github.com/SinaXhpm/submarine/pull/260))
  - A mirror's start no longer downloads what it just uploaded. (Their [#259](https://github.com/SinaXhpm/submarine/pull/259))
  - A refused Remote Desktop login says why. (Their [#258](https://github.com/SinaXhpm/submarine/pull/258))
  - The terminal takes your keys again after a reconnect. (Their [#271](https://github.com/SinaXhpm/submarine/pull/271))
  - Closing a connected tab no longer calls every connection SSH, and deleting a server forgets this computer's Remote Desktop and VNC settings for it. (Their [#272](https://github.com/SinaXhpm/submarine/pull/272))
  - On a server without SFTP, reading a file can no longer go past its end into running commands. (Their [#269](https://github.com/SinaXhpm/submarine/pull/269))
  - On Linux, programs the app starts (your browser, file manager, editor, ProxyCommand) no longer inherit the app's own graphics workarounds. (Their [#266](https://github.com/SinaXhpm/submarine/pull/266))
  - ([#142](https://github.com/paulm981/Submarine/issues/142))

## v0.4.0-fork.19

- **The Mac app, for a release you've tried.** Every release still comes out for Windows and Linux. Once one has been tried, one click on GitHub builds the Mac app from that same release and adds it to the release's downloads. A Mac build costs ten times the GitHub minutes, so they go only to releases that work. The Mac app is for Apple silicon (M1 or later); it doesn't run on an Intel Mac, which the README wrongly said it did. ([#140](https://github.com/paulm981/Submarine/issues/140))

## v0.4.0-fork.18

- **The graphics card draws the terminals you can see.** It can draw only about 16 terminals at once. In a Wall of 36, the terminals that kept it used to be simply the last ones opened, hidden tabs included, while tiles on screen drew the slower way. Now at most 12 terminals use it, and a terminal on screen comes first: it takes a free place, or one from a terminal you can't see. Switching between a few tabs keeps theirs. **Copy diagnostics** shows how many are in use. ([#127](https://github.com/paulm981/Submarine/issues/127))
- **Check that a download came from this project.** Each release file now carries a signed record of the repository, workflow and commit it was built from. The release notes and the README show the one command that checks it. ([#48](https://github.com/paulm981/Submarine/issues/48))
- **Fixes for setting up sync through a folder or git repository.**
  - Opening another profile while setup is still running no longer links that profile instead, and no longer undoes its own sync settings.
  - Two computers that set up sync before the folder had copied either one's files now find each other at their next sync and share one folder. Before, each kept syncing only with itself.
  - A computer on an older version no longer clears a server's color scheme or highlight set on your other computers.
  - ([#50](https://github.com/paulm981/Submarine/issues/50))

## v0.4.0-fork.17

### From the original project

- **Search and history on Ctrl+Shift+F and Ctrl+Shift+R.** In a terminal, the search now opens on **Ctrl+Shift+F** and the command history on **Ctrl+Shift+R** (Cmd+Shift on a Mac). Plain Ctrl+F and Ctrl+R go to the shell again: Ctrl+R is its own history search, and Ctrl+F pages forward in less and vim. The keys work on any keyboard layout. F5 and Ctrl+Shift+R no longer reload the app and drop your sessions. (Their [#238](https://github.com/SinaXhpm/submarine/pull/238); [#121](https://github.com/paulm981/Submarine/issues/121))
- **The built-in Remote Desktop tab gets its own tools.**
  - A connection mark (Good, Fair, Weak or Not answering, with the delay) opens a panel with the figures.
  - **Settings** has the speed and quality choices, quick sets of them, a resolution, and a pause of the picture while the tab is hidden.
  - **Files** chooses what of this computer the server gets. A share chosen there goes only to the server's address at the time, as from the server's form.
  - Choices made in the tab are kept for that server on this computer.
  - The tab's bar fits a narrow split pane.
  - (Their [#244](https://github.com/SinaXhpm/submarine/pull/244) and [#246](https://github.com/SinaXhpm/submarine/pull/246); [#121](https://github.com/paulm981/Submarine/issues/121))
- **The VNC tab shares the clipboard, and gets the same tools.**
  - Text copied on the server comes to this computer while you're looking at its tab. Otherwise it waits in a note in the tab for you to copy.
  - A paste key in the tab puts your text on the server's clipboard.
  - Only plain text (ASCII) is shared; the tab says why when text can't go. The clipboard is on by default; you can turn it off per server.
  - Like the Remote Desktop tab, VNC gets a connection mark, a **Settings** panel and a pause while the tab is hidden, which is on by default.
  - A busy screen no longer falls seconds behind, and resizing the window works more reliably.
  - A Remote Desktop or VNC server that stops taking what you send, or floods the connection, now ends its own session. Before, its tab froze and closing it left the connection open, or it could use up the app's memory.
  - (Their [#247](https://github.com/SinaXhpm/submarine/pull/247) and [#249](https://github.com/SinaXhpm/submarine/pull/249); [#121](https://github.com/paulm981/Submarine/issues/121))

## v0.4.0-fork.16

### From the original project

- **"Save password" when you type a password.** When a server has no saved password, or its saved one is wrong, the password prompt has a **Save password** tick. Ticked, the password is saved once the login with it works: to the server, to its saved login if it uses one, or to the jump host it was asked for. This covers SSH, jump hosts, the built-in Remote Desktop client and VNC.
  - The tick starts off when the login also tried keys, or when the profile is shared. In a shared profile, a line under the tick says everyone in it gets the password.
  - When the saved login is used by other servers too, a line under the tick names them.
  - A VNC password is saved only into the server itself: there's no tick when a VNC server has a saved login.
  - Nothing is saved if the server was changed while you logged in, for example by someone else in a shared profile. Only the first password a server asks for is offered for saving, and only when the login name is the server's saved one.
  - A saved Remote Desktop or VNC password that was refused isn't tried again on reconnect.
  - (Their [#241](https://github.com/SinaXhpm/submarine/pull/241); [#121](https://github.com/paulm981/Submarine/issues/121))
- **Remote Desktop Connection gets the server's saved password.** On Windows, Remote Desktop Connection is given the server's own saved password, encrypted for your Windows account, in a file that's deleted once the app has read it. A saved login's password is never given to it. The server's form has a password field for an RDP server with its own login. A server opened in the RDP app through an SSH server keeps the same local address each time, so what the app remembers for it is found again. **Open again** doesn't send the password once the tab's connection through the SSH server has closed. (Their [#242](https://github.com/SinaXhpm/submarine/pull/242); [#121](https://github.com/paulm981/Submarine/issues/121))

## v0.4.0-fork.15

- **Less lag with many tabs and in a Broadcast.**
  - Terminals in tabs you can't see no longer draw their output while it comes in. With many busy tabs open, that drawing slowed the terminal in front to a few frames a second, so what you typed showed up late. Hidden terminals still take in all their output and show it as soon as you look.
  - In a Broadcast, each key costs less: the Wall no longer redraws every tile per key, and terminals report back less often while little output comes in.
  - Output highlighting now has one time limit shared by all terminals, so many terminals at once can't slow the window down.
  - Diagnostics no longer report "the window was stuck" when it was only hidden, minimized, or the computer was asleep.
  - ([#127](https://github.com/paulm981/Submarine/issues/127))

## v0.4.0-fork.14

### From the original project

- **Speed and quality for Remote Desktop (built-in client) and VNC.** A server's form has **Speed and quality**. For RDP you can choose the colours, desktop effects, the network type and whether to offer RemoteFX. For VNC you can choose the encoding, the colours and the most updates per second. The choices are kept on this computer only. A server you don't change connects exactly as before. (Their [#233](https://github.com/SinaXhpm/submarine/pull/233); [#121](https://github.com/paulm981/Submarine/issues/121))
- **The built-in Remote Desktop client can share the clipboard and files.**
  - **Clipboard:** shared unless you turn off **Share the clipboard** in the server's form. Text copied on the server comes to your clipboard only while you're looking at its tab. If you're elsewhere, it waits in a note in that tab for you to copy. This stops a server from swapping what you're about to paste into a terminal. Your text goes to the server only when you paste it there. Only text is shared, never files.
  - **Files:** off until you choose one folder, or a whole drive on Windows, in the server's form. You can make it read only.
  - **Where files are kept safe:**
    - Every path the server asks for is checked so it can't reach outside what you shared.
    - Even with all of `/` shared on Linux, the system's own files that hold running programs' memory and settings stay out of reach.
    - A server can have at most 1,024 files open at once.
    - Files go only to the server address you chose them for: if someone in a shared profile changes the address, nothing is shared until you save the form again.
  - These settings stay on this computer: they never sync with a shared profile, and phones don't have them.
  - (Their [#234](https://github.com/SinaXhpm/submarine/pull/234), with the fork's checks from [#131](https://github.com/paulm981/Submarine/issues/131); [#121](https://github.com/paulm981/Submarine/issues/121))
- **A new terminal takes the lowest free number.** Close terminal 2 of 1, 2 and 3, open another, and it's called 2 again, not a second 3. (Their [#235](https://github.com/SinaXhpm/submarine/pull/235); [#121](https://github.com/paulm981/Submarine/issues/121))
- **A terminal can log in on its own.** Right-click the **+** button for **New terminal with its own login**, or **Always for this server**. That terminal logs in to the server again instead of sharing its tab's connection. This helps with software that ties rights to the login, such as CA Access Control's `sesu`. It uses the password you typed for the tab and the host key you accepted there, and asks nothing new. If that login fails, the terminal uses the tab's connection and says so. (Their [#236](https://github.com/SinaXhpm/submarine/pull/236); [#121](https://github.com/paulm981/Submarine/issues/121))

## v0.4.0-fork.13

- **vim no longer freezes the terminal.** Opening a file in vim used to stop that terminal for good: nothing more showed up and typing did nothing, so you had to close it and log in again. nano and less were fine. It came from how the app was built: vim asks the terminal whether the cursor blinks, and the built copy of the terminal code failed on that question. The app is now built so that code works, and every build checks for this kind of fault before it can ship. ([#129](https://github.com/paulm981/Submarine/issues/129))

## v0.4.0-fork.12

### From the original project

- **Locking your profile calls off logins still in progress.** A connection that was still logging in, waiting on the server or on a question, no longer comes up after the profile is locked. (Their [#237](https://github.com/SinaXhpm/submarine/pull/237); [#121](https://github.com/paulm981/Submarine/issues/121))

## v0.4.0-fork.11

- **Home terminals end more tidily.** "The shell has ended" now comes after the shell's last output, not before it. A terminal that couldn't start is marked ended, so closing Submarine doesn't count it as a running shell. On Windows, lines wrap and redraw as Windows' own console does when you resize the window. Fast output after a reconnect is paced correctly, and a burst of output no longer leaves its memory in use afterwards. ([#52](https://github.com/paulm981/Submarine/issues/52))
- **Ping a server from the session list.** Right-click a server in the tree or the grid, then **Ping**. A small panel shows each answer and how long it took, when the server stopped answering and when it came back, and whether its SSH port answers yet, so you know when you can log in. A short, quiet sound plays when it does; you can turn it off in **Settings › Servers**. It checks every 2 seconds until the server answers, then every 30 seconds. You can ping several servers at once (more than 10 asks first), and closing a server's card stops its pings. The check is a plain connection to the SSH port, so it works where ping is blocked and logs in nowhere. A server behind a jump host is checked from the jump host, over a connection already open to it; without one, the panel says to open a terminal first. ([#33](https://github.com/paulm981/Submarine/issues/33))
- **Choose the Info panel's text size.** It's in **Settings › UI Customization**, from 10 to 16 px; it was always 11 px. The panel's smaller labels and icons grow with it. The setting stays on this computer. ([#56](https://github.com/paulm981/Submarine/issues/56))
- **A server whose jump host was deleted never connects directly without asking.** Deleting a jump host says how many servers use it, for example "3 servers use web-gw as their jump host. They won't connect until you pick another jump host or choose to connect directly." Opening one of them then says "db-01's jump host (web-gw) was deleted, so Submarine won't connect it directly without asking", with **Connect directly** and **Pick a jump host**; saving the server's form counts as choosing too. It's the same when the jump host was deleted on another device of a shared profile. Monitoring and Ping don't check such a server, and **Resume all** in Monitoring skips it and goes on with the rest. ([#51](https://github.com/paulm981/Submarine/issues/51))

## v0.4.0-fork.10

- **Choose how much the AI may run without asking, per server.** There are three levels: **Ask every time** (the default, and how it worked before), **Run my allowed commands, ask for the rest**, and **Run anything, just log it**. Set one on a server or a folder from its right-click menu in the session tree (**AI approval…**), or for every server in **Settings › AI tools**. A server's own setting wins over its folder's, and a folder's over the one for every server. It's kept on this computer only: it never syncs with a shared profile, and the AI can't change it. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **Levels and allowed commands stay on this computer, even in a profile file.** An exported or imported profile, or one copied by hand, arrives with every server at **Ask every time** and no allowed commands; its always-ask list comes along. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **A teammate's change that lets the AI do more waits for your OK.** In a shared profile, if someone moves a server, or a folder it's in, somewhere the AI may do more here, adds a server there, or changes where a server the AI may run commands on connects to, it shows in **Waiting for your OK**, for example "Someone moved db-01 into Lab; there the AI may run anything without asking." Until you choose, every command on that server asks. **Allow** lets the new setting apply; **Keep as it was** keeps the server at **Ask every time** here. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **Commands you always allow, and commands that always ask.** `uptime` allows exactly that, `systemctl status *` allows it with any arguments, and `cat /var/log/*` allows any file in that folder, but not a second file or a path out of it with `..`. An allowed command must be one simple command: anything with `;`, `&&`, `|`, `$(`, a backtick, a redirect or a new line still asks you, and so does `%` or `^` on a Windows server. A command with sudo, su, doas, dzdo, pkexec, sudoedit, run0, runuser or `machinectl shell`, however it's written, asks unless the server is at **Run anything**. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **The always-ask list beats every allow rule and Run anything.** It finds its words anywhere in a command, so `reboot` also catches `sudo reboot`, `uptime; reboot`, `re''boot`, `re$@boot`, `/sbin/reb?ot` and `shutdown.exe`, and `rm -rf *` also catches `rm -fr` and `rm --recursive --force`. A command that builds the name as it runs can still get past it. New profiles start with `reboot`, `shutdown`, `rm -rf *`, `dd *`, `format`, `Restart-Computer` and a few more on it, and you can remove any of them. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **New on the request's card:** **Always allow this command here** saves the command, or a pattern you edit it into, to the server's list, and allows it. A pattern for a command that runs other commands, such as `timeout *` or `bash *`, warns you by name and takes one more click. **Allow everything here until this AI tab closes** runs the rest without asking, until the AI tab closes, the server stops being shared, you end it, or you change the server's level. **Deny with a note** tells the AI why, so it can try another way. Each takes a click, never a key. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **You can see when the AI runs commands without asking.** A shared server's tab and the AI pane say "allowed commands run" or "runs without asking", with a click to change it. The AI panel marks such commands "ran without asking", and says why. The audit log records how each command was allowed. At most four commands from one AI tab run at once on a server. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **The taskbar flashes when the AI needs you** and Submarine isn't the window in front. You can turn it off in **Settings › AI tools**. ([#115](https://github.com/paulm981/Submarine/issues/115))
- **The AI can ask to run a command as root, and never sees the password.** It asks Submarine, instead of typing sudo itself. The request's card says "on **web-01** as **root** (via sudo)". If the server wants a password, the card asks you for it, and Submarine gives it only to sudo's or dzdo's own prompt, as SFTP root mode does. A wrong password counts as one try, and the card asks again. ([#120](https://github.com/paulm981/Submarine/issues/120))
- **A root password you type for the AI is kept for that AI tab and server only.** It's forgotten when access ends: when the AI tab or the server's tab closes, you stop sharing or press Stop, the server reconnects, AI tools are turned off, or the profile locks. It's not shared with the tab's own SFTP root mode or Info panel, either way, and the AI never rides on a sudo you typed in your own terminal. It's never in the output, the audit log, or anything the AI gets. It's only as safe as the server account, though: a program running as that account could catch it. A sudoers rule that needs no password for the commands you want is safer. ([#120](https://github.com/paulm981/Submarine/issues/120))
- **Root passwords only go to a sudo or dzdo that belongs to root.** In SFTP root mode, the Info panel, the Docker tab and for the AI, Submarine finds sudo or dzdo in the system's own folders, not wherever the login's PATH points, and types the password only if it and its folder are owned by root and nobody else can change them. Otherwise it says so and sends nothing. sudo is asked in English, so a wrong password counts as one try whatever language the server speaks, and its question is answered only when nothing else came first. ([#120](https://github.com/paulm981/Submarine/issues/120))
- **Root requests follow your approval levels.** At **Ask every time** and **Run my allowed commands**, every root request asks. At **Run anything**, it runs without asking once you've typed the password for that AI tab, or where the server needs none. The always-ask list still asks. The AI panel and the audit log mark root requests. ([#120](https://github.com/paulm981/Submarine/issues/120))

## v0.4.0-fork.9

- **Claude Code starts without your SSH agent and cloud logins in its environment, in a folder of its own.** It used to start in your home folder, where it reads files without asking, `~/.ssh` included; now it starts in `ai-workspace` in Submarine's data folder. Its `SSH_AUTH_SOCK` points where no agent is, on Windows too, and the usual cloud and GitHub logins (AWS, Azure, Google Cloud, GitHub and a few more) are taken out. Ordinary Home terminals keep them, and Claude Code's own login stays, even through AWS, Google Cloud or Azure. It still runs as you, so ssh run by it can still use an agent named in `~/.ssh/config`, a connection your own terminal keeps open, or a key file with no passphrase. **Settings › AI tools** says so, and gives the Claude Code settings that keep it out of your keys. ([#116](https://github.com/paulm981/Submarine/issues/116))
- **One question per command.** Claude Code no longer asks before it uses Submarine's tools: Submarine asks before every command on a server, and Claude Code still asks before anything it does on your computer. ([#116](https://github.com/paulm981/Submarine/issues/116))
- **The request's card says who a command runs as:** "on **web-01** as **root**", or whoever the tab logged in as. A tab logged in as root is marked as root for every command, as a command with sudo, dzdo, su, doas or pkexec already was. Where those need no password, nothing asks first, and Submarine can't always tell. The audit log names the user too. ([#116](https://github.com/paulm981/Submarine/issues/116))
- **The keyboard goes into the AI pane only when you put it there:** with a click in its terminal, or by opening the pane with a key on its own button. Opening or folding it any other way, with a click, with **Ctrl+Shift+L** or by starting Claude Code beside a tab, leaves the keyboard where it was, so typing meant for the server can't answer the AI tool's questions. The terminal with the keyboard is bright, and the other dims a little. ([#116](https://github.com/paulm981/Submarine/issues/116))
- **Settings › AI tools says where output goes.** With AI tools on, a note says that the output you allow, and the files the AI tool reads on this computer, go to the AI provider. ([#116](https://github.com/paulm981/Submarine/issues/116))

## v0.4.0-fork.8

- **The host keys this app trusts always come first.** A key listed in `~/.ssh/known_hosts` only spares you the first-time question. If a server's key has changed, you get the warning even when known_hosts lists the new key. If known_hosts lists a different key for a server the app doesn't know yet, you get the changed-key warning instead of the first-time question. A server certificate can't slip in a different key either: if the key inside it isn't the one the app trusts, you get the same changed-key warning, and if you accept, that key is saved in place of the old one. A certificate that doesn't check out is refused, whatever you answer. A key marked `@revoked` there is always refused, by Monitoring too. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **A key known_hosts vouches for is remembered.** The first time, it's saved to the app's trusted keys, as if you had approved it, so Monitoring can check that server too. ([#43](https://github.com/paulm981/Submarine/issues/43))
- **Monitoring says when a certificate is all that vouches for a server.** Such a server (a Teleport node, say) isn't checked, and its card says why. Once you've accepted the key in its certificate in a terminal, Monitoring checks it. ([#43](https://github.com/paulm981/Submarine/issues/43))
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
