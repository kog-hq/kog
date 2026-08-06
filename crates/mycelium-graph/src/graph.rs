use crate::discover::discover;
use crate::extractor::{Extractor, Resolution, Specifier};
use crate::model::{
    Diagnostic, DiagnosticKind, Edge, EdgeKind, Failure, Graph, Node, Stats, MAX_DIAGNOSTICS,
};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Project-relative, slash-separated identity for a file.
fn node_id(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Same as [`node_id`], but falls back to canonicalising `path` first when a
/// direct `strip_prefix` against the (already canonical) `root` fails. This
/// covers paths that were built by an `Extractor` against its own idea of
/// root (e.g. a tsconfig index) rather than the canonical root `build_graph`
/// uses for its file nodes.
fn node_id_lenient(root: &Path, path: &Path) -> Option<String> {
    node_id(root, path).or_else(|| node_id(root, &path.canonicalize().ok()?))
}

/// A file that was successfully read and parsed, so it will always become a
/// `Node`. Kept separate from a failed read/parse so that `known` (see
/// `build_graph`) only ever contains files that actually end up in
/// `graph.nodes`: an import into a file that failed to read or parse must
/// never be counted `resolved`, and must never produce an edge pointing at
/// a node that does not exist.
struct ParsedFile {
    id: String,
    path: PathBuf,
    source: String,
    specifiers: Vec<Specifier>,
}

/// Record one non-resolved specifier, capped at `MAX_DIAGNOSTICS` so a huge
/// broken repo cannot produce an unbounded `graph.json`. The precise
/// counters on `Stats` (`unresolved`/`excluded`) are updated by the caller
/// and always hold the true total even once this cap is hit.
fn record_diagnostic(stats: &mut Stats, path: &str, specifier: &Specifier, kind: DiagnosticKind) {
    if stats.diagnostics.len() < MAX_DIAGNOSTICS {
        stats.diagnostics.push(Diagnostic {
            path: path.to_string(),
            line: specifier.line,
            specifier: specifier.raw.clone(),
            kind,
        });
    }
}

/// Scan a project into a graph. Never panics on a bad file: it is recorded in
/// `stats.failures` and the scan continues.
pub fn build_graph(root: &Path, extractor: &dyn Extractor) -> Graph {
    // `discover` canonicalises its own root internally; do the same here so
    // that `node_id` strips the exact prefix `discover` used. Without this,
    // a non-canonical `root` (e.g. one containing a `..` component) makes
    // every `strip_prefix` below fail silently, and every node is skipped.
    let root: PathBuf = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root: &Path = &root;

    let files = discover(root, extractor.extensions());

    let mut stats = Stats {
        files_discovered: files.len(),
        ..Default::default()
    };

    // Pass 1: read and parse every discovered file, recording a `Failure`
    // immediately for anything that cannot be. `known` is built only from
    // what survives this pass — computing it from *all* discovered files
    // (before failures are known) would let a specifier into a file that
    // failed be counted `resolved` and produce an edge to a node that is
    // never actually emitted.
    let mut parsed: Vec<ParsedFile> = Vec::new();
    for file in &files {
        let id = match node_id(root, file) {
            Some(id) => id,
            None => continue,
        };

        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("read failed: {e}"),
                });
                continue;
            }
        };

        let specifiers = match extractor.extract(&source) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("extract failed: {e}"),
                });
                continue;
            }
        };
        stats.files_parsed += 1;

        parsed.push(ParsedFile {
            id,
            path: file.clone(),
            source,
            specifiers,
        });
    }

    // Only a file that survived pass 1 can be an edge's target.
    let known: HashSet<&str> = parsed.iter().map(|p| p.id.as_str()).collect();

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: HashSet<Edge> = HashSet::new();
    let mut external_names: BTreeSet<String> = BTreeSet::new();

    // Pass 2: resolve every specifier now that `known` is final.
    for file in &parsed {
        let mut node_externals: BTreeSet<String> = BTreeSet::new();

        for specifier in &file.specifiers {
            stats.specifiers_total += 1;
            match extractor.resolve(&specifier.raw, &file.path) {
                Resolution::External(package) => {
                    stats.external_specifiers += 1;
                    external_names.insert(package.clone());
                    node_externals.insert(package);
                }
                Resolution::Internal(target) => {
                    stats.specifiers_internal += 1;
                    match node_id(root, &target).filter(|t| known.contains(t.as_str())) {
                        Some(target_id) => {
                            stats.resolved += 1;
                            if target_id != file.id {
                                edges.insert(Edge {
                                    source: file.id.clone(),
                                    target: target_id,
                                    kind: EdgeKind::Import,
                                });
                            }
                        }
                        // Resolved to a real file, but it is not part of the
                        // scanned node set (gitignored, always-skipped, a
                        // failed read/parse, or an extension this extractor
                        // does not claim) — a deliberate exclusion, not a
                        // broken import. Must not depress the resolution
                        // rate the way a genuinely unresolved specifier does.
                        None => {
                            stats.excluded += 1;
                            record_diagnostic(
                                &mut stats,
                                &file.id,
                                specifier,
                                DiagnosticKind::Excluded,
                            );
                        }
                    }
                }
                Resolution::Unresolved => {
                    stats.specifiers_internal += 1;
                    stats.unresolved += 1;
                    record_diagnostic(&mut stats, &file.id, specifier, DiagnosticKind::Unresolved);
                }
            }
        }

        nodes.push(Node {
            id: file.id.clone(),
            path: file.id.clone(),
            lang: extractor.lang().to_string(),
            loc: file.source.lines().count(),
            external_deps: node_externals.into_iter().collect(),
        });
    }

    // A config the extractor could not use (e.g. an unreadable or malformed
    // tsconfig) degrades resolution for a whole subtree; design doc §7
    // demands that never disappear silently. The reason is prefixed so it
    // reads distinctly from a source-file failure.
    for skipped in extractor.skipped_configs() {
        let path = node_id_lenient(root, &skipped.path)
            .unwrap_or_else(|| skipped.path.to_string_lossy().into_owned());
        stats.failures.push(Failure {
            path,
            reason: format!("tsconfig skipped: {}", skipped.reason),
        });
    }

    stats.external_packages_distinct = external_names.len();
    stats.resolution_rate = stats.resolution_rate();

    let mut edges: Vec<Edge> = edges.into_iter().collect();
    edges.sort_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    Graph {
        nodes,
        edges,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
    use crate::extractors::TypeScriptExtractor;
    use crate::model::{DiagnosticKind, Graph};
    use crate::tsconfig::SkippedConfig;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn build(dir: &TempDir) -> Graph {
        let extractor = TypeScriptExtractor::new(dir.path());
        crate::build_graph(dir.path(), &extractor)
    }

    #[test]
    fn every_discovered_file_becomes_a_node() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "export const b = 1;");
        let graph = build(&dir);
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn node_ids_are_project_relative_and_slash_separated() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/deep/a.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.nodes[0].id, "src/deep/a.ts");
    }

    #[test]
    fn external_packages_land_on_the_node_not_in_the_edges() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import React from "react";"#);
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 0);
        assert_eq!(graph.nodes[0].external_deps, vec!["react".to_string()]);
    }

    #[test]
    fn duplicate_edges_are_collapsed() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import { x } from "./b";
import { y } from "./b";"#,
        );
        write(&dir, "src/b.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn stats_count_internal_and_external_separately() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import React from "react";
import b from "./b";
import ghost from "./ghost";"#,
        );
        write(&dir, "src/b.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.stats.specifiers_total, 3);
        assert_eq!(graph.stats.specifiers_internal, 2);
        assert_eq!(graph.stats.resolved, 1);
        assert_eq!(graph.stats.unresolved, 1);
        // "./ghost" points nowhere on disk at all, so it is a genuine
        // unresolved import, not merely `excluded` (see the dedicated test
        // below for that distinction).
        assert_eq!(graph.stats.excluded, 0);
        assert_eq!(graph.stats.external_specifiers, 1);
        assert_eq!(graph.stats.external_packages_distinct, 1);
        assert!((graph.stats.resolution_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn loc_counts_the_lines_of_each_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "one\ntwo\nthree");
        let graph = build(&dir);
        assert_eq!(graph.nodes[0].loc, 3);
    }

    #[test]
    fn an_edge_pointing_outside_the_project_is_dropped_and_counted() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import x from "../../outside/x";"#);
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 0);
    }

    #[test]
    fn nodes_are_sorted_so_runs_are_reproducible() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/z.ts", "");
        write(&dir, "src/a.ts", "");
        let graph = build(&dir);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/z.ts"]);
    }

    // --- Extra coverage beyond the brief ---

    #[test]
    fn build_graph_handles_a_non_canonical_root() {
        // `discover` canonicalises its root internally. If `build_graph`
        // does not canonicalise the same way before computing node ids, the
        // `strip_prefix` inside `node_id` silently fails against the
        // canonical paths `discover` hands back, and every node is skipped.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "export const b = 1;");

        // Force a `..` component into the root so a naive `strip_prefix`
        // against the raw (non-canonical) root would fail.
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let noncanonical_root = subdir.join("..");
        assert!(
            noncanonical_root
                .components()
                .any(|c| c == std::path::Component::ParentDir),
            "test setup must actually produce a `..` component"
        );

        let extractor = TypeScriptExtractor::new(&noncanonical_root);
        let graph = crate::build_graph(&noncanonical_root, &extractor);

        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn a_skipped_tsconfig_is_recorded_as_a_failure() {
        // Design doc §7: a tsconfig that could not be read or parsed
        // degrades alias resolution for a whole subtree, and that must
        // never disappear silently.
        let dir = TempDir::new().unwrap();
        write(&dir, "tsconfig.json", "{ this is not json at all ");
        write(&dir, "src/a.ts", "");
        let graph = build(&dir);

        let failure = graph
            .stats
            .failures
            .iter()
            .find(|f| f.path == "tsconfig.json");
        assert!(
            failure.is_some(),
            "expected a failure entry for the skipped tsconfig.json, got {:?}",
            graph.stats.failures
        );
        let failure = failure.unwrap();
        assert!(
            failure.reason.starts_with("tsconfig skipped: "),
            "reason should be prefixed to distinguish it from a source-file \
             failure, got {:?}",
            failure.reason
        );
        assert!(!failure.reason["tsconfig skipped: ".len()..].is_empty());
    }

    // --- Fix round 1: Finding 1 — root canonicalisation inside the extractor ---

    #[cfg(unix)]
    #[test]
    fn build_graph_resolves_a_tsconfig_alias_through_a_symlinked_root() {
        use std::os::unix::fs::symlink;

        let real = TempDir::new().unwrap();
        write(
            &real,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        write(&real, "src/app/page.tsx", r#"import x from "@/lib/api";"#);
        write(&real, "src/lib/api.ts", "");

        // A symlinked root is deliberately non-canonical: `Path::canonicalize`
        // resolves through it, but this crate's lexical-only normalisation
        // does not. This is the same shape of non-canonical root macOS
        // hands every `TempDir` for free (`/var/...` symlinks to
        // `/private/var/...`), made explicit and platform-independent so
        // the test does not depend on the host's temp directory layout.
        let link_parent = TempDir::new().unwrap();
        let symlinked_root = link_parent.path().join("root-link");
        symlink(real.path(), &symlinked_root).unwrap();

        let extractor = TypeScriptExtractor::new(&symlinked_root);
        let graph = crate::build_graph(&symlinked_root, &extractor);

        assert_eq!(
            graph.edges.len(),
            1,
            "the alias-resolved import must become an edge, got {:?}",
            graph.edges
        );
        assert_eq!(graph.edges[0].source, "src/app/page.tsx");
        assert_eq!(graph.edges[0].target, "src/lib/api.ts");
        assert!(
            (graph.stats.resolution_rate - 1.0).abs() < 1e-9,
            "got rate {}",
            graph.stats.resolution_rate
        );
    }

    // --- Fix round 1: Finding 2 — excluded vs. unresolved, and diagnostics ---

    #[test]
    fn a_specifier_resolving_outside_the_scanned_set_is_excluded_not_unresolved() {
        // Mirrors a generated Prisma client on the acceptance target: the
        // target file genuinely exists on disk, it is simply gitignored, so
        // it never became a node. That must not count against the parser or
        // resolver the way a truly broken import does.
        let dir = TempDir::new().unwrap();
        write(&dir, ".gitignore", "generated/\n");
        write(&dir, "src/a.ts", r#"import g from "../generated/g";"#);
        write(&dir, "generated/g.ts", "");
        let graph = build(&dir);

        assert_eq!(
            graph.edges.len(),
            0,
            "no edge may point at a node outside the scanned set"
        );
        assert_eq!(graph.stats.excluded, 1);
        assert_eq!(graph.stats.unresolved, 0);
        assert!(
            (graph.stats.resolution_rate - 1.0).abs() < 1e-9,
            "an excluded specifier must not depress the resolution rate, got {}",
            graph.stats.resolution_rate
        );

        let diag = graph
            .stats
            .diagnostics
            .iter()
            .find(|d| d.specifier == "../generated/g");
        assert!(
            diag.is_some(),
            "expected a diagnostic for the excluded specifier, got {:?}",
            graph.stats.diagnostics
        );
        let diag = diag.unwrap();
        assert_eq!(diag.kind, DiagnosticKind::Excluded);
        assert_eq!(diag.path, "src/a.ts");
        assert_eq!(diag.line, 1);
    }

    #[test]
    fn an_unresolved_specifier_is_recorded_as_a_diagnostic() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "\nimport g from \"./ghost\";");
        let graph = build(&dir);

        let diag = graph
            .stats
            .diagnostics
            .iter()
            .find(|d| d.specifier == "./ghost");
        assert!(
            diag.is_some(),
            "expected a diagnostic for the unresolved specifier, got {:?}",
            graph.stats.diagnostics
        );
        let diag = diag.unwrap();
        assert_eq!(diag.kind, DiagnosticKind::Unresolved);
        assert_eq!(diag.path, "src/a.ts");
        assert_eq!(diag.line, 2);
    }

    #[test]
    fn diagnostics_are_capped_but_the_true_totals_are_exact() {
        // 600 comfortably exceeds the 500 cap: proves the vector is bounded
        // while `stats.unresolved` (the auditable total) is not.
        let dir = TempDir::new().unwrap();
        let mut body = String::new();
        for i in 0..600 {
            body.push_str(&format!("import x{i} from \"./ghost{i}\";\n"));
        }
        write(&dir, "src/a.ts", &body);
        let graph = build(&dir);

        assert_eq!(
            graph.stats.unresolved, 600,
            "the true total must never be capped, only the recorded list"
        );
        assert_eq!(
            graph.stats.diagnostics.len(),
            500,
            "the recorded list must stop growing at the documented cap"
        );
    }

    // --- Fix round 1: Finding 3 — a failed file must not leave a dangling edge ---

    /// Test-only wrapper: forces `extract` to fail whenever the source
    /// contains a marker, otherwise delegates to a real `TypeScriptExtractor`.
    /// Real tree-sitter is fault-tolerant (see
    /// `a_syntactically_broken_file_still_yields_what_it_can` in
    /// `extractors::typescript`) and will not fail on any input a test could
    /// plausibly hand it, so exercising the "file fails to parse" branch of
    /// `build_graph` needs this instead.
    struct FailOnMarkerExtractor(TypeScriptExtractor);

    impl Extractor for FailOnMarkerExtractor {
        fn lang(&self) -> &'static str {
            self.0.lang()
        }
        fn extensions(&self) -> &'static [&'static str] {
            self.0.extensions()
        }
        fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
            if source.contains("FORCE_PARSE_FAILURE") {
                return Err(ExtractError::Parse);
            }
            self.0.extract(source)
        }
        fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
            self.0.resolve(raw, importer)
        }
        fn skipped_configs(&self) -> &[SkippedConfig] {
            self.0.skipped_configs()
        }
    }

    #[test]
    fn an_import_into_a_file_that_fails_to_parse_produces_no_dangling_edge() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "// FORCE_PARSE_FAILURE");

        let extractor = FailOnMarkerExtractor(TypeScriptExtractor::new(dir.path()));
        let graph = crate::build_graph(dir.path(), &extractor);

        // "src/b.ts" failed to parse, so it must never become a node...
        assert!(
            !graph.nodes.iter().any(|n| n.id == "src/b.ts"),
            "a file that failed to parse must not become a node"
        );
        // ...and no edge may point at it, regardless.
        assert!(
            graph.edges.iter().all(|e| e.target != "src/b.ts"),
            "no edge may target a node that does not exist, got {:?}",
            graph.edges
        );
        assert_eq!(graph.edges.len(), 0);

        // The failure itself is still recorded...
        assert!(graph
            .stats
            .failures
            .iter()
            .any(|f| f.path == "src/b.ts" && f.reason.starts_with("extract failed")));
        // ...and the import into it lands as `excluded` (a real file exists,
        // it simply never became a scanned node), never silently `resolved`.
        assert_eq!(graph.stats.resolved, 0);
        assert_eq!(graph.stats.excluded, 1);
    }

    // --- Fix round 1: Finding 4 — sort guarantees, actually exercised ---

    #[test]
    fn edges_are_sorted_with_more_than_one_distinct_edge() {
        // Four distinct edges whose collection order out of the internal
        // `HashSet<Edge>` is unrelated to (and, empirically, not already)
        // the sorted order asserted below — see the fix report for the
        // break/restore proof that removing `edges.sort_by` fails this.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import x from "./m";"#);
        write(&dir, "src/b.ts", r#"import x from "./z";"#);
        write(&dir, "src/m.ts", r#"import x from "./b";"#);
        write(&dir, "src/z.ts", r#"import x from "./a";"#);
        let graph = build(&dir);

        let pairs: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .map(|e| (e.source.as_str(), e.target.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("src/a.ts", "src/m.ts"),
                ("src/b.ts", "src/z.ts"),
                ("src/m.ts", "src/b.ts"),
                ("src/z.ts", "src/a.ts"),
            ]
        );
    }

    #[test]
    fn node_sort_uses_string_order_on_the_id_not_path_component_order() {
        // `PathBuf`'s `Ord` compares components, not raw characters: as
        // components, "api" is a strict prefix of "api-client.ts" and so
        // sorts first, meaning `discover()`'s own pre-sort (which sorts
        // `PathBuf`s) places "src/api/client.ts" before
        // "src/api-client.ts". As strings — what the documented,
        // slash-separated node id actually is — '-' (0x2D) sorts before
        // '/' (0x2F), so the correct order is the other way around. This
        // pair only proves `build_graph`'s own `nodes.sort_by` is
        // load-bearing (rather than piggy-backing on `discover`'s pre-sort)
        // because the two orderings genuinely disagree here.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/api-client.ts", "");
        write(&dir, "src/api/client.ts", "");
        let graph = build(&dir);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/api-client.ts", "src/api/client.ts"]);
    }
}
