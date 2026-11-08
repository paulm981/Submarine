# Working on this repository

## Every change goes issue → pull request → release

1. **An issue first.** Every change has an issue: an existing one, or a new one opened before the work starts. The issue says what and why; a design that grows goes in the issue, so it's kept.
2. **A pull request that closes it.** The pull request's body starts with `Closes #N` (one line per issue it finishes; `Part of #N` when it does only some of one). GitHub links them and closes the issue on merge. One pull request may close several small issues that don't touch the same code.
3. **A changelog line** for what someone using the app will notice, ending with the issue's link, e.g. `([#45](https://github.com/paulm981/Submarine/issues/45))`. It goes in a file of its own, `changelog.d/<issue>.md` (see its README), not in CHANGELOG.md: two pull requests then never change the same lines, and the release's own commit never conflicts with one that's open. Never guess a version number: every merge to main is a release and takes the next number.
4. **The release** is made by the merge itself (`.github/workflows/build.yml`): it's numbered with the original project's version and the fork's count, such as `v0.4.0-fork.N`, and its page lists the merged pull requests, each linking its issues. So a pull request's title is written for the person using the app, not for the code.

Changes only to `.md` files (docs, this file, the changelog), `docs/` (screenshots) or `tools/screenshots/` don't make a release.

When the merge is released, the release job puts the released commit's `changelog.d` lines under that release's number in CHANGELOG.md and deletes the files (`scripts/changelog_release.py`), in a `.md`-only commit on main; a file merged meanwhile waits for its own release. Only if that step couldn't push (the release job says so) does the next pull request do it by hand.

## Fork features that don't cost sync time

Every line the fork changes inside one of the original project's files can conflict at the next sync; files only the fork has cost nothing. So ([#163](https://github.com/paulm981/Submarine/issues/163)):

- **A sync comes first.** When the original project has changes the fork doesn't, bring them in before any other work (FORK.md, "Keeping up with the original project").
- **New features go in files of the fork's own.** The original project's files get one-line hooks only, each checked by a test (`fork_hooks.rs`), so a sync that drops one fails instead of quietly losing the feature.
- **Where the original project has a feature, the fork uses theirs.** Replacing one of theirs needs the owner's OK in its issue first: the fork's Walls, in place of their Broadcast, were thousands of lines in their busiest files.
- **Every pull request says what it does to the footprint:** `scripts/upstream-footprint.sh` before and after (the lines the fork changes in the original project's files). It should only go down, or stay as it is.
- **One pull request at a time,** its checks green before it merges.

## How we work

- **Start from the plan.** What's done and what's next is in [#163](https://github.com/paulm981/Submarine/issues/163)'s latest comment; read it first, and update it when a batch of work ends.
- **What the owner cares about most:**
  - the AI (Claude Code, Codex) working well, with as few approval prompts as safety allows;
  - Broadcast to many servers;
  - pushing and pulling files on many servers, by the owner and by the AI;
  - sessions that come back after a reconnect, without ever asking "close session?" or "close connection?";
  - Settings in categories;
  - staying close to the original project, so syncs stay cheap.
- **Decide, don't ask.** The owner can't weigh design options. Make the recommendation from these goals, act on it, and say why in the issue or pull request. Ask first only for:
  - replacing one of the original project's features;
  - a change in how the app behaves that the owner would notice (e.g. what "Run anything" allows);
  - anything that can't be undone.
- **Pictures and words follow the app.** A pull request that changes what someone sees retakes its screenshots in the same pull request (`node tools/screenshots/take.mjs <name>`, which every picture in `docs/screenshots/fork/` comes from) and updates README and FORK.md, alt text included. Look at each retaken picture before it goes up.
- **Merge when green.** Once every check passes on the current head, merge; don't wait for the owner. Say plainly in each pull request what wasn't tried in the running app, and keep a short checklist for the owner to try.
- **No custom Remote Desktop.** RDP is the original project's code. The only fork addition worth having there would be jump-host support, and that needs its own issue first.
- **Security first, honestly.** The AI never gets keys or passwords, and the approval card is the real guard. Never claim prompt injection is solved: a model can be talked round by text it reads, so protections that matter are enforced by Submarine, not left to the model.
- **Helpers' work is checked before it goes up.** Read the risky parts yourself: save payloads, path checks, anything security-related, effect order. Re-run every check. Look at screenshots. Code that only builds on Windows, macOS or Android is reviewed by eye, and only the CI job for that system proves it.
- **The working branch.** After a squash merge, start the next branch from `origin/main`, then `git merge -s ours` the old tip of the working branch, so the push is a fast-forward. No force-push and no deleting remote branches. Each task gets its own worktree, removed once it's merged.

## Speed, security and Rust patterns

Every change is held to these, in review and in the code itself:

**Security first.**
- Everything from a server, a sync, an import or the AI is untrusted. Check it at the edge: lengths, counts, paths, names. Give it a limit, and refuse what doesn't fit, rather than trying to repair it.
- Secrets live in `Zeroizing` and are never logged. A token is compared in constant time, and goes only where it's meant to go (an environment variable, not a command line or a file).
- `unsafe` only for FFI, with a `// SAFETY:` comment saying why each call is sound, and tests that drive it.
- Paths from outside are walked without following links, or checked with `guard_local_path`, before anything is opened.

**Speed.**
- Nothing blocking on an async worker: SQLite, file I/O and other synchronous work go in `spawn_blocking`. Never hold a `std::sync::Mutex` guard across an `.await`.
- Anything that can grow from outside has a bound: channels, queues, buffers, output, retries.
- Nothing heavy per keystroke or per frame in the page: batch it and coalesce it, and measure before and after when a change is about speed.
- One command for a batch rather than one per item, across the Tauri bridge or SSH.

**Well-known Rust patterns, not anti-patterns.**
- Errors go up with `?`. No `unwrap`, `expect` or panic on anything that can fail at run time outside tests. A Tauri command returns an error; it never panics.
- Borrow rather than clone on a hot path. `&str` and `&[T]` in, owned out only where it's kept.
- A value that must be one thing is a `static`, not a `const`: each use of a `const` can be a copy of its own.
- Types say what's valid: an enum, not a string or bool flag; parse once at the edge.
- Small functions with one job, tested where they're decided. No new clippy warnings (`.github/clippy-warnings`): fix the warning, don't `allow` it, unless the line is the original project's.
- Follow the code around it. The original project's style holds in their files, so a sync stays small.

## Before pushing

- Rust, in `src-tauri`: `cargo test --locked` and `cargo clippy --locked --lib` with no more warnings than `.github/clippy-warnings` allows. The tests need a stand-in page: `mkdir -p dist && [ -f dist/index.html ] || echo '<!doctype html><title>Submarine</title>' > dist/index.html`.
- Frontend: `npm run typecheck` and `npm run build`.
- `src-tauri/src/lib.rs` keeps CRLF line endings on every line, as the original project has it: `grep -c $'\r$'` must equal `wc -l`. An editor or tool that writes LF there makes every line conflict at the next merge.
- Syncing with the original project: FORK.md's "Keeping up with the original project".
- The pull request template's checklist, including saying plainly when something wasn't tried in the app.

## Writing

The changelog, issues, pull requests and the app's own words are plain English for the people who use the app: short sentences, what they'll notice first, the detail after.
