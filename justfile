# Contributor commands. See README.md for the user-facing `mycelium` usage.
#
# `app/dist` must exist before `cargo build`/`cargo test` — the CLI embeds it
# at compile time (see crates/mycelium-cli/build.rs). `just build` and
# `just install` handle that; if you're only touching Rust and already have
# a `dist`, `cargo build`/`cargo test` alone are enough.

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

# Build and install `mycelium` to ~/.cargo/bin, ready to run from any project.
install: build
    cargo install --path crates/mycelium-cli

# Rebuild the page on every change, for iterating on `app/src`. This talks
# to Vite's dev server directly — it is not what `mycelium` runs; the
# installed binary always serves the embedded, built page.
dev:
    cd app && bun install && bun run dev
