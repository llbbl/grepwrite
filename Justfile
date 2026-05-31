# grepwrite — task runner. Run `just` to list recipes.

default:
    @just --list

# Build the gw binary (debug)
build:
    cargo build

# Build the gw binary (release)
build-release:
    cargo build --release

# Run all tests
test:
    cargo test

# Run a single test by name substring
test-one PATTERN:
    cargo test {{PATTERN}}

# Type-check without producing artifacts
check:
    cargo check

# Lint: clippy with warnings as errors + rustfmt check
lint:
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

# Format the codebase in place
fmt:
    cargo fmt

# Full pre-commit gate
pre-commit: fmt lint test

# Run the gw binary locally
run *ARGS:
    cargo run -- {{ARGS}}

# Regenerate the full CHANGELOG.md from git history
changelog:
    git cliff -o CHANGELOG.md

# Preview unreleased changelog entries (does not write)
changelog-preview:
    git cliff --unreleased

# Bump patch version (0.0.X) and tag a release (agents allowed)
release-patch:
    @echo "Bumping patch version..."
    cargo set-version --bump patch
    just _finalize-release

# Bump minor version (0.X.0) and tag a release (agents allowed)
release-minor:
    @echo "Bumping minor version..."
    cargo set-version --bump minor
    just _finalize-release

# Internal: regenerate changelog and create a signed tag for the current Cargo version.
_finalize-release:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(cargo pkgid | sed -E 's/.*[#@]([0-9]+\.[0-9]+\.[0-9]+.*)$/\1/')
    echo "Finalizing release v${VERSION}"
    git cliff -o CHANGELOG.md --tag "v${VERSION}"
    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit -m "chore(release): v${VERSION}"
    git tag -a "v${VERSION}" -m "v${VERSION}"
    echo "Tagged v${VERSION}. Push with: git push && git push --tags"

# NOTE: there is intentionally no release-major recipe.
# Major bumps are reserved to the project owner — see docs/RELEASING.md.
