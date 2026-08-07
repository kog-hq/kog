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
//! ## What is not here
//!
//! **SVG.** It is on the roadmap and it is not in this file, for a reason
//! worth stating: an SVG needs a layout, and the only layout KOG has runs in
//! the browser. Writing a worse one in Rust to produce a picture that *looks*
//! authoritative is the failure this project is against — and both formats
//! below hand the graph to tools whose layouts are far better than anything
//! that would be written here.

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
    fn an_empty_scan_still_produces_a_valid_document() {
        let dir = TempDir::new().unwrap();
        let workspace = scan_workspace(dir.path());

        let xml = graphml(&workspace);
        assert!(xml.starts_with("<?xml") && xml.trim_end().ends_with("</graphml>"));
        assert!(cypher(&workspace).contains("CREATE CONSTRAINT"));
    }
}
