use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use mycelium_graph::{build_graph, TypeScriptExtractor};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "mycelium", version, about = "Map a codebase into a graph")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a project and emit its file/import graph.
    Scan {
        /// Project root to scan.
        root: PathBuf,
        /// Where to write the graph. Defaults to `graph.json`.
        #[arg(short, long, default_value = "graph.json")]
        output: PathBuf,
        /// Print the statistics only, write no file.
        #[arg(long)]
        stats_only: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan {
            root,
            output,
            stats_only,
        } => scan(root, output, stats_only),
    }
}

fn scan(root: PathBuf, output: PathBuf, stats_only: bool) -> Result<()> {
    // Fail loudly on an unreadable root: everything else is recoverable.
    if !root.is_dir() {
        bail!("{} is not a readable directory", root.display());
    }
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot canonicalize {}", root.display()))?;

    let extractor = TypeScriptExtractor::new(&root);
    let graph = build_graph(&root, &extractor);
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
    // capped (see `mycelium_graph::MAX_DIAGNOSTICS`), so the two can
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

    if !stats_only {
        let json = serde_json::to_string_pretty(&graph)?;
        std::fs::write(&output, json)
            .with_context(|| format!("cannot write {}", output.display()))?;
        println!("written                {}", output.display());
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
        let output = dir.path().join("graph.json");

        let err = scan(missing.clone(), output, false).unwrap_err();

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
        let output = dir.path().join("graph.json");

        let err = scan(file.clone(), output, false).unwrap_err();

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
    fn a_valid_scan_succeeds_and_writes_a_graph_with_one_resolved_edge() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), r#"import { b } from "./b";"#).unwrap();
        fs::write(root.join("src/b.ts"), "export const b = 1;").unwrap();
        let output = dir.path().join("graph.json");

        scan(root, output.clone(), false).unwrap();

        assert!(output.exists(), "scan should have written {output:?}");
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
    /// `mycelium_graph::build_graph`) errors with `InvalidData` on a file
    /// that is not valid UTF-8, which a `.ts` file full of raw non-UTF-8
    /// bytes reliably triggers. A genuine tree-sitter *parse* failure could
    /// not be constructed this way from outside the crate — tree-sitter is
    /// fault-tolerant and recovers from any malformed source a test could
    /// plausibly hand it (see
    /// `mycelium_graph::extractors::typescript::tests::a_syntactically_broken_file_still_yields_what_it_can`
    /// and `mycelium_graph::graph::tests::an_import_into_a_file_that_fails_to_parse_produces_no_dangling_edge`,
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

        scan(root, output.clone(), false).expect("an unreadable file must not be fatal");

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
    fn stats_only_writes_no_file() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "export const a = 1;").unwrap();
        let output = dir.path().join("graph.json");

        scan(root, output.clone(), true).unwrap();

        assert!(
            !output.exists(),
            "--stats-only must not write an output file, found {output:?}"
        );
    }
}
