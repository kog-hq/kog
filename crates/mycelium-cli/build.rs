//! `rust-embed` bakes `app/dist` into the binary at compile time (see
//! `src/main.rs`). That directory only exists after the web page has been
//! built, and a missing folder makes `rust-embed`'s macro fail with a bare
//! "file not found" that names no fix. Check for it ourselves and, if it's
//! absent, fail with the exact command that produces it.

use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is always set by cargo when running a build script");
    // crates/mycelium-cli -> repo root -> app/dist
    let dist_dir = PathBuf::from(&manifest_dir).join("../../app/dist");
    let index_html = dist_dir.join("index.html");

    // Re-embed on every rebuild of the page, not just the first build: a
    // missing `rerun-if-changed` here is the failure mode most likely to
    // waste hours later, silently serving a stale bundle after `app` changes.
    println!("cargo:rerun-if-changed={}", dist_dir.display());

    if !index_html.exists() {
        eprintln!();
        eprintln!("error: app/dist/index.html not found");
        eprintln!();
        eprintln!("mycelium-cli embeds the web UI (app/dist) into the binary at compile");
        eprintln!("time via rust-embed, so the page must be built before `cargo build`.");
        eprintln!();
        eprintln!("Fix:");
        eprintln!("    cd app && bun install && bun run build");
        eprintln!();
        eprintln!("then re-run the cargo build.");
        eprintln!();
        std::process::exit(1);
    }
}
