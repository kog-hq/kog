use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use kog_graph::{
    render, scan_workspace, Answer, Atlas, FileStatus, Project, Question, Workspace, DEFAULT_LIMIT,
    QUESTION_FORMS,
};
use std::path::PathBuf;

mod mcp;
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
        /// Also write GraphML here, for Gephi, yEd or Cytoscape.
        #[arg(long, value_name = "FILE")]
        graphml: Option<PathBuf>,
        /// Also write Cypher here, to load into Neo4j with `cypher-shell`.
        #[arg(long, value_name = "FILE")]
        cypher: Option<PathBuf>,
        /// Also write the graph as YAML here.
        #[arg(long, value_name = "FILE")]
        yaml: Option<PathBuf>,
        /// Also write the numbers as a Markdown report here.
        #[arg(long, value_name = "FILE")]
        markdown: Option<PathBuf>,
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
    /// Ask the graph a question, in words.
    ///
    /// Shares its entire implementation with the MCP server: two
    /// implementations of "what depends on this?" would eventually disagree,
    /// and only one of them would be measured.
    Query {
        /// The question. One of:
        /// "what depends on <path>", "what does <path> depend on",
        /// "blast radius of <path>", "files touching <package>", "summary".
        question: String,
        /// Project root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// How many file names to list. The total is always reported in full.
        #[arg(long)]
        limit: Option<usize>,
        /// How many hops a blast radius walks.
        #[arg(long)]
        depth: Option<usize>,
        /// Print the answer as JSON instead of prose.
        #[arg(long)]
        json: bool,
    },
    /// Serve the graph to an agent over MCP, on stdin and stdout.
    ///
    /// Scans once at startup, then answers until the stream closes. Progress
    /// goes to stderr; stdout carries protocol messages and nothing else.
    Mcp {
        /// Project root to scan. Defaults to the current directory.
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
        Command::Scan {
            root,
            output,
            graphml,
            cypher,
            yaml,
            markdown,
        } => scan(
            root,
            Exports {
                output,
                graphml,
                cypher,
                yaml,
                markdown,
            },
        ),
        Command::View { root } => view(root),
        Command::Query {
            question,
            root,
            limit,
            depth,
            json,
        } => query(root, &question, limit, depth, json),
        Command::Mcp { root } => mcp::serve(&canonicalize_root(root)?),
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

/// Print one project's numbers: what was read, what resolved, and — just
/// as loudly — what was not read at all.
fn print_project(project: &Project, labelled: bool) {
    let graph = &project.graph;
    let stats = &graph.stats;
    let coverage = &stats.coverage;

    if labelled {
        let kinds: Vec<String> = project.kinds.iter().map(|k| k.to_string()).collect();
        println!();
        println!("── {}  ({})", project.id, kinds.join(", "));
    }

    println!("files seen             {}", coverage.files_seen);
    println!("  analysed             {}", coverage.files_analysed);
    println!("  unsupported          {}", coverage.files_unsupported);
    println!("  not source           {}", coverage.files_not_source);
    println!("source coverage        {:.4}", coverage.source_coverage());
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

    // A language ships when it passes its own gate, so every language
    // publishes its own rate — an aggregate can hide a broken resolver
    // behind a majority language that works.
    for (lang, lang_stats) in &stats.by_lang {
        println!(
            "  {:<20} {:.4}  ({} files, {} edges)",
            lang, lang_stats.resolution_rate, lang_stats.files, lang_stats.edges
        );
    }

    // The gap list: source files KOG saw and could not read. This is the
    // number that stops a one-language tool from scoring 1.0000 on a
    // polyglot repository.
    let gaps: Vec<&kog_graph::ExtensionCoverage> = coverage
        .extensions
        .iter()
        .filter(|e| e.status == FileStatus::UnsupportedLanguage)
        .collect();
    if !gaps.is_empty() {
        println!(
            "not read               {} files",
            coverage.files_unsupported
        );
        for gap in gaps.iter().take(10) {
            println!(
                "  {:<19} {:>6}  {}",
                gap.label,
                gap.count,
                gap.lang.as_deref().unwrap_or("unrecognised")
            );
        }
        if gaps.len() > 10 {
            println!("  … {} more extensions", gaps.len() - 10);
        }
    }

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
                "  {}:{} [{}: {}] {}",
                diagnostic.path,
                diagnostic.line,
                diagnostic.kind,
                diagnostic.reason,
                diagnostic.specifier
            );
        }
        if stats.diagnostics.len() > 10 {
            println!("  … {} more", stats.diagnostics.len() - 10);
        }
    }
}

fn print_workspace(workspace: &Workspace) {
    println!("root                   {}", workspace.root);
    if workspace.split {
        println!(
            "projects               {} (the scanned directory contains projects rather than being one)",
            workspace.projects.len()
        );
    }

    for project in &workspace.projects {
        print_project(project, workspace.split);
    }

    if workspace.split {
        let totals = &workspace.totals;
        println!();
        println!("── all {} projects", totals.projects);
        println!("nodes                  {}", totals.nodes);
        println!("edges                  {}", totals.edges);
        println!("files analysed         {}", totals.files_analysed);
        println!("files not read         {}", totals.files_unsupported);
        println!("source coverage        {:.4}", totals.source_coverage);
        println!("resolution rate        {:.4}", totals.resolution_rate);
        if workspace.unassigned_files > 0 {
            println!(
                "unassigned files       {} (under the root, inside no project)",
                workspace.unassigned_files
            );
        }
    }
}

/// Scan, print the numbers, and write whatever files were asked for.
///
/// Every format is behind its own flag and none is a default: a scan that
/// wrote four files because you ran it is a scan you stop running.
/// Where a scan was asked to write, by format. Grouped so adding a format
/// does not lengthen a signature every test has to repeat.
#[derive(Default)]
struct Exports {
    output: Option<PathBuf>,
    graphml: Option<PathBuf>,
    cypher: Option<PathBuf>,
    yaml: Option<PathBuf>,
    markdown: Option<PathBuf>,
}

fn scan(root: PathBuf, exports: Exports) -> Result<()> {
    let root = canonicalize_root(root)?;
    let workspace = scan_workspace(&root);
    print_workspace(&workspace);

    let write = |path: Option<PathBuf>, body: &dyn Fn() -> String| -> Result<()> {
        let Some(path) = path else { return Ok(()) };
        std::fs::write(&path, body())
            .with_context(|| format!("cannot write {}", path.display()))?;
        println!("written                {}", path.display());
        Ok(())
    };

    write(exports.output, &|| {
        serde_json::to_string_pretty(&workspace).unwrap_or_default()
    })?;
    write(exports.graphml, &|| kog_graph::graphml(&workspace))?;
    write(exports.cypher, &|| kog_graph::cypher(&workspace))?;
    write(exports.yaml, &|| kog_graph::yaml(&workspace))?;
    write(exports.markdown, &|| kog_graph::markdown(&workspace))?;

    Ok(())
}

/// Scan `root` and serve the resulting graph, on 127.0.0.1 only, on an
/// OS-assigned free port. Never writes a file.
fn view(root: PathBuf) -> Result<()> {
    let root = canonicalize_root(root)?;
    let workspace = scan_workspace(&root);
    let graph_json = serde_json::to_string(&workspace)?;

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
        "{} nodes, {} edges across {} project(s) from {}",
        workspace.totals.nodes,
        workspace.totals.edges,
        workspace.totals.projects,
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

/// Scan `root` and answer one question about it.
///
/// Every step after parsing the phrase is the MCP server's code, unchanged:
/// the same `Atlas`, the same `Answer`, the same rendering. A second
/// implementation here would drift, and the terminal would start disagreeing
/// with the agent about the same repository.
fn query(
    root: PathBuf,
    question: &str,
    limit: Option<usize>,
    depth: Option<usize>,
    json: bool,
) -> Result<()> {
    let root = canonicalize_root(root)?;
    let mut question = Question::parse(question).map_err(|_| {
        let forms = QUESTION_FORMS
            .iter()
            .map(|form| format!("  {form}"))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow!("cannot answer {question:?}. Ask one of:\n{forms}")
    })?;

    match (&mut question, depth) {
        (Question::BlastRadius { depth: walk, .. }, Some(requested)) => *walk = requested,
        // Silently ignoring a flag the user typed would leave them believing
        // it applied. Say so, and answer the question they asked anyway.
        (_, Some(_)) => eprintln!("note: --depth only applies to a blast radius; ignoring it"),
        (_, None) => {}
    }

    let atlas = Atlas::scan(&root);
    let answer = atlas.answer(&question, limit.unwrap_or(DEFAULT_LIMIT));

    if json {
        println!("{}", serde_json::to_string_pretty(&answer)?);
    } else {
        print!("{}", render(&answer));
    }

    // A path that named nothing is a real failure for anything scripting
    // this, even though the printed answer is the useful one. Exit non-zero
    // after printing it, rather than instead of printing it.
    if matches!(answer, Answer::Unlocated { .. }) {
        use std::io::Write;
        std::io::stdout().flush().ok();
        std::process::exit(1);
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

        let err = scan(missing.clone(), Exports::default()).unwrap_err();

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

        let err = scan(file.clone(), Exports::default()).unwrap_err();

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

        scan(
            root,
            Exports {
                output: Some(output.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(output.exists(), "scan -o should have written {output:?}");
        let contents = fs::read_to_string(&output).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&contents).expect("written output should parse as valid JSON");

        // `scan -o` writes the workspace: one entry per project, each with
        // its own graph, so a directory holding several projects produces
        // several graphs rather than one merged fiction.
        let project = &json["projects"][0];
        assert_eq!(project["id"], ".");
        assert_eq!(project["graph"]["stats"]["resolution_rate"], 1.0);
        assert_eq!(project["graph"]["edges"].as_array().unwrap().len(), 1);
        assert_eq!(json["totals"]["projects"], 1);
    }

    /// The feature this shape exists for: pointed at a folder that merely
    /// contains projects, the scan produces one graph per project.
    #[test]
    fn scanning_a_folder_of_projects_writes_one_graph_per_project() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("workspace");
        for (name, manifest) in [("web", "package.json"), ("api", "package.json")] {
            fs::create_dir_all(root.join(name).join("src")).unwrap();
            fs::write(
                root.join(name).join(manifest),
                format!(r#"{{"name":"{name}"}}"#),
            )
            .unwrap();
            fs::write(root.join(name).join("src/a.ts"), "export const a = 1;").unwrap();
        }
        let output = dir.path().join("graph.json");

        scan(
            root,
            Exports {
                output: Some(output.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(json["split"], true);
        let projects = json["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 2);
        let ids: Vec<&str> = projects.iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["api", "web"]);
        // Two source files and the two `package.json` manifests: every file
        // is a node, not only the ones an extractor can read.
        assert_eq!(json["totals"]["nodes"], 4);
        assert_eq!(json["totals"]["files_analysed"], 2);
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

        scan(
            root,
            Exports {
                output: Some(output.clone()),
                ..Default::default()
            },
        )
        .expect("an unreadable file must not be fatal");

        let contents = fs::read_to_string(&output).unwrap();
        let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let failures = json["projects"][0]["graph"]["stats"]["failures"]
            .as_array()
            .unwrap();
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

        scan(root.clone(), Exports::default()).unwrap();

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
                graphml: None,
                cypher: None,
                yaml: None,
                markdown: None,
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
                graphml: None,
                cypher: None,
                yaml: None,
                markdown: None,
            })
        );
    }

    /// The question is positional and the root is a flag, so the shortest
    /// useful invocation is `kog query "what depends on X"` in the directory
    /// you are already standing in.
    /// Every export is behind its own flag and none is a default: a scan
    /// that wrote four files because you ran it is a scan you stop running.
    #[test]
    fn a_scan_writes_only_the_formats_it_was_asked_for() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.ts"), "export const x = 1;").unwrap();
        fs::write(root.join("src/a.ts"), r#"import { x } from "./lib";"#).unwrap();

        let graphml = dir.path().join("g.graphml");
        scan(
            root.clone(),
            Exports {
                graphml: Some(graphml.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let written = fs::read_to_string(&graphml).unwrap();
        assert!(written.contains("<node id=\"src/a.ts\">"), "got {written}");
        assert!(written.contains("target=\"src/lib.ts\""));
        assert!(
            !dir.path().join("g.cypher").exists(),
            "a format that was not asked for must not be written"
        );

        let cypher = dir.path().join("g.cypher");
        scan(
            root,
            Exports {
                cypher: Some(cypher.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(fs::read_to_string(&cypher)
            .unwrap()
            .contains("MERGE (a)-[:IMPORTS]->(b)"));
    }

    #[test]
    fn each_export_has_its_own_flag() {
        let cli = Cli::try_parse_from([
            "kog",
            "scan",
            "dir",
            "--graphml",
            "g.graphml",
            "--cypher",
            "g.cypher",
        ])
        .unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Scan {
                root: PathBuf::from("dir"),
                output: None,
                graphml: Some(PathBuf::from("g.graphml")),
                cypher: Some(PathBuf::from("g.cypher")),
                yaml: None,
                markdown: None,
            })
        );
    }

    #[test]
    fn query_takes_the_question_first_and_defaults_the_root() {
        let cli = Cli::try_parse_from(["kog", "query", "what depends on src/a.ts"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Query {
                question: "what depends on src/a.ts".to_string(),
                root: PathBuf::from("."),
                limit: None,
                depth: None,
                json: false,
            })
        );
    }

    #[test]
    fn query_accepts_a_root_a_limit_a_depth_and_json() {
        let cli = Cli::try_parse_from([
            "kog",
            "query",
            "blast radius of src/a.ts",
            "--root",
            "some/dir",
            "--limit",
            "5",
            "--depth",
            "2",
            "--json",
        ])
        .unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Query {
                question: "blast radius of src/a.ts".to_string(),
                root: PathBuf::from("some/dir"),
                limit: Some(5),
                depth: Some(2),
                json: true,
            })
        );
    }

    #[test]
    fn mcp_root_defaults_to_the_current_directory() {
        let cli = Cli::try_parse_from(["kog", "mcp"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Mcp {
                root: PathBuf::from("."),
            })
        );
    }

    /// A phrase the parser does not recognise must not be answered with
    /// something else. The error names every form that would have worked, so
    /// the next attempt succeeds.
    #[test]
    fn an_unreadable_question_names_the_forms_that_would_have_worked() {
        let dir = TempDir::new().unwrap();
        let err = query(dir.path().to_path_buf(), "do the thing", None, None, false).unwrap_err();

        let message = err.to_string();
        for form in QUESTION_FORMS {
            assert!(
                message.contains(form),
                "the error should list {form:?}, got {message:?}"
            );
        }
    }

    /// `query` reaches the same `Answer` the MCP server does — asserted
    /// through the JSON output, which is that answer verbatim.
    #[test]
    fn query_answers_from_a_real_scan() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.ts"), "export const x = 1;").unwrap();
        fs::write(root.join("src/a.ts"), r#"import { x } from "./lib";"#).unwrap();

        query(
            root.clone(),
            "what depends on src/lib.ts",
            None,
            None,
            false,
        )
        .expect("a well-formed question about a real file must be answered");
    }

    #[test]
    fn a_query_against_an_unreadable_root_fails_the_same_way_a_scan_does() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");

        let err = query(missing.clone(), "summary", None, None, false).unwrap_err();

        assert!(err.to_string().contains("not a readable directory"));
    }
}
