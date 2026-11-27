# Agent Entry Point (`paulm981/Submarine`)

All coding, fork-footprint, security, performance, changelog, and release rules for this repository live in [CLAUDE.md](CLAUDE.md) (single source of truth) and [FORK.md](FORK.md).

Before starting any task (whether using Claude Code, Antigravity, or another agent):

1. Read `CLAUDE.md`, `FORK.md`, and the latest comment on [issue #163](https://github.com/paulm981/Submarine/issues/163).
2. Always pass `-R paulm981/Submarine` to `gh` commands whenever `upstream` (`SinaXhpm/submarine`) is configured as a git remote.
3. Check active work (`gh pr list -R paulm981/Submarine` and open issues) and claim your issue before editing so two agents never duplicate or collide on a branch.
4. Keep fork changes out of upstream files whenever possible (`scripts/upstream-footprint.sh`), preserve CRLF line endings on `src-tauri/src/lib.rs`, and run the local verification suite listed in `CLAUDE.md` before committing or merging.
5. Never trigger a release build (`.github/workflows/build.yml` with `publish=true`) without explicit owner approval; queue unreleased changes in `changelog.d/<issue>.md`.
