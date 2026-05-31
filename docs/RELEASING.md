# Releasing grepwrite

## Versioning policy

`grepwrite` follows [SemVer](https://semver.org/). While the project is in the `0.x` series, breaking changes ship in **minor** bumps (per SemVer's 0.x rules); the **major** version is reserved as a deliberate signal and is only bumped manually.

| Bump  | When                                                                |
|-------|---------------------------------------------------------------------|
| patch | Bug fixes, doc-only changes, internal refactors, in-progress builds |
| minor | New features, larger checkpoints, breaking changes while in `0.x`   |
| major | Reserved for the `1.0.0` cut and any subsequent major bump          |

During pre-`0.1.0` development, expect frequent **patch** bumps as modules land and an occasional **minor** bump to mark a meaningful checkpoint (e.g. "find pipeline end-to-end", "rewrite + undo working").

## Prerequisites

```bash
cargo install cargo-edit   # provides `cargo set-version`
brew install git-cliff     # or: cargo install git-cliff
brew install just          # or: cargo install just
```

## Patch or minor release

```bash
just release-patch   # 0.0.X
just release-minor   # 0.X.0
```

Each recipe:

1. Bumps `Cargo.toml` (and `Cargo.lock`) via `cargo set-version`.
2. Regenerates `CHANGELOG.md` via `git cliff --tag vX.Y.Z`.
3. Commits the bump as `chore(release): vX.Y.Z`.
4. Creates an annotated tag `vX.Y.Z`.

Push with:

```bash
git push && git push --tags
```

## Major release

There is intentionally no `just release-major` recipe. Major bumps require the manual procedure below so they are always a conscious decision rather than a side effect of an automated recipe:

```bash
cargo set-version --bump major
git cliff -o CHANGELOG.md --tag "v$(cargo pkgid | sed -E 's/.*[#@]([0-9]+\.[0-9]+\.[0-9]+.*)$/\1/')"
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore(release): v1.0.0"
git tag -a v1.0.0 -m "v1.0.0"
git push && git push --tags
```

## Publishing to crates.io

(Not yet enabled — added once the CLI surface stabilizes around `0.1.x`.)

When ready:

```bash
cargo publish --dry-run
cargo publish
```

## Changelog format

The changelog is generated from [Conventional Commits](https://www.conventionalcommits.org/) by [`git-cliff`](https://git-cliff.org/). See [`cliff.toml`](../cliff.toml) for the parser configuration.

Preview unreleased entries without writing:

```bash
just changelog-preview
```
