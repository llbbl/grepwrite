# grepwrite

> Ripgrep-style search plus safe, transactional, AST-aware rewrites with built-in undo. Designed first for LLM coding agents, useful for humans.

**Binary:** `gw` &nbsp;·&nbsp; **Status:** v0.1.x — `find` / `rewrite --apply` / `undo` / `snapshots` working end-to-end

`gw` is what `rg --replace` would be if it actually wrote files. It wraps [ripgrep](https://github.com/BurntSushi/ripgrep) (and optionally [ast-grep](https://ast-grep.github.io/)) for search, then owns the mutation path: dry-run by default, atomic per-file writes, and every `--apply` automatically snapshots to a git ref so `gw undo` restores the exact pre-edit state.

## Why

The existing options for "regex search-and-rewrite across a file tree" are all bad in different ways:

- `sed -i` — BSD/GNU divergence, no `.gitignore` awareness, no preview, no rollback, no AST scoping.
- `rg -r` — preview only. Files are never touched. Workaround: pipe filenames to `sed -i`.
- `ast-grep` — capable, but its UX is general-purpose, not LLM-shaped, and its multi-file apply story is thin.
- `perl -i -pe` — works, opaque, 1995.

LLM coding agents currently chain `rg` + `sed` + read-after-write to verify. Every link adds tokens, latency, and a failure mode. `gw` collapses the chain.

## Status

All four verbs are wired end-to-end:

- `gw find <pattern> [path]` — regex via [ripgrep](https://github.com/BurntSushi/ripgrep); `--in function|class|imports|comments` switches to [ast-grep](https://ast-grep.github.io/) for AST-scoped search.
- `gw rewrite <pattern> <replacement> [path]` — dry-run by default; `--apply` requires a clean git tree (or `--force`) and writes atomically per file under a git-ref snapshot.
- `gw undo [--snapshot <id|name>]` — restores the last (or named) snapshot, refusing to clobber user edits made on top of `gw`'s output.
- `gw snapshots` — lists snapshots, newest first.

Output formats: `compact` (default; rg-style for `find`, unified diff for `rewrite`), `caveman` (LLM-token-minimal `path:line`), `json` (stable schema v1), `diff` (unified, `rewrite` only).

## Build

```bash
just build   # or: cargo build
just test    # or: cargo test
just         # list all recipes
```

Requires Rust 1.88+ and [just](https://just.systems/). See [`docs/RELEASING.md`](docs/RELEASING.md) for the release procedure.

## Smoke Testing

For ephemeral, Docker-based manual exploration that doesn't pollute your host, see [`docs/SMOKE.md`](docs/SMOKE.md) (`just docker-build`, `just docker-smoke`, `just docker-smoke-demo`).

## Debugging

Set `GW_LOG` to enable development logging (off by default):

```bash
GW_LOG=info gw find foo
GW_LOG=debug gw rewrite foo bar --apply
```

Log levels: `error`, `warn`, `info`, `debug`, `trace`. Output is plain-text on stderr, kept separate from user-facing errors (which always prefix with `gw:`).

## Credits

The `caveman` output format (`gw find -o caveman` / `gw rewrite -o caveman`) takes its name from [JuliusBrussee/caveman](https://github.com/JuliusBrussee/caveman), a JS-side LLM-token-minimal output utility. `gw`'s caveman format is a Rust-side reinterpretation (`path:line` per match, frozen post-1.0); the implementation and exact output shape are grepwrite-specific.

## License

MIT. See [`LICENSE`](LICENSE).
