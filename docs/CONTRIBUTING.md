# Contributing to grepwrite

## Building from source

```bash
just build         # or: cargo build
just build-release # or: cargo build --release
just               # list all recipes
```

Requires Rust 1.88+ (the project's MSRV, set in `Cargo.toml`'s
`rust-version` field) and [just](https://just.systems/).

See [`RELEASING.md`](RELEASING.md) for the release procedure.

## Running tests

```bash
just test          # or: cargo test
just test-one NAME # cargo test NAME — substring match
```

The suite is ~114 tests, split between in-process unit tests and
integration tests that shell out to `gw` against ephemeral `tempfile`
directories. Tests that need `rg`, `ast-grep`, or `git` invoke them as
subprocesses; if a binary is missing from `PATH`, the corresponding
tests skip rather than fail.

## Linting and formatting

```bash
just fmt           # cargo fmt
just lint          # cargo clippy --all-targets -- -D warnings  +  cargo fmt --check
just pre-commit    # fmt + lint + test — the full local gate
```

`just lint` treats every clippy warning as an error. CI runs the same
gate, so a clean local `just pre-commit` is a reliable predictor.

## Debugging

`gw` keeps user-facing output (always prefixed with `gw:`) on stdout and
development logging on stderr. Logging is off by default. Enable it
with the `GW_LOG` environment variable:

```bash
GW_LOG=info  gw find foo
GW_LOG=debug gw rewrite foo bar --apply
GW_LOG=trace gw undo
```

Levels: `error`, `warn`, `info`, `debug`, `trace`. Output is plain text
on stderr so it never pollutes machine-readable stdout (e.g. `-o json`,
`-o caveman`).

## Smoke testing

For manual exploration against real files without polluting your host
filesystem, there's a Docker-based smoke environment with `gw`, `rg`,
`ast-grep`, and `git` preinstalled and a RAM-backed `/workspace`:

```bash
just docker-build       # one-time
just docker-smoke       # interactive shell
just docker-smoke-demo  # pre-canned find -> rewrite -> undo demo
```

See [`SMOKE.md`](SMOKE.md) for full details, including the persistent-fixture mode.

## Releasing

Versioning follows [SemVer](https://semver.org/). Patch releases are
cut with `just release-patch`, minor releases with `just release-minor`.
Both recipes bump `Cargo.toml`, regenerate `CHANGELOG.md` from git
history via `git cliff`, tag, and push.

See [`RELEASING.md`](RELEASING.md) for the full procedure, including
the crates.io publish step and the post-release verification checklist.

## Credits

The `caveman` output format (`gw find -o caveman` /
`gw rewrite -o caveman`) takes its name from
[JuliusBrussee/caveman](https://github.com/JuliusBrussee/caveman), a
JS-side LLM-token-minimal output utility. `gw`'s caveman format is a
Rust-side reinterpretation (`path:line` per match, frozen post-1.0);
the implementation and exact output shape are grepwrite-specific.

Search and AST scoping are built on
[ripgrep](https://github.com/BurntSushi/ripgrep) and
[ast-grep](https://ast-grep.github.io/) — `gw` invokes them as
subprocesses and inherits their semantics.
