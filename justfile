# Contributor commands. See README.md for the user-facing `kog` usage.
#
# `app/dist` must exist before `cargo build`/`cargo test` — the CLI embeds it
# at compile time (see crates/kog-cli/build.rs). `just build` and
# `just install` handle that; if you're only touching Rust and already have
# a `dist`, `cargo build`/`cargo test` alone are enough.
#
# The `release-*` recipes need `dist` (cargo-dist) on PATH:
# https://github.com/axodotdev/cargo-dist/releases, or `cargo install
# cargo-dist --locked`. Nothing else does.

# Build the page, then the release binary that embeds it.
build:
    cd app && bun install && bun run build
    cargo build --release

# Run the whole test suite.
test:
    cargo test --workspace

# Format and lint; fails on any warning, same as CI.
lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

# Build and install `kog` to ~/.cargo/bin, ready to run from any project.
install: build
    cargo install --path crates/kog-cli

# Rebuild the page on every change, for iterating on `app/src`. This talks
# to Vite's dev server directly — it is not what `kog` runs; the
# installed binary always serves the embedded, built page.
dev:
    cd app && bun install && bun run dev

# What a release would contain, without releasing anything.
release-plan:
    dist plan

# Build this machine's release artifact — archive, checksum, licences — into
# target/distrib, exactly as CI does. The one way to find out that a release
# is broken before pushing a tag rather than after.
release-build: build
    dist build --artifacts=local

# Rewrite .github/workflows/release.yml from dist-workspace.toml and
# build-setup.yml. Required after touching either: the workflow is generated,
# and dist silently drops step keys it does not know (see build-setup.yml).
release-ci:
    dist generate
