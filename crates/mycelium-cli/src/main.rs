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

    if !stats_only {
        let json = serde_json::to_string_pretty(&graph)?;
        std::fs::write(&output, json)
            .with_context(|| format!("cannot write {}", output.display()))?;
        println!("written                {}", output.display());
    }

    Ok(())
}
