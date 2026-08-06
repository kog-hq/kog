use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use kog_graph::{build_graph, Graph, TypeScriptExtractor};
use std::path::PathBuf;

mod server;

#[derive(Parser)]
#[command(
    name = "kog",
    version,
    about = "Map a codebase into a graph, and view it"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug, PartialEq)]
enum Command {
    /// Scan a project and print its file/import graph statistics.
    ///
    /// Writes nothing unless `-o`/`--output` is given.
    Scan {
        /// Project root to scan. Defaults to the current directory.
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Write the graph as JSON to this path.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Scan a project and serve the graph in a browser.
    ///
    /// This is what bare `kog` runs. Never writes a file: the graph is
    /// held in memory and served from the binary's embedded page.
    View {
        /// Project root to view. Defaults to the current directory.
        #[arg(default_value = ".")]
        root: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Bare `kog` (no subcommand at all) is explicitly `view .` — typing
    // a path is optional everywhere, and typing a subcommand is optional
    // too. This is a deliberate default, not clap falling through to one.
    let command = cli.command.unwrap_or(Command::View {
        root: PathBuf::from("."),
    });
    match command {
        Command::Scan { root, output } => scan(root, output),
        Command::View { root } => view(root),
    }
}

/// Validate and canonicalise a project root. Shared by `scan` and `view`: an
/// unreadable root is fatal for both, in the same way and with the same
/// message.
fn canonicalize_root(root: PathBuf) -> Result<PathBuf> {
    if !root.is_dir() {
        bail!("{} is not a readable directory", root.display());
    }
    root.canonicalize()
        .with_context(|| format!("cannot canonicalize {}", root.display()))
}

fn scan_root(root: &std::path::Path) -> Graph {
    let extractor = TypeScriptExtractor::new(root);
    build_graph(root, &extractor)
}

fn scan(root: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let root = canonicalize_root(root)?;
    let graph = scan_root(&root);
    let stats = &graph.stats;

    println!("root                   {}", root.display());
    println!("files discovered       {}", stats.files_discovered);
    println!("files parsed           {}", stats.files_parsed);
    println!("specifiers total       {}", stats.specifiers_total);
    println!("  internal             {}", stats.specifiers_internal);
    println!("    resolved           {}", stats.resolved);
    println!("    unresolved         {}", stats.unresolved);
    println!("    excluded           {}", stats.excluded);
    println!(
        "  external             {} ({} distinct packages)",
        stats.external_specifiers, stats.external_packages_distinct
    );
    println!("nodes                  {}", graph.nodes.len());
    println!("edges                  {}", graph.edges.len());
    println!("resolution rate        {:.4}", stats.resolution_rate);

    if !stats.failures.is_empty() {
        println!("failures               {}", stats.failures.len());
        for failure in stats.failures.iter().take(10) {
            println!("  {} — {}", failure.path, failure.reason);
        }
        if stats.failures.len() > 10 {
            println!("  … {} more", stats.failures.len() - 10);
        }
    }

    // `unresolved` and `excluded` are the exact totals; `diagnostics` is
    // capped (see `kog_graph::MAX_DIAGNOSTICS`), so the two can
    // diverge on a badly broken repo. Report both rather than let the
    // printed count silently understate the real one.
    let non_resolved_total = stats.unresolved + stats.excluded;
    if non_resolved_total > 0 {
        if stats.diagnostics.len() < non_resolved_total {
            println!(
                "diagnostics            {} (capped; {} total)",
                stats.diagnostics.len(),
                non_resolved_total
            );
        } else {
            println!("diagnostics            {}", stats.diagnostics.len());
        }
        for diagnostic in stats.diagnostics.iter().take(10) {
            println!(
                "  {}:{} [{}] {}",
                diagnostic.path, diagnostic.line, diagnostic.kind, diagnostic.specifier
            );
        }
        if stats.diagnostics.len() > 10 {
            println!("  … {} more", stats.diagnostics.len() - 10);
        }
    }

    if let Some(output) = output {
        let json = serde_json::to_string_pretty(&graph)?;
        std::fs::write(&output, json)
            .with_context(|| format!("cannot write {}", output.display()))?;
        println!("written                {}", output.display());
    }

    Ok(())
}

/// Scan `root` and serve the resulting graph, on 127.0.0.1 only, on an
/// OS-assigned free port. Never writes a file.
fn view(root: PathBuf) -> Result<()> {
    let root = canonicalize_root(root)?;
    let graph = scan_root(&root);
    let graph_json = serde_json::to_string(&graph)?;

    // Port 0 asks the OS for whatever's free; a hardcoded 4173/5173 could
    // already be in use. 127.0.0.1 only — this serves the user's
    // source-code structure and has no business being reachable off-host.
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|e| anyhow::anyhow!("failed to start local server: {e}"))?;
    let port = match server.server_addr().to_ip() {
        Some(addr) => addr.port(),
        None => bail!("local server did not bind to an IP address"),
    };
    let url = format!("http://127.0.0.1:{port}");

    println!(
        "{} nodes, {} edges from {}",
        graph.nodes.len(),
        graph.edges.len(),
        root.display()
    );
    // Printed before opening the browser: this is the one line that still
    // works over SSH, or if the browser fails to launch.
    println!("{url}");

    if let Err(e) = open::that(&url) {
        eprintln!("warning: could not open a browser automatically: {e}");
    }

    for request in server.incoming_requests() {
        let decision = server::route(request.url(), &graph_json);
        if let Err(e) = server::respond(request, decision) {
            eprintln!("warning: failed to respond to a request: {e}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Design doc §7: an unreadable root is the *one* fatal error this layer
    /// is contractually required to get right. Asserting on the message
    /// content (not just `is_err()`) is deliberate — a bare `is_err()` would
    /// still pass if the `bail!` message were emptied or genericised, which
    /// defeats the point of this test existing at all.
    #[test]
    fn a_missing_root_is_a_named_readable_directory_error() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");

        let err = scan(missing.clone(), None).unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains(&missing.display().to_string()),
            "error message should name the offending path, got {message:?}"
        );
        assert!(
            message.contains("not a readable directory"),
            "error message should say the path is not a readable directory, got {message:?}"
        );
    }

    /// The `is_dir()` guard must reject a regular file, not just a path that
    /// does not exist at all.
    #[test]
    fn a_file_passed_as_root_is_also_rejected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("not-a-directory.txt");
        fs::write(&file, "hello").unwrap();

        let err = scan(file.clone(), None).unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains(&file.display().to_string()),
            "error message should name the offending path, got {message:?}"
        );
        assert!(
            message.contains("not a readable directory"),
            "error message should say the path is not a readable directory, got {message:?}"
        );
    }

    #[test]
    fn a_valid_scan_with_output_writes_a_graph_with_one_resolved_edge() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), r#"import { b } from "./b";"#).unwrap();
        fs::write(root.join("src/b.ts"), "export const b = 1;").unwrap();
        let output = dir.path().join("graph.json");

        scan(root, Some(output.clone())).unwrap();

        assert!(output.exists(), "scan -o should have written {output:?}");
        let contents = fs::read_to_string(&output).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&contents).expect("written output should parse as valid JSON");

        assert_eq!(json["stats"]["resolution_rate"], 1.0);
        assert_eq!(json["edges"].as_array().unwrap().len(), 1);
    }

    /// A file that cannot be read (as opposed to one that fails to parse)
    /// must be recorded in `stats.failures` and must not make the scan
    /// fatal. Design doc §7 groups "will not parse" and "will not resolve"
    /// together as always-recoverable; a read failure is the same category
    /// one step earlier (the file never even became source text to hand the
    /// extractor).
    ///
    /// This is a *genuine* failure reachable through the public API, not a
    /// simulated one: `std::fs::read_to_string` (used internally by
    /// `kog_graph::build_graph`) errors with `InvalidData` on a file
    /// that is not valid UTF-8, which a `.ts` file full of raw non-UTF-8
    /// bytes reliably triggers. A genuine tree-sitter *parse* failure could
    /// not be constructed this way from outside the crate — tree-sitter is
    /// fault-tolerant and recovers from any malformed source a test could
    /// plausibly hand it (see
    /// `kog_graph::extractors::typescript::tests::a_syntactically_broken_file_still_yields_what_it_can`
    /// and `kog_graph::graph::tests::an_import_into_a_file_that_fails_to_parse_produces_no_dangling_edge`,
    /// which resorts to a test-only `Extractor` wrapper for that reason).
    #[test]
    fn a_file_that_cannot_be_read_does_not_make_the_scan_fatal() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "export const a = 1;").unwrap();
        // Invalid UTF-8: `std::fs::read_to_string` fails on this with
        // `ErrorKind::InvalidData`, exercising `build_graph`'s "read failed"
        // branch through the real filesystem, not a mock.
        fs::write(root.join("src/bad.ts"), [0xff, 0xfe, 0xfd]).unwrap();
        let output = dir.path().join("graph.json");

        scan(root, Some(output.clone())).expect("an unreadable file must not be fatal");

        let contents = fs::read_to_string(&output).unwrap();
        let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let failures = json["stats"]["failures"].as_array().unwrap();
        assert!(
            failures.iter().any(|f| f["path"] == "src/bad.ts"
                && f["reason"].as_str().unwrap().contains("read failed")),
            "expected a recorded failure for src/bad.ts, got {failures:?}"
        );
    }

    #[test]
    fn scan_writes_nothing_unless_output_is_given() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "export const a = 1;").unwrap();

        scan(root.clone(), None).unwrap();

        assert!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(|e| e.ok())
                .all(|e| e.path().starts_with(root.join("src"))),
            "scan without -o must write nothing into the project directory"
        );
    }

    /// `ROOT` on `scan` defaults to the current directory — asserted at the
    /// clap layer, since exercising it through `scan()` would mean changing
    /// the test process's actual working directory.
    #[test]
    fn scan_root_defaults_to_the_current_directory() {
        let cli = Cli::try_parse_from(["kog", "scan"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Scan {
                root: PathBuf::from("."),
                output: None,
            })
        );
    }

    /// `ROOT` on `view` defaults to the current directory too.
    #[test]
    fn view_root_defaults_to_the_current_directory() {
        let cli = Cli::try_parse_from(["kog", "view"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Command::View {
                root: PathBuf::from("."),
            })
        );
    }

    /// Bare `kog`, with no subcommand at all, must resolve to `view .`
    /// — this is the whole feature. Parsing alone leaves `command: None`;
    /// `main` is what applies the default, so this test exercises the
    /// clap-level half of that contract.
    #[test]
    fn bare_invocation_parses_with_no_subcommand() {
        let cli = Cli::try_parse_from(["kog"]).unwrap();
        assert_eq!(cli.command, None);
    }

    #[test]
    fn scan_with_output_short_flag_parses() {
        let cli = Cli::try_parse_from(["kog", "scan", "some/dir", "-o", "g.json"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Scan {
                root: PathBuf::from("some/dir"),
                output: Some(PathBuf::from("g.json")),
            })
        );
    }
}
