# Smoke Testing in Docker

A throwaway container for exercising `gw` against real files without
polluting your host `/tmp` or accumulating leftover snapshots, dirty git
trees, or half-applied edits.

The in-process integration tests (`cargo test`) already use `tempfile`
and clean up after themselves. This environment is purely for **manual**
exploration: scribble a fixture, run a command, observe, and exit. The
filesystem is RAM-backed and discarded on container exit, so every
session starts from zero.

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/)
- [just](https://just.systems/) (optional, the recipes wrap raw `docker` invocations)

## Recipes

| Recipe                  | What it does                                                                 |
| ----------------------- | ---------------------------------------------------------------------------- |
| `just docker-build`     | Build the `grepwrite-smoke:latest` image (multi-stage, slim runtime).        |
| `just docker-smoke`     | Drop into an interactive shell with a fresh tmpfs `/workspace`.              |
| `just docker-smoke-demo`| Run the pre-canned demo script (find → rewrite → undo) in a fresh container. |
| `just docker-fixture P` | Mount host path `P` at `/workspace` for cases where you want persistence.    |

## Image contents

- `gw` (built from the current source tree)
- `git`
- `ripgrep` (`rg`)
- `ast-grep` (`ast-grep` and the `sg` alias)
- `bash`

Built on `debian:bookworm-slim`. The Rust toolchain lives only in the
builder stage and is not shipped in the runtime image.

## Interactive smoke

```bash
just docker-build         # one-time, ~3-5 minutes
just docker-smoke         # interactive shell
```

Inside the shell:

```bash
git init -q
git config user.email you@example.com
git config user.name you
echo 'TODO: docs' > a.md
git add -A && git commit -q -m initial
gw find TODO -o caveman
gw rewrite TODO DONE --apply
gw snapshots
gw undo
exit                      # /workspace is gone, container is gone
```

## Demo script

`just docker-smoke-demo` runs `docker/smoke.sh`. Expected tail of the
output:

```
=== rewrite --apply ===
a.md:1
b.md:1
=== snapshots ===
<snapshot id>  <timestamp>  rewrite TODO -> DONE
=== undo ===
restored 2 files from snapshot <id>
=== verify restored ===
TODO: write the docs
TODO: tests
```

(Exact formatting depends on the current `gw` version; the meaningful
checks are: `find` reports two matches, `rewrite --apply` reports two
written files, `snapshots` lists one entry, and the final `cat` shows
the original `TODO:` lines back in place.)

## Persistent fixture

If you want to keep the working tree around between runs, mount a host
directory instead of using tmpfs:

```bash
just docker-fixture ./scratch
```

`./scratch` is mounted read-write at `/workspace`. Anything you do
inside the container persists on the host. Caveat: that means it can
also pollute the host, which is the thing this environment exists to
avoid — use only when you actually need persistence.

## Notes

- The image is intentionally minimal — runtime stage carries no Rust toolchain. Current size is roughly 250 MB, dominated by `ast-grep` and `git` helpers.
- The Dockerfile builder uses a recent stable Rust (`rust:1.90-bookworm`); `Cargo.toml`'s `rust-version` (currently `1.88`) is the project floor, not a builder pin. Don't bump the builder to `rust:latest` without thinking.
- Nothing in the standard build flow (`cargo build`, `cargo test`,
  `just pre-commit`) depends on Docker. It is purely optional infra.
