# syntax=docker/dockerfile:1.7

# ---------- Stage 1: builder ----------
# Cargo.toml's rust-version (currently 1.88) is the project floor for
# downstream consumers; the builder uses a more recent known-good stable
# so CI catches new lints early. Bump deliberately, not to `latest`.
FROM rust:1.90-bookworm AS builder

WORKDIR /build

# Copy manifests first to leverage layer caching for deps.
COPY Cargo.toml Cargo.lock ./

# Pre-fetch dependencies (cached when Cargo.* are unchanged).
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/gw* target/release/gw*

# Copy real sources and build the release binary.
COPY src ./src
RUN cargo build --release

# ---------- Stage 2: runtime ----------
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="grepwrite-smoke" \
      org.opencontainers.image.description="Ephemeral smoke-testing environment for the gw CLI." \
      org.opencontainers.image.source="https://github.com/llbbl/grepwrite" \
      org.opencontainers.image.licenses="MIT"

# Runtime deps:
#   git       - required by gw snapshots
#   ripgrep   - rg backend
#   curl      - used to fetch ast-grep installer
#   ca-certificates - TLS for the installer
#   unzip     - ast-grep release artifact is a zip
# ast-grep is installed from its official GitHub release for arch portability;
# bookworm's apt does not ship ast-grep, and `cargo install` would balloon the
# runtime image with a toolchain.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      git ripgrep curl ca-certificates unzip \
 && ARCH="$(uname -m)" \
 && case "$ARCH" in \
        x86_64)  AST_TARGET="x86_64-unknown-linux-gnu" ;; \
        aarch64) AST_TARGET="aarch64-unknown-linux-gnu" ;; \
        *) echo "unsupported arch: $ARCH" >&2; exit 1 ;; \
    esac \
 && AST_VER="0.39.5" \
 && curl -fsSL -o /tmp/ast-grep.zip \
      "https://github.com/ast-grep/ast-grep/releases/download/${AST_VER}/app-${AST_TARGET}.zip" \
 && unzip -j /tmp/ast-grep.zip -d /tmp/ast-grep \
 && install -m 0755 /tmp/ast-grep/ast-grep /usr/local/bin/ast-grep \
 && ln -s /usr/local/bin/ast-grep /usr/local/bin/sg \
 && rm -rf /tmp/ast-grep /tmp/ast-grep.zip \
 && apt-get purge -y --auto-remove curl unzip \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/gw /usr/local/bin/gw

WORKDIR /workspace

CMD ["bash"]
