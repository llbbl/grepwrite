# grepwrite

> Ripgrep-style search plus safe, transactional, AST-aware rewrites with built-in undo. Designed first for LLM coding agents, useful for humans.

**Binary:** `gw` &nbsp;·&nbsp; **Status:** pre-implementation scaffold (v0.0.1)

`gw` is what `rg --replace` would be if it actually wrote files. It wraps [ripgrep](https://github.com/BurntSushi/ripgrep) (and optionally [ast-grep](https://ast-grep.github.io/)) for search, then owns the mutation path: dry-run by default, atomic per-file writes, and every `--apply` automatically snapshots to a git ref so `gw undo` restores the exact pre-edit state.

## Why

The existing options for "regex search-and-rewrite across a file tree" are all bad in different ways:

- `sed -i` — BSD/GNU divergence, no `.gitignore` awareness, no preview, no rollback, no AST scoping.
- `rg -r` — preview only. Files are never touched. Workaround: pipe filenames to `sed -i`.
- `ast-grep` — capable, but its UX is general-purpose, not LLM-shaped, and its multi-file apply story is thin.
- `perl -i -pe` — works, opaque, 1995.

LLM coding agents currently chain `rg` + `sed` + read-after-write to verify. Every link adds tokens, latency, and a failure mode. `gw` collapses the chain.

## Status

Scaffold only. The CLI surface (`gw find`, `gw rewrite`, `gw undo`, `gw snapshots`) is wired up; the locate / mutate / snapshot / output modules ship as stubs and are filled in module-by-module. Track progress in the repo's task list.

## Build

```bash
just build   # or: cargo build
just test    # or: cargo test
just         # list all recipes
```

Requires Rust 1.85+ and [just](https://just.systems/). See [`docs/RELEASING.md`](docs/RELEASING.md) for the release procedure.

## Credits

The `caveman` output format (`gw find -o caveman` / `gw rewrite -o caveman`) takes its name from [JuliusBrussee/caveman](https://github.com/JuliusBrussee/caveman), a JS-side LLM-token-minimal output utility. `gw`'s caveman format is a Rust-side reinterpretation (`path:line` per match, frozen post-1.0); the implementation and exact output shape are grepwrite-specific.

## License

MIT. See [`LICENSE`](LICENSE).
