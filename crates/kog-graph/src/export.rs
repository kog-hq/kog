//! Handing the graph to tools that are better at something than KOG is.
//!
//! A scan already writes JSON, which is the complete record. These two
//! formats exist because a graph is worth more inside a graph tool: GraphML
//! opens in Gephi, yEd, Cytoscape; Cypher loads into Neo4j and can then be
//! queried in ways `kog query` will never cover.
//!
//! ## What an exported id is
//!
//! A node id is only unique *within a project*, and a scan can hold several —
//! two of them can both contain `src/index.ts`. Every id written here is
//! therefore qualified with its project, exactly as [`crate::FileRef`]
//! displays one, so a workspace of nine projects does not silently merge nine
//! `src/index.ts` into one node in Neo4j.
//!
//! [`markdown`] is the odd one out: it is not the graph, it is the *report* —
//! the two numbers, the rate per language, what was not read, and the files
//! everything points at. It is what you paste into a pull request, and it is
//! the one export a person reads rather than a tool.
//!
//! ## What is not here
//!
//! **Any picture — SVG, PNG, PDF.** All three need a layout, and the only
//! layout KOG has runs in the browser. Writing a worse one in Rust to produce
//! an image that *looks* authoritative is the failure this project is
//! against, and a second layout would drift from the first besides. The
//! interface exports a PNG of exactly what you are looking at, which is the
//! same graph laid out by the code that laid it out on screen; the formats
//! here hand the graph to tools whose layouts are better than either.

use crate::model::NodeKind;
use crate::project::Workspace;
use std::fmt::Write;

/// A node's identity outside the scan: project-qualified, so two projects'
/// `src/index.ts` stay two nodes.
fn qualified(project: &str, id: &str) -> String {
    if project == "." {
        id.to_string()
    } else {
        format!("{project}/{id}")
    }
}

fn kind_of(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Source => "source",
        NodeKind::UnreadSource => "unread_source",
        NodeKind::Asset => "asset",
    }
}

/// Escape text for an XML attribute or element body.
///
/// Not optional: a file called `a&b.ts` — or any path with a `<` in it —
/// produces a document no parser will open, and the export would fail at the
/// far end rather than here.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(character),
        }
    }
    out
}

/// Escape text for a double-quoted Cypher string literal.
///
/// Same reasoning as [`xml`], with a sharper edge: an unescaped quote in a
/// path does not merely break the file, it changes what the statement says.
fn cypher_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(character),
        }
    }
    out
}

/// The whole workspace as GraphML.
pub fn graphml(workspace: &Workspace) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns">
  <key id="path" for="node" attr.name="path" attr.type="string"/>
  <key id="project" for="node" attr.name="project" attr.type="string"/>
  <key id="lang" for="node" attr.name="lang" attr.type="string"/>
  <key id="kind" for="node" attr.name="kind" attr.type="string"/>
  <key id="loc" for="node" attr.name="loc" attr.type="int"/>
  <key id="bytes" for="node" attr.name="bytes" attr.type="long"/>
"#,
    );
    // `edgedefault="directed"`: an import has a direction, and losing it
    // would turn "what depends on this" into "what is near this".
    out.push_str("  <graph id=\"kog\" edgedefault=\"directed\">\n");

    for project in &workspace.projects {
        for node in &project.graph.nodes {
            let id = xml(&qualified(&project.id, &node.id));
            let _ = write!(
                out,
                "    <node id=\"{id}\">\n      \
                 <data key=\"path\">{}</data>\n      \
                 <data key=\"project\">{}</data>\n      \
                 <data key=\"lang\">{}</data>\n      \
                 <data key=\"kind\">{}</data>\n      \
                 <data key=\"loc\">{}</data>\n      \
                 <data key=\"bytes\">{}</data>\n    \
                 </node>\n",
                xml(&node.path),
                xml(&project.id),
                xml(&node.lang),
                kind_of(node.kind),
                node.loc,
                node.bytes,
            );
        }
    }

    let mut edge = 0usize;
    for project in &workspace.projects {
        for import in &project.graph.edges {
            let _ = writeln!(
                out,
                "    <edge id=\"e{edge}\" source=\"{}\" target=\"{}\"/>",
                xml(&qualified(&project.id, &import.source)),
                xml(&qualified(&project.id, &import.target)),
            );
            edge += 1;
        }
    }

    out.push_str("  </graph>\n</graphml>\n");
    out
}

/// The whole workspace as Cypher statements, ready for `cypher-shell`.
pub fn cypher(workspace: &Workspace) -> String {
    let mut out = String::from(
        "// Generated by kog. Load with:\n\
         //   cat graph.cypher | cypher-shell -u neo4j\n\
         //\n\
         // MERGE rather than CREATE throughout, so loading the same scan twice\n\
         // updates one graph instead of building a second one beside it.\n\
         CREATE CONSTRAINT kog_file_id IF NOT EXISTS\n  \
         FOR (f:File) REQUIRE f.id IS UNIQUE;\n\n",
    );

    for project in &workspace.projects {
        for node in &project.graph.nodes {
            let _ = write!(
                out,
                "MERGE (f:File {{id: \"{}\"}})\n  SET f.path = \"{}\", f.project = \"{}\", \
                 f.lang = \"{}\", f.kind = \"{}\", f.loc = {}, f.bytes = {};\n",
                cypher_string(&qualified(&project.id, &node.id)),
                cypher_string(&node.path),
                cypher_string(&project.id),
                cypher_string(&node.lang),
                kind_of(node.kind),
                node.loc,
                node.bytes,
            );
        }
    }
    out.push('\n');

    for project in &workspace.projects {
        for import in &project.graph.edges {
            let _ = write!(
                out,
                "MATCH (a:File {{id: \"{}\"}}), (b:File {{id: \"{}\"}})\n  \
                 MERGE (a)-[:IMPORTS]->(b);\n",
                cypher_string(&qualified(&project.id, &import.source)),
                cypher_string(&qualified(&project.id, &import.target)),
            );
        }
    }

    out
}

/// The whole workspace as YAML: the same record as the JSON, for anything
/// that would rather read one.
pub fn yaml(workspace: &Workspace) -> String {
    yaml_serde::to_string(workspace).unwrap_or_else(|error| format!("# kog: {error}\n"))
}

/// One row of the "most depended upon" table.
fn hubs(project: &crate::project::Project, take: usize) -> Vec<(usize, &str)> {
    let mut counted: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for edge in &project.graph.edges {
        *counted.entry(edge.target.as_str()).or_default() += 1;
    }
    let mut hubs: Vec<(usize, &str)> = counted.into_iter().map(|(id, n)| (n, id)).collect();
    hubs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    hubs.truncate(take);
    hubs
}

/// How many rows a table shows before it starts counting instead. Every one
/// of them still reports its true total on the line above.
const TABLE_ROWS: usize = 15;

/// The scan as a report a person reads.
///
/// Every number here is one the scan already published; nothing is computed
/// for the document that is not in the JSON beside it. A truncated table says
/// what it left out, for the same reason every other listing in this codebase
/// does.
pub fn markdown(workspace: &Workspace) -> String {
    let totals = &workspace.totals;
    let mut out = String::new();
    // Written line by line rather than as one continued literal: a stray
    // leading space is invisible in the source and turns a Markdown table
    // into an indented code block, which is exactly what happened the first
    // time this was written.
    let _ = writeln!(out, "# Scan of `{}`\n", workspace.root);
    let _ = writeln!(out, "| | |");
    let _ = writeln!(out, "| --- | ---: |");
    let _ = writeln!(
        out,
        "| **Resolution rate** | **{:.4}** |",
        totals.resolution_rate
    );
    let _ = writeln!(
        out,
        "| **Source coverage** | **{:.4}** |",
        totals.source_coverage
    );
    let _ = writeln!(out, "| Projects | {} |", totals.projects);
    let _ = writeln!(out, "| Files seen | {} |", totals.files_seen);
    let _ = writeln!(out, "| Analysed | {} |", totals.files_analysed);
    let _ = writeln!(out, "| Not read | {} |", totals.files_unsupported);
    let _ = writeln!(out, "| Nodes | {} |", totals.nodes);
    let _ = writeln!(out, "| Edges | {} |", totals.edges);
    out.push_str(
        "\n`resolution rate = resolved / (internal − excluded)` — of the imports KOG read, \
how many pointed at a file it found.\n\n\
`source coverage = analysed / (analysed + unsupported)` — of the files that are source \
code, how many an extractor actually read. The first number alone lets a tool that reads \
one language score 1.0000 on a polyglot repository.\n",
    );

    for project in &workspace.projects {
        let stats = &project.graph.stats;
        if workspace.split {
            let _ = write!(
                out,
                "\n## `{}`\n\nRate {:.4}, coverage {:.4}, {} nodes, {} edges.\n",
                project.id,
                stats.resolution_rate,
                stats.coverage.source_coverage(),
                project.graph.nodes.len(),
                project.graph.edges.len(),
            );
        }

        // A language ships when it passes its own gate, so each publishes its
        // own rate: an aggregate can be healthy while one resolver is broken.
        out.push_str("\n### By language\n\n| Language | Files | Edges | Rate |\n| --- | ---: | ---: | ---: |\n");
        for (lang, lang_stats) in &stats.by_lang {
            let _ = writeln!(
                out,
                "| {lang} | {} | {} | {:.4} |",
                lang_stats.files, lang_stats.edges, lang_stats.resolution_rate
            );
        }

        let gaps: Vec<_> = stats
            .coverage
            .extensions
            .iter()
            .filter(|entry| entry.status == crate::model::FileStatus::UnsupportedLanguage)
            .collect();
        if !gaps.is_empty() {
            let _ = writeln!(
                out,
                "\n### Not read — {} files\n",
                stats.coverage.files_unsupported
            );
            out.push_str(
                "These are nodes in the graph; their own imports are not, because KOG has no \
extractor for them yet.\n\n| Extension | Files | Language |\n| --- | ---: | --- |\n",
            );
            for gap in gaps.iter().take(TABLE_ROWS) {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} |",
                    gap.label,
                    gap.count,
                    gap.lang.as_deref().unwrap_or("unrecognised")
                );
            }
            if gaps.len() > TABLE_ROWS {
                let _ = writeln!(out, "\n… and {} more extensions.", gaps.len() - TABLE_ROWS);
            }
        }

        let hubs = hubs(project, TABLE_ROWS);
        if !hubs.is_empty() {
            out.push_str("\n### Most depended upon\n\n| Dependents | File |\n| ---: | --- |\n");
            for (count, id) in hubs {
                let _ = writeln!(out, "| {count} | `{id}` |");
            }
        }

        let broken = stats.unresolved + stats.excluded;
        if broken > 0 {
            let shown = stats.diagnostics.len().min(TABLE_ROWS);
            let _ = writeln!(
                out,
                "\n### Every import that did not become an edge — {broken}\n"
            );
            out.push_str("| File | Line | Specifier | Why |\n| --- | ---: | --- | --- |\n");
            for diagnostic in stats.diagnostics.iter().take(TABLE_ROWS) {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | `{}` | {} |",
                    diagnostic.path, diagnostic.line, diagnostic.specifier, diagnostic.reason
                );
            }
            if broken > shown {
                let _ = writeln!(
                    out,
                    "\n… and {} more. The count above is exact; this table is not the record — `kog scan -o graph.json` is.",
                    broken - shown
                );
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::scan_workspace;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn one_project() -> (TempDir, Workspace) {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "package.json", r#"{"name":"app"}"#);
        write_file(&dir, "src/lib.ts", "export const x = 1;\n");
        write_file(&dir, "src/a.ts", "import { x } from \"./lib\";\n");
        let workspace = scan_workspace(dir.path());
        (dir, workspace)
    }

    #[test]
    fn graphml_holds_every_node_and_every_edge() {
        let (_dir, workspace) = one_project();
        let xml = graphml(&workspace);

        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains(r#"edgedefault="directed""#));
        assert_eq!(xml.matches("<node id=").count(), workspace.totals.nodes);
        assert_eq!(xml.matches("<edge id=").count(), workspace.totals.edges);
        assert!(xml.contains(r#"<node id="src/a.ts">"#));
        assert!(xml.contains(r#"source="src/a.ts" target="src/lib.ts""#));
        assert!(xml.trim_end().ends_with("</graphml>"));
    }

    #[test]
    fn cypher_holds_every_node_and_every_edge() {
        let (_dir, workspace) = one_project();
        let script = cypher(&workspace);

        assert_eq!(
            script.matches("MERGE (f:File").count(),
            workspace.totals.nodes
        );
        assert_eq!(
            script.matches("MERGE (a)-[:IMPORTS]->(b)").count(),
            workspace.totals.edges
        );
        assert!(
            script.contains("CREATE CONSTRAINT"),
            "loading twice must update one graph, not build a second"
        );
    }

    /// A node id is only unique within a project. Two projects can both hold
    /// `src/index.ts`, and exporting them under the same id would merge two
    /// real files into one node at the far end — silently, and only in the
    /// export.
    #[test]
    fn two_projects_holding_the_same_path_stay_two_nodes() {
        let dir = TempDir::new().unwrap();
        for name in ["web", "api"] {
            write_file(&dir, &format!("{name}/package.json"), r#"{"name":"p"}"#);
            write_file(&dir, &format!("{name}/src/index.ts"), "");
        }
        let workspace = scan_workspace(dir.path());
        assert!(workspace.split, "test setup must produce a split scan");

        let xml = graphml(&workspace);
        assert!(xml.contains(r#"<node id="web/src/index.ts">"#));
        assert!(xml.contains(r#"<node id="api/src/index.ts">"#));

        let script = cypher(&workspace);
        assert!(script.contains(r#"id: "web/src/index.ts""#));
        assert!(script.contains(r#"id: "api/src/index.ts""#));
    }

    /// A path is a filename, and filenames contain the characters that break
    /// both formats. Unescaped, GraphML produces a document no parser opens
    /// and Cypher produces a statement that says something else.
    #[test]
    fn a_path_with_hostile_characters_is_escaped_in_both_formats() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "package.json", r#"{"name":"app"}"#);
        write_file(&dir, r#"src/a&b<c>.ts"#, "");
        let workspace = scan_workspace(dir.path());

        let xml = graphml(&workspace);
        assert!(xml.contains("a&amp;b&lt;c&gt;.ts"), "got {xml}");
        assert!(
            !xml.contains("a&b<c>.ts"),
            "the raw characters must not survive into the document"
        );

        assert_eq!(cypher_string(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(cypher_string("a\nb"), "a\\nb");
    }

    #[test]
    fn yaml_round_trips_to_the_same_workspace() {
        let (_dir, workspace) = one_project();
        let text = yaml(&workspace);
        let back: Workspace = yaml_serde::from_str(&text).expect("kog must emit valid YAML");
        assert_eq!(back, workspace, "YAML must be the same record as the JSON");
    }

    #[test]
    fn the_markdown_report_leads_with_the_two_numbers() {
        let (_dir, workspace) = one_project();
        let report = markdown(&workspace);

        assert!(report.starts_with("# Scan of "), "got {report}");
        assert!(
            report.contains("**Resolution rate** | **1.0000**"),
            "got {report}"
        );
        assert!(report.contains("**Source coverage**"));
        assert!(report.contains("### By language"));
        assert!(report.contains("| typescript |"));
        assert!(
            report.contains("### Most depended upon"),
            "the hubs are the first useful thing about an unfamiliar repository"
        );
        assert!(report.contains("`src/lib.ts`"));
    }

    /// A report that quietly showed fifteen of six hundred broken imports
    /// would read as a repository with fifteen. The count is exact and the
    /// table says what it left out — the same rule as every other listing
    /// here.
    #[test]
    fn a_truncated_table_says_what_it_left_out() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "package.json", r#"{"name":"app"}"#);
        let mut body = String::new();
        for i in 0..40 {
            body.push_str(&format!("import x{i} from \"./ghost{i}\";\n"));
        }
        write_file(&dir, "src/a.ts", &body);
        let workspace = scan_workspace(dir.path());

        let report = markdown(&workspace);
        assert!(
            report.contains("did not become an edge — 40"),
            "got {report}"
        );
        assert!(report.contains("… and 25 more"), "got {report}");
    }

    /// A leading space is invisible in the source and fatal in the output:
    /// four of them turn a Markdown table into an indented code block, so the
    /// table renders as literal text. The first version of this report did
    /// exactly that, and nothing but reading the file would have shown it.
    #[test]
    fn no_line_of_the_report_starts_with_whitespace() {
        // A split scan with enough broken imports to truncate a table: the
        // first version of this guard only built one project and missed three
        // indented blocks that only a second project reached.
        let dir = TempDir::new().unwrap();
        for name in ["web", "api"] {
            write_file(&dir, &format!("{name}/package.json"), r#"{"name":"p"}"#);
            let mut body = String::new();
            for i in 0..30 {
                body.push_str(&format!("import x{i} from \"./ghost{i}\";\n"));
            }
            write_file(&dir, &format!("{name}/src/a.ts"), &body);
            write_file(&dir, &format!("{name}/main.hs"), "module Main where");
        }
        let workspace = scan_workspace(dir.path());
        assert!(workspace.split, "the guard must cover a split scan");

        for (number, line) in markdown(&workspace).lines().enumerate() {
            assert!(
                !line.starts_with(char::is_whitespace),
                "line {} is indented, which Markdown reads as a code block: {line:?}",
                number + 1
            );
        }
    }

    #[test]
    fn an_empty_scan_still_produces_a_valid_document() {
        let dir = TempDir::new().unwrap();
        let workspace = scan_workspace(dir.path());

        let xml = graphml(&workspace);
        assert!(xml.starts_with("<?xml") && xml.trim_end().ends_with("</graphml>"));
        assert!(cypher(&workspace).contains("CREATE CONSTRAINT"));
    }
}
