# Screenshots

The pictures of the fork's features in `docs/screenshots/fork/` come from the real page, run in a browser. The backend's calls are answered by Tauri's own mock (`@tauri-apps/api/mocks`) with made-up servers and addresses from the documentation ranges, so nothing here is a real server or a real person's data.

```sh
node tools/screenshots/take.mjs                      # every picture
node tools/screenshots/take.mjs bulk-push ai-levels  # some of them
```

It needs the app's `npm ci` and Playwright with Chromium (`require("playwright")` must work). Nothing here is part of the app's build.

- `vite.config.mjs`: the app's own Vite config, with `mocks.js` loaded before `src/main.tsx`.
- `mocks.js`: the canned answers. A call with none returns null, and `take.mjs` lists those calls after each picture.
- `take.mjs`: opens the page at 1400×900 (2× pixels, larger where a screen needs it), clicks to each screen, and saves the picture. Every picture in `docs/screenshots/fork/` comes from here: a change to what the app shows retakes its pictures in the same pull request (CLAUDE.md).

## Timing many terminals (#127)

```sh
node tools/screenshots/perf.mjs [servers=36]
```

The same page and mocks, with that many more servers opening by themselves (`mock-servers` in localStorage), whose shells echo what is typed. It opens them, pins each to the Wall, switches between the Wall and a tab, turns Broadcast on and types 40 keys to every server, and closes every tab, and prints, for each step, how long the page was busy in long tasks (50 ms or more) and the longest one. `PROFILE=1` adds where the time went when the Wall opens; `SHOT=<file>.png` saves the Wall and a tab. The numbers are this machine's headless Chromium, not a user's webview: compare a run before a change with one after, on the same machine.

## Timing a terminal's input and output

```sh
node tools/screenshots/perf-io.mjs                   # every measure, 5 runs each, medians
node tools/screenshots/perf-io.mjs output input      # some of: output, stream, input, broadcast
JSON=before.json node tools/screenshots/perf-io.mjs output stream   # on main, before a change
BASE=before.json node tools/screenshots/perf-io.mjs output stream   # after it: fails past +25% busy time
CHECK=1 node tools/screenshots/perf-io.mjs input broadcast          # fails when a key takes more calls than perf-io.budget.json
ROOT=/path/to/the/original/project node tools/screenshots/perf-io.mjs output stream input   # the original project's page, same mocks
```

The same page and mocks, with one terminal fed made-up output (`corpus.mjs`: plain text, a log, text a program colored, `ls --color`, `yes`, a `top`-like redraw) the way the backend hands it over: 256 KiB batches, no more than 4 MB not yet drawn. It prints how long a burst took to draw, how long the page's main thread was at work for it ("busy ms", the number to compare), its longest hold and how many frames it drew; with highlighting off, on, and on with three rules of your own. Then typing: from a key going down to the write call, how many calls a key takes, and how long until its echo is drawn, in one terminal and with Broadcast to 36 servers.

`PROFILE=1` says which functions the time went to, `PARTS=1` splits it into script, layout and styles, `TIMERS=1` says who set timers meanwhile, `KINDS=log,ls` and `MB=40` pick the output, `GPU=1` draws with WebGL (in software here, so only its script time means anything).

What it can't show: the bridge between the page and the backend (each call and each batch of output crosses it in the real app), the backend's own pacing (a key's echo waits there for its 8 ms flush window; `cargo test --lib measure_ -- --ignored --nocapture` times that), SSH, and a real GPU. The times are this machine's: compare a run with another on the same machine. The counts are the same everywhere.

What's checked by itself, in the tests every pull request runs:

- `tools/test/perf.test.ts`: a key is one call in the same turn of the page with no timer on its way, Broadcast's is one call for every server, a key's echo is never held back by highlighting, and highlighting's work on each kind of output (runs of the regex engine, bytes handed to xterm) stays within a budget written there. Counts, so a busy CI machine can't fail them. Two look at the clock, with budgets several times under today's speed.
- `src-tauri/src/term_output.rs` and `local_term.rs`: the backend's filter and send do 12 MB/s or more in a test build (ten times that today), and a Home terminal's small output goes to the page at once.
