# grepwrite

[![crates.io](https://img.shields.io/crates/v/grepwrite.svg)](https://crates.io/crates/grepwrite)
[![downloads](https://img.shields.io/crates/d/grepwrite.svg)](https://crates.io/crates/grepwrite)
[![license](https://img.shields.io/crates/l/grepwrite.svg)](LICENSE)

> Ripgrep-style search plus safe, transactional, AST-aware rewrites with built-in undo. Designed first for LLM coding agents, useful for humans.

**Binary:** `gw`

## Install

```bash
cargo install grepwrite
```

(Homebrew formula in progress. Prebuilt binaries: planned.)

## Usage

### `gw find <pattern> [path]`

Locate matches via ripgrep, or via ast-grep with `--in function|class|imports|comments`.

```text
$ gw find TODO src/
src/lib.rs
1:4: // TODO: rename foo to bar

src/main.rs
2:8:     // TODO: wire up cli
```

### `gw rewrite <pattern> <replacement> [path]`

Preview by default; `--apply` writes atomically with a git snapshot.

```text
$ gw rewrite 'foo' 'bar' src/ -o diff       # preview
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
-// TODO: rename foo to bar
-pub fn foo() -> u32 {
+// TODO: rename bar to bar
+pub fn bar() -> u32 {
     42
 }
...

$ gw rewrite 'foo' 'bar' src/ --apply       # write
...
applied (snapshot: 2026-05-31T19-52-42-9ceabf)
```

### `gw undo [--snapshot <id|name>]`

Restores files exactly, refusing to clobber edits you made on top.

```text
$ gw undo
undone: 2026-05-31T19-52-42-9ceabf (2 files restored)
```

### `gw snapshots`

Lists snapshots, newest first.

```text
$ gw snapshots
2026-05-31T19-52-42-9ceabf  2026-05-31T19:52:42Z  3 edits  -
```

### Output formats

`compact` (default — rg-style for `find`, unified diff for `rewrite`), `caveman` (LLM-token-minimal `path:line` per match), `json` (stable schema v1), `diff` (unified, `rewrite` only).

## Why

`gw` wraps two excellent existing tools — [ripgrep](https://github.com/BurntSushi/ripgrep) for regex search and [ast-grep](https://ast-grep.github.io/) for structural search — and adds the missing piece: a safe, transactional rewrite layer with built-in undo.

ripgrep deliberately stops at preview: `rg -r` prints what it would write without writing. The conventional workaround is to chain `rg` with `sed -i` and a read-after-write check, which works but is brittle in scripts and especially hostile to LLM coding agents — three tools, three failure modes, tokens spent reconciling each step.

`gw` collapses that chain into one binary: locate, plan, dry-run by default, and atomic per-file writes wrapped in a git-ref snapshot when you commit to `--apply`. The locate layer is unchanged — it's just ripgrep and ast-grep underneath — so search semantics are exactly what you already know.

## Documentation

- [Contributing & build instructions](docs/CONTRIBUTING.md) — build, test, lint, debug, credits.
- [Smoke testing in Docker](docs/SMOKE.md) — ephemeral fixtures for manual exploration.
- [Release procedure](docs/RELEASING.md) — versioning policy and release recipes.

## License

MIT. See [LICENSE](LICENSE).
