use crate::catalogue::{self, Kind};
use crate::discover::{is_in_skipped_directory, survey};
use crate::extractor::{Extractor, Resolution, Specifier};
use crate::model::{
    Coverage, Diagnostic, DiagnosticKind, Edge, EdgeKind, ExclusionReason, ExtensionCoverage,
    Failure, FileStatus, Graph, LangStats, Node, NodeKind, SkippedDirectory, Stats,
    MAX_DIAGNOSTICS,
};
use crate::registry::Registry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
struct ParsedFile<'a> {
    id: String,
    path: PathBuf,
    source: String,
    specifiers: Vec<Specifier>,
    /// The extractor that claimed this file, kept so pass 2 resolves each
    /// file's specifiers with the language that wrote them.
    extractor: &'a dyn Extractor,
    /// `Node::lang` for this file, which is not always the extractor's own
    /// label — see [`Extractor::lang_for`].
    lang: &'static str,
    /// Size on disk, carried through so every node reports one.
    bytes: u64,
}

/// Record one non-resolved specifier, capped at `MAX_DIAGNOSTICS` so a huge
/// broken repo cannot produce an unbounded `graph.json`. The precise
/// counters on `Stats` (`unresolved`/`excluded`) are updated by the caller
/// and always hold the true total even once this cap is hit.
fn record_diagnostic(
    stats: &mut Stats,
    path: &str,
    specifier: &Specifier,
    kind: DiagnosticKind,
    reason: ExclusionReason,
    lang: &str,
) {
    if stats.diagnostics.len() < MAX_DIAGNOSTICS {
        stats.diagnostics.push(Diagnostic {
            path: path.to_string(),
            line: specifier.line,
            specifier: specifier.raw.clone(),
            kind,
            reason,
            lang: lang.to_string(),
        });
    }
}

/// A specifier's target, once looked up against the scanned node set.
enum TargetOutcome {
    /// It is a node: this becomes an edge.
    Node(String),
    /// It is not, and this is precisely why.
    Missing(ExclusionReason),
}

/// Whether a reason means the tool (or the import) is broken, rather than
/// the target being deliberately out of scope. This is the single place
/// that decides what depresses the published resolution rate.
fn depresses_the_rate(reason: ExclusionReason) -> bool {
    matches!(
        reason,
        ExclusionReason::NotFound | ExclusionReason::TargetUnreadable
    )
}

/// Everything pass 2 needs to explain a target it could not turn into an
/// edge. Grouped so the explanation is computed in exactly one place.
struct NodeSet<'a> {
    root: &'a Path,
    /// Ids of files that were read, parsed, and became nodes.
    known: HashSet<&'a str>,
    /// Ids of files that were claimed by an extractor but could not be read
    /// or parsed. Their imports are our failure, not an exclusion.
    failed: &'a HashSet<String>,
    /// Ids of every file the walker visited, whatever its extension. A
    /// target here but not in `known` was seen and deliberately not
    /// claimed: an extension no extractor reads.
    seen: &'a HashSet<String>,
}

impl NodeSet<'_> {
    /// Look one resolved target up, and explain it if it is not a node.
    ///
    /// The target is known to exist on disk: an `Extractor` only returns an
    /// internal resolution for a path it probed and found. So every branch
    /// below describes a file that is really there, and the question is
    /// only why it is not in the graph.
    fn classify(&self, target: &Path) -> TargetOutcome {
        let Some(id) = node_id(self.root, target) else {
            // `strip_prefix` failed: the path is not under the scan root at
            // all — a sibling project, or an alias that pointed up and out.
            return TargetOutcome::Missing(ExclusionReason::OutsideRoot);
        };
        if self.known.contains(id.as_str()) {
            return TargetOutcome::Node(id);
        }
        if self.failed.contains(&id) || self.seen.contains(&id) {
            // Seen but not a node means it was claimed and then failed to
            // read or parse — our failure, not an exclusion.
            return TargetOutcome::Missing(ExclusionReason::TargetUnreadable);
        }
        // Never visited. The walker refuses a path for exactly two reasons,
        // so the remaining one is the answer.
        if is_in_skipped_directory(target) {
            TargetOutcome::Missing(ExclusionReason::SkippedDirectory)
        } else {
            TargetOutcome::Missing(ExclusionReason::Gitignored)
        }
    }
}

/// One file the walker visited, and what became of it. Drives the coverage
/// report: the tally is by extension, but the status is per file, since two
/// `.ts` files can end differently (one a node, one unreadable).
struct FileRecord {
    /// Project-relative id, i.e. `Node::id`.
    id: String,
    /// Lowercased, without the leading dot. Empty for an extensionless file.
    extension: String,
    file_name: String,
    status: FileStatus,
    /// The extractor's language label, for claimed files.
    lang: Option<&'static str>,
    /// Size on disk, for the node. Read from the directory entry's
    /// metadata, never by opening the file — which is what makes a PDF or a
    /// 200 MB binary a node at no cost and with no risk.
    bytes: u64,
}

/// Scan a project into a graph. Never panics on a bad file: it is recorded in
/// `stats.failures` and the scan continues.
///
/// Every extractor in `registry` is applied to the files it claims, so one
/// scan of a polyglot repository produces one graph containing all of them.
/// Files claimed by nobody are not dropped silently: each is classified and
/// counted in [`Stats::coverage`], which is what makes "KOG does not read
/// this language yet" a number rather than an omission.
pub fn build_graph(root: &Path, registry: &Registry) -> Graph {
    // `survey` canonicalises its own root internally; do the same here so
    // that `node_id` strips the exact prefix it used. Without this, a
    // non-canonical `root` (e.g. one containing a `..` component) makes
    // every `strip_prefix` below fail silently, and every node is skipped.
    let root: PathBuf = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root: &Path = &root;

    let survey = survey(root);

    // Pass 0: split what the walker saw into files an extractor claims and
    // files it does not, keeping a record of every one either way.
    let mut records: Vec<FileRecord> = Vec::with_capacity(survey.files.len());
    let mut claimed: Vec<(usize, PathBuf, &dyn Extractor)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::with_capacity(survey.files.len());

    for file in &survey.files {
        let Some(id) = node_id(root, file) else {
            continue;
        };
        let extension = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let file_name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        seen.insert(id.clone());

        let bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let index = records.len();
        match registry.for_extension(&extension) {
            Some(extractor) => {
                records.push(FileRecord {
                    id,
                    extension,
                    file_name,
                    // Provisional: pass 1 downgrades it to `Failed` if the
                    // file cannot be read or parsed.
                    status: FileStatus::Analysed,
                    lang: Some(extractor.lang_for(file)),
                    bytes,
                });
                claimed.push((index, file.clone(), extractor));
            }
            None => {
                let classification = catalogue::classify(&extension, &file_name);
                records.push(FileRecord {
                    id,
                    extension,
                    file_name,
                    status: match classification.kind {
                        // Source code nobody claimed: the coverage gap, and
                        // the only status that argues for another extractor.
                        Kind::Source => FileStatus::UnsupportedLanguage,
                        Kind::NotSource | Kind::Unrecognised => FileStatus::NotSource,
                    },
                    lang: classification.lang,
                    bytes,
                });
            }
        }
    }

    let mut stats = Stats {
        files_discovered: claimed.len(),
        ..Default::default()
    };

    // Pass 1: read and parse every claimed file, recording a `Failure`
    // immediately for anything that cannot be. `known` is built only from
    // what survives this pass — computing it from *all* claimed files
    // (before failures are known) would let a specifier into a file that
    // failed be counted `resolved` and produce an edge to a node that is
    // never actually emitted. `failed` records the id of every file that did
    // *not* survive, so pass 2 can tell them apart from a real file that is
    // merely out of scope: an import into a file the tool itself could not
    // read or parse is our failure, not a deliberately excluded target, and
    // must depress the resolution rate like any other broken import.
    let mut parsed: Vec<ParsedFile> = Vec::new();
    let mut failed: HashSet<String> = HashSet::new();
    for (index, file, extractor) in claimed {
        let id = match node_id(root, &file) {
            Some(id) => id,
            None => continue,
        };

        let source = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("read failed: {e}"),
                });
                records[index].status = FileStatus::Failed;
                failed.insert(id);
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
                records[index].status = FileStatus::Failed;
                failed.insert(id);
                continue;
            }
        };
        stats.files_parsed += 1;

        let lang = extractor.lang_for(&file);
        let bytes = records[index].bytes;
        parsed.push(ParsedFile {
            id,
            path: file,
            source,
            specifiers,
            extractor,
            lang,
            bytes,
        });
    }

    // Every file the walker visited is a node — an image, a PDF, a
    // lockfile, a language KOG cannot parse — except the ones it claimed
    // and then could not read, which must never be an edge's target.
    // A map that omits what it cannot read has holes in it shaped like
    // empty space; an import of `./logo.png` points at something real, and
    // the graph should say so.
    let node_set = NodeSet {
        root,
        known: records
            .iter()
            .filter(|r| r.status != FileStatus::Failed)
            .map(|r| r.id.as_str())
            .collect(),
        failed: &failed,
        seen: &seen,
    };

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: HashSet<Edge> = HashSet::new();
    let mut external_names: BTreeSet<String> = BTreeSet::new();
    let mut by_lang: BTreeMap<String, LangStats> = BTreeMap::new();

    // Pass 2: resolve every specifier now that the node set is final.
    for file in &parsed {
        let mut node_externals: BTreeSet<String> = BTreeSet::new();
        let lang_stats = by_lang.entry(file.lang.to_string()).or_default();
        lang_stats.files += 1;

        for specifier in &file.specifiers {
            stats.specifiers_total += 1;
            let resolution = file.extractor.resolve(&specifier.raw, &file.path);

            // Counters are kept in step for the whole scan and for this
            // file's language at once: a per-language rate computed from a
            // different set of events than the global one would be a second
            // number that cannot be reconciled with the first.
            let lang_stats = by_lang.entry(file.lang.to_string()).or_default();
            lang_stats.specifiers_total += 1;

            match resolution {
                Resolution::External(package) => {
                    stats.external_specifiers += 1;
                    lang_stats.external_specifiers += 1;
                    external_names.insert(package.clone());
                    node_externals.insert(package);
                }
                Resolution::Unresolved => {
                    stats.specifiers_internal += 1;
                    stats.unresolved += 1;
                    lang_stats.specifiers_internal += 1;
                    lang_stats.unresolved += 1;
                    record_diagnostic(
                        &mut stats,
                        &file.id,
                        specifier,
                        DiagnosticKind::Unresolved,
                        ExclusionReason::NotFound,
                        file.lang,
                    );
                }
                internal => {
                    stats.specifiers_internal += 1;
                    let lang_stats = by_lang.entry(file.lang.to_string()).or_default();
                    lang_stats.specifiers_internal += 1;

                    // One specifier can name several files (a Go package, a
                    // C# namespace). It counts once towards the rate and
                    // contributes an edge per target that is a node.
                    let mut resolved_any = false;
                    let mut first_reason: Option<ExclusionReason> = None;
                    for target in internal.targets() {
                        match node_set.classify(target) {
                            TargetOutcome::Node(target_id) => {
                                resolved_any = true;
                                if target_id != file.id {
                                    edges.insert(Edge {
                                        source: file.id.clone(),
                                        target: target_id,
                                        kind: EdgeKind::Import,
                                    });
                                }
                            }
                            TargetOutcome::Missing(reason) => {
                                // Keep the *worst* reason, not the first:
                                // if any target of a set is merely out of
                                // scope while another is genuinely broken,
                                // the broken one is what the reader needs.
                                let keep = match first_reason {
                                    Some(existing) if depresses_the_rate(existing) => existing,
                                    _ => reason,
                                };
                                first_reason = Some(keep);
                            }
                        }
                    }

                    let lang_stats = by_lang.entry(file.lang.to_string()).or_default();
                    if resolved_any {
                        stats.resolved += 1;
                        lang_stats.resolved += 1;
                        continue;
                    }

                    // An empty target set is a resolver that claimed
                    // "internal" and produced nothing: nothing was found.
                    let reason = first_reason.unwrap_or(ExclusionReason::NotFound);
                    let kind = if depresses_the_rate(reason) {
                        stats.unresolved += 1;
                        lang_stats.unresolved += 1;
                        DiagnosticKind::Unresolved
                    } else {
                        stats.excluded += 1;
                        lang_stats.excluded += 1;
                        DiagnosticKind::Excluded
                    };
                    record_diagnostic(&mut stats, &file.id, specifier, kind, reason, file.lang);
                }
            }
        }

        nodes.push(Node {
            id: file.id.clone(),
            path: file.id.clone(),
            lang: file.lang.to_string(),
            kind: NodeKind::Source,
            loc: file.source.lines().count(),
            bytes: file.bytes,
            external_deps: node_externals.into_iter().collect(),
        });
    }

    // Every remaining file: read by nobody, node all the same. `loc` is
    // zero because the file was never opened — `bytes` is the measure that
    // means something for a PNG.
    let parsed_ids: HashSet<&str> = parsed.iter().map(|p| p.id.as_str()).collect();
    for record in &records {
        if record.status == FileStatus::Failed || parsed_ids.contains(record.id.as_str()) {
            continue;
        }
        nodes.push(Node {
            id: record.id.clone(),
            path: record.id.clone(),
            lang: record
                .lang
                .map(|l| l.to_ascii_lowercase())
                .unwrap_or_else(|| "unknown".to_string()),
            kind: match record.status {
                FileStatus::UnsupportedLanguage => NodeKind::UnreadSource,
                _ => NodeKind::Asset,
            },
            loc: 0,
            bytes: record.bytes,
            external_deps: Vec::new(),
        });
    }

    // A config an extractor could not use (e.g. an unreadable or malformed
    // tsconfig) degrades resolution for a whole subtree; design doc §7
    // demands that never disappear silently. The reason is prefixed so it
    // reads distinctly from a source-file failure.
    for extractor in registry.extractors() {
        for skipped in extractor.skipped_configs() {
            let path = node_id_lenient(root, &skipped.path)
                .unwrap_or_else(|| skipped.path.to_string_lossy().into_owned());
            stats.failures.push(Failure {
                path,
                reason: format!("tsconfig skipped: {}", skipped.reason),
            });
        }
    }
    stats.failures.sort_by(|a, b| a.path.cmp(&b.path));

    stats.external_packages_distinct = external_names.len();
    stats.resolution_rate = stats.resolution_rate();

    let mut edges: Vec<Edge> = edges.into_iter().collect();
    edges.sort_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    // Edges are attributed to the language of the file that wrote them, and
    // only once the set has been deduplicated — counting them as they are
    // inserted would count a repeated import twice.
    let lang_of: HashMap<&str, &str> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.lang.as_str()))
        .collect();
    for edge in &edges {
        if let Some(lang) = lang_of.get(edge.source.as_str()) {
            by_lang.entry((*lang).to_string()).or_default().edges += 1;
        }
    }
    for lang in by_lang.values_mut() {
        lang.resolution_rate = lang.resolution_rate();
    }

    stats.by_lang = by_lang;
    stats.coverage = coverage_of(&records, &survey.skipped_directories);

    Graph {
        nodes,
        edges,
        stats,
    }
}

/// Aggregate the per-file records into the coverage report.
fn coverage_of(records: &[FileRecord], skipped: &BTreeMap<String, usize>) -> Coverage {
    let mut coverage = Coverage {
        files_seen: records.len(),
        ..Default::default()
    };

    // Keyed by (extension, status): one extension can appear twice when two
    // files with it ended differently — a `.ts` file that parsed and one
    // that could not. Collapsing those into one row would hide the failure.
    let mut tally: BTreeMap<(String, FileStatus), (usize, Option<&'static str>, String)> =
        BTreeMap::new();
    for record in records {
        match record.status {
            FileStatus::Analysed => coverage.files_analysed += 1,
            FileStatus::UnsupportedLanguage => coverage.files_unsupported += 1,
            FileStatus::NotSource => coverage.files_not_source += 1,
            // A file that failed is neither covered nor a language gap; it
            // is a failure, already named in `stats.failures`.
            FileStatus::Failed => {}
        }
        // An extensionless file is reported under its own name (`Makefile`,
        // `Dockerfile`), which is the only identity it has.
        let (key, label) = if record.extension.is_empty() {
            (record.file_name.clone(), record.file_name.clone())
        } else {
            (record.extension.clone(), format!(".{}", record.extension))
        };
        let entry = tally
            .entry((key, record.status))
            .or_insert((0, record.lang, label));
        entry.0 += 1;
    }

    coverage.extensions = tally
        .into_iter()
        .map(
            |((extension, status), (count, lang, label))| ExtensionCoverage {
                extension,
                label,
                count,
                status,
                lang: lang.map(str::to_string),
                note: note_for(status, lang),
            },
        )
        .collect();
    // Most files first, then alphabetically, so the table reads as a
    // priority list and two runs on an unchanged tree agree byte for byte.
    coverage
        .extensions
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.extension.cmp(&b.extension)));

    coverage.skipped_directories = skipped
        .iter()
        .map(|(name, count)| SkippedDirectory {
            name: name.clone(),
            count: *count,
            rule: "always-skip".to_string(),
        })
        .collect();
    coverage
        .skipped_directories
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

    coverage
}

/// The human-readable reason a group of files is not in the graph.
fn note_for(status: FileStatus, lang: Option<&'static str>) -> Option<String> {
    match status {
        FileStatus::Analysed => None,
        FileStatus::Failed => Some("read or parse failed".to_string()),
        FileStatus::UnsupportedLanguage => Some("no extractor for this language yet".to_string()),
        FileStatus::NotSource => Some(match lang {
            Some(family) => family.to_string(),
            None => "unrecognised extension".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
    use crate::extractors::TypeScriptExtractor;
    use crate::model::{DiagnosticKind, ExclusionReason, FileStatus, Graph, NodeKind};
    use crate::registry::Registry;
    use crate::tsconfig::SkippedConfig;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn build_at(root: &Path) -> Graph {
        let registry = Registry::single(Box::new(TypeScriptExtractor::new(root)));
        crate::build_graph(root, &registry)
    }

    fn build(dir: &TempDir) -> Graph {
        build_at(dir.path())
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
        // The original version of this test used a target that did not
        // exist on disk at all, so `resolve` returned `Unresolved` before
        // ever reaching the `strip_prefix`-based containment check its name
        // implies — its sole assertion passed for the wrong reason. Fixed
        // here to genuinely exercise that branch: a real file, sitting
        // above the scanned root, reached by a relative import that
        // actually resolves to it.
        let outer = TempDir::new().unwrap();
        write(&outer, "outside/x.ts", "");
        write(
            &outer,
            "project/src/a.ts",
            r#"import x from "../../outside/x";"#,
        );
        let root = outer.path().join("project");
        let graph = build_at(&root);

        assert_eq!(
            graph.edges.len(),
            0,
            "no edge may point at a node outside the scanned root"
        );
        // Not merely dropped: a real file was found, just outside the
        // scanned root, so it is counted `excluded` — the same treatment as
        // any other resolved-but-out-of-scope target — never `unresolved`
        // (the tool did not fail here) and never silently ignored.
        assert_eq!(graph.stats.excluded, 1);
        assert_eq!(graph.stats.unresolved, 0);
        // And the reason names the containment rule that rejected it, not
        // just "excluded".
        assert_eq!(
            graph.stats.diagnostics[0].reason,
            ExclusionReason::OutsideRoot
        );
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
        // `survey` canonicalises its root internally. If `build_graph`
        // does not canonicalise the same way before computing node ids, the
        // `strip_prefix` inside `node_id` silently fails against the
        // canonical paths it hands back, and every node is skipped.
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

        let graph = build_at(&noncanonical_root);

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

        let graph = build_at(&symlinked_root);

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
        // v0.1: the *why* is recorded, not just the fact. This is the one
        // that makes the measurement documents regenerable with `jq`.
        assert_eq!(diag.reason, ExclusionReason::Gitignored);
        assert_eq!(diag.lang, "typescript");
    }

    #[test]
    fn an_import_into_a_skipped_directory_says_so() {
        // `dist/` is refused by the always-skip list, not by `.gitignore`
        // — the two are different answers to "why is this not a node?" and
        // the diagnostic must not conflate them.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import g from "../dist/bundle";"#);
        write(&dir, "dist/bundle.ts", "");
        let graph = build(&dir);

        assert_eq!(graph.stats.excluded, 1);
        assert_eq!(
            graph.stats.diagnostics[0].reason,
            ExclusionReason::SkippedDirectory
        );
    }

    #[test]
    fn an_import_into_a_language_with_no_extractor_still_becomes_an_edge() {
        // The file is right there and the import names it correctly, so the
        // edge is real whether or not KOG can read Svelte. What is missing
        // is the *other* end — the widget's own imports — and that is what
        // the coverage report is for, not the resolution rate.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import C from "./widget.svelte";"#);
        write(&dir, "src/widget.svelte", "<script></script>");
        let graph = build(&dir);

        assert_eq!(graph.stats.resolved, 1);
        assert_eq!(graph.stats.excluded, 0);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].target, "src/widget.svelte");

        let widget = graph
            .nodes
            .iter()
            .find(|n| n.id == "src/widget.svelte")
            .expect("an unreadable language is still a node");
        assert_eq!(widget.kind, NodeKind::UnreadSource);
        assert_eq!(graph.stats.coverage.files_unsupported, 1);
    }

    #[test]
    fn every_file_becomes_a_node_including_the_ones_nothing_can_read() {
        // A PDF and a PNG are never opened — reading them as text would
        // fail — but they are part of the repository and so part of the
        // map. `bytes` is what measures them; `loc` is zero by definition.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "export const a = 1;");
        write(&dir, "docs/manual.pdf", "%PDF-1.7 not really");
        write(&dir, "assets/logo.png", "\u{0}\u{1}binary-ish");
        let graph = build(&dir);

        assert_eq!(graph.nodes.len(), 3);
        let png = graph
            .nodes
            .iter()
            .find(|n| n.id == "assets/logo.png")
            .expect("an image is a node");
        assert_eq!(png.kind, NodeKind::Asset);
        assert_eq!(png.lang, "image");
        assert_eq!(png.loc, 0, "an asset is never opened, so it has no lines");
        assert!(png.bytes > 0, "an asset is measured in bytes");
        assert!(png.external_deps.is_empty());
    }

    #[test]
    fn an_asset_an_import_names_is_a_real_edge() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import logo from "./logo.png";"#);
        write(&dir, "src/logo.png", "binary");
        let graph = build(&dir);

        assert_eq!(graph.stats.resolved, 1);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].target, "src/logo.png");
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
        assert_eq!(diag.reason, ExclusionReason::NotFound);
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
        fn lang_for(&self, path: &Path) -> &'static str {
            self.0.lang_for(path)
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

    fn build_failing(root: &Path) -> Graph {
        let registry = Registry::single(Box::new(FailOnMarkerExtractor(TypeScriptExtractor::new(
            root,
        ))));
        crate::build_graph(root, &registry)
    }

    #[test]
    fn an_import_into_a_file_that_fails_to_parse_produces_no_dangling_edge() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "// FORCE_PARSE_FAILURE");

        let graph = build_failing(dir.path());

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
        // ...and the import into it lands as `unresolved`: the target file
        // genuinely exists, but *our own tool* failed to read or parse it —
        // that is our failure, not an out-of-scope target, so it must
        // depress the resolution rate exactly like any other broken import.
        // It must never be silently `resolved`, and must never be
        // `excluded` either (that would let a broken extractor hide behind
        // a perfect published rate).
        assert_eq!(graph.stats.resolved, 0);
        assert_eq!(graph.stats.unresolved, 1);
        assert_eq!(graph.stats.excluded, 0);
        assert_eq!(
            graph.stats.diagnostics[0].reason,
            ExclusionReason::TargetUnreadable
        );
    }

    #[test]
    fn several_files_failing_to_parse_measurably_drop_the_resolution_rate() {
        // Guards against the failure mode this whole fix exists for: an
        // extractor broken on a chunk of the codebase must not leave the
        // published rate at a deceptive 1.0000. Three files fail to parse,
        // each imported once, and none of those imports may land anywhere
        // but `unresolved`.
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import b1 from "./b1";
import b2 from "./b2";
import b3 from "./b3";"#,
        );
        write(&dir, "src/b1.ts", "// FORCE_PARSE_FAILURE");
        write(&dir, "src/b2.ts", "// FORCE_PARSE_FAILURE");
        write(&dir, "src/b3.ts", "// FORCE_PARSE_FAILURE");

        let graph = build_failing(dir.path());

        assert_eq!(graph.stats.failures.len(), 3);
        assert_eq!(graph.stats.resolved, 0);
        assert_eq!(graph.stats.unresolved, 3);
        assert_eq!(graph.stats.excluded, 0);
        assert!(
            graph.stats.resolution_rate < 1.0,
            "a parser broken on multiple files must measurably depress the \
             published rate, got {}",
            graph.stats.resolution_rate
        );
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
        // sorts first, meaning the survey's own pre-sort (which sorts
        // `PathBuf`s) places "src/api/client.ts" before
        // "src/api-client.ts". As strings — what the documented,
        // slash-separated node id actually is — '-' (0x2D) sorts before
        // '/' (0x2F), so the correct order is the other way around. This
        // pair only proves `build_graph`'s own `nodes.sort_by` is
        // load-bearing (rather than piggy-backing on the survey's pre-sort)
        // because the two orderings genuinely disagree here.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/api-client.ts", "");
        write(&dir, "src/api/client.ts", "");
        let graph = build(&dir);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/api-client.ts", "src/api/client.ts"]);
    }

    // --- Coverage: what the scan saw and did not read ---

    #[test]
    fn coverage_accounts_for_every_file_the_walker_visited() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.tsx", "");
        write(&dir, "main.go", "package main");
        write(&dir, "README.md", "# hi");
        write(&dir, "logo.png", "");
        let graph = build(&dir);

        let coverage = &graph.stats.coverage;
        assert_eq!(coverage.files_seen, 5);
        assert_eq!(coverage.files_analysed, 2, "only the two TypeScript files");
        assert_eq!(coverage.files_unsupported, 1, "main.go is a real gap");
        assert_eq!(coverage.files_not_source, 2, "the readme and the image");
        assert_eq!(
            coverage.files_analysed + coverage.files_unsupported + coverage.files_not_source,
            coverage.files_seen,
            "every visited file must land in exactly one bucket"
        );
    }

    #[test]
    fn an_unsupported_language_is_named_in_the_coverage_table() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "cmd/main.go", "package main");
        write(&dir, "cmd/other.go", "package main");
        let graph = build(&dir);

        let entry = graph
            .stats
            .coverage
            .extensions
            .iter()
            .find(|e| e.extension == "go")
            .expect("go must appear in the coverage table");
        assert_eq!(entry.count, 2);
        assert_eq!(entry.status, FileStatus::UnsupportedLanguage);
        assert_eq!(
            entry.lang.as_deref(),
            Some("Go"),
            "an unsupported entry must name the language, not just the extension"
        );
        assert!(entry.note.is_some());
    }

    #[test]
    fn source_coverage_ignores_documentation_and_assets() {
        let dir = TempDir::new().unwrap();
        write(&dir, "a.ts", "");
        write(&dir, "b.go", "");
        for i in 0..20 {
            write(&dir, &format!("docs/{i}.md"), "");
        }
        let graph = build(&dir);
        // One of two source files read: 0.5, not 2/22.
        assert!((graph.stats.coverage.source_coverage() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn refused_directories_are_named_with_their_rule() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "node_modules/pkg/index.ts", "");
        let graph = build(&dir);

        let skipped = &graph.stats.coverage.skipped_directories;
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "node_modules");
        assert_eq!(skipped[0].count, 1);
        assert_eq!(skipped[0].rule, "always-skip");
    }

    #[test]
    fn a_file_that_failed_is_neither_covered_nor_a_language_gap() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.ts", "// FORCE_PARSE_FAILURE");
        let graph = build_failing(dir.path());

        let coverage = &graph.stats.coverage;
        assert_eq!(coverage.files_seen, 2);
        assert_eq!(coverage.files_analysed, 1);
        assert_eq!(coverage.files_unsupported, 0);
        assert_eq!(coverage.files_not_source, 0);
        let failed = coverage
            .extensions
            .iter()
            .find(|e| e.status == FileStatus::Failed)
            .expect("the failure must still appear in the table");
        assert_eq!(failed.count, 1);
    }

    // --- Per-language statistics ---

    #[test]
    fn typescript_and_javascript_are_counted_as_different_languages() {
        // One extractor, two languages: `.js` is not TypeScript, and a
        // per-language table that said otherwise would misreport every
        // JavaScript project as a TypeScript one.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "");
        write(&dir, "src/legacy.js", r#"import x from "./ghost";"#);
        let graph = build(&dir);

        let by_lang = &graph.stats.by_lang;
        assert_eq!(by_lang["typescript"].files, 2);
        assert_eq!(by_lang["typescript"].resolved, 1);
        assert_eq!(by_lang["typescript"].edges, 1);
        assert_eq!(by_lang["javascript"].files, 1);
        assert_eq!(by_lang["javascript"].unresolved, 1);
        assert_eq!(by_lang["javascript"].edges, 0);
        assert_eq!(
            by_lang["javascript"].resolution_rate, 0.0,
            "a language whose every import is broken must publish 0, not the \
             project-wide average"
        );
        assert_eq!(by_lang["typescript"].resolution_rate, 1.0);
        // The node itself carries the same label.
        let legacy = graph
            .nodes
            .iter()
            .find(|n| n.id == "src/legacy.js")
            .unwrap();
        assert_eq!(legacy.lang, "javascript");
    }

    #[test]
    fn per_language_counters_sum_to_the_project_totals() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import React from "react";
import b from "./b";
import ghost from "./ghost";"#,
        );
        write(&dir, "src/b.ts", "");
        write(&dir, "src/c.js", r#"import b from "./b";"#);
        let graph = build(&dir);

        let sum = |f: fn(&crate::model::LangStats) -> usize| -> usize {
            graph.stats.by_lang.values().map(f).sum()
        };
        assert_eq!(sum(|l| l.specifiers_total), graph.stats.specifiers_total);
        assert_eq!(
            sum(|l| l.specifiers_internal),
            graph.stats.specifiers_internal
        );
        assert_eq!(sum(|l| l.resolved), graph.stats.resolved);
        assert_eq!(sum(|l| l.unresolved), graph.stats.unresolved);
        assert_eq!(sum(|l| l.excluded), graph.stats.excluded);
        assert_eq!(sum(|l| l.edges), graph.edges.len());
        assert_eq!(sum(|l| l.files), graph.nodes.len());
    }
}
