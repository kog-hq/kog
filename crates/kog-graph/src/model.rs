use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What KOG did with a file, which is not the same question as what the
/// file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Code in a language KOG reads: its imports are in the graph.
    Source,
    /// Code in a language KOG does not read yet. It is a node — things can
    /// point at it — but its own imports are missing, and that is exactly
    /// what [`Coverage`] measures.
    UnreadSource,
    /// Anything else: documentation, data, images, fonts, PDFs, binaries.
    /// A node with no outgoing edges, because there are none to find.
    /// Never opened — a PNG is not read as text, it is measured and placed.
    Asset,
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            NodeKind::Source => "source",
            NodeKind::UnreadSource => "unread source",
            NodeKind::Asset => "asset",
        };
        write!(f, "{text}")
    }
}

/// One file in the scanned project. Every file, not only the ones KOG can
/// parse.
///
/// A map that omits what it cannot read is a map with holes in it that look
/// like empty space. An image, a PDF, a lockfile and a Haskell module are
/// all nodes here; what differs is [`Node::kind`] and whether anything
/// leaves them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Project-relative path, also used as the graph identity.
    pub id: String,
    pub path: String,
    /// The language for source, the file family (`image`, `documentation`,
    /// `data`) for an asset.
    pub lang: String,
    pub kind: NodeKind,
    /// Lines of code. Zero for a file that was never opened — an asset is
    /// measured in [`Node::bytes`] instead.
    pub loc: usize,
    /// Size on disk. The one measure that means something for every file
    /// type, including the ones with no lines at all.
    pub bytes: u64,
    /// External package names this file imports, never graph edges.
    pub external_deps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
}

/// A file that could not be read or parsed. Never fatal, always reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

/// Why one specifier did not become a graph edge. Distinct from a
/// [`Failure`]: nothing here means the parser or resolver malfunctioned —
/// see the field on each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// The specifier looks internal, but either no file was found for it
    /// anywhere on disk, or a file was found and the tool itself failed to
    /// read or parse it (our failure, not the target's) — a genuinely
    /// broken import either way. Depresses the resolution rate.
    Unresolved,
    /// The specifier resolved to a real file, but that file falls outside
    /// the scanned node set (gitignored, inside an always-skipped
    /// directory, or an extension no extractor claims) — e.g. a generated
    /// Prisma client. A deliberate exclusion, not a parser failure; left
    /// out of the resolution rate's denominator.
    Excluded,
}

impl std::fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticKind::Unresolved => write!(f, "unresolved"),
            DiagnosticKind::Excluded => write!(f, "excluded"),
        }
    }
}

/// The precise cause behind a [`Diagnostic`], one level finer than its
/// [`DiagnosticKind`].
///
/// The kind alone answers "did this count against the rate?"; the reason
/// answers "what do I do about it?", which is the question a reader of the
/// measurement documents actually has. Without it, reproducing the
/// categorisation table in `docs/measurements/` means inspecting the
/// filesystem by hand — see the v0.1 entry in `ROADMAP.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Nothing exists on disk under any name this resolver would try.
    /// Pairs with [`DiagnosticKind::Unresolved`].
    NotFound,
    /// The target file exists, but KOG itself could not read or parse it,
    /// so it never became a node. Our failure, not the import's, and it
    /// depresses the rate like any other broken import.
    /// Pairs with [`DiagnosticKind::Unresolved`].
    TargetUnreadable,
    /// The target file exists but lies outside the scanned root — a
    /// sibling project, or a path an alias pointed up and out of.
    OutsideRoot,
    /// The target file exists inside the root, in a directory the walker
    /// never enters (`node_modules`, `dist`, `target`, …).
    SkippedDirectory,
    /// The target file exists inside the root and is excluded by a
    /// `.gitignore` rule — typically generated code, absent from a fresh
    /// clone but present after a build step.
    Gitignored,
}

impl std::fmt::Display for ExclusionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            ExclusionReason::NotFound => "not found",
            ExclusionReason::TargetUnreadable => "target unreadable",
            ExclusionReason::OutsideRoot => "outside root",
            ExclusionReason::SkippedDirectory => "skipped directory",
            ExclusionReason::Gitignored => "gitignored",
        };
        write!(f, "{text}")
    }
}

/// One specifier that did not become an edge, identified precisely enough
/// to find and fix in the source. Design doc §7: a count alone is not
/// auditable, only the identity of what failed is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The importing file's id (project-relative), i.e. `Node::id`.
    pub path: String,
    /// One-based line number of the import statement.
    pub line: usize,
    /// The raw specifier text, exactly as written in the source.
    pub specifier: String,
    pub kind: DiagnosticKind,
    /// Why, precisely. Every diagnostic carries one, so the categorisation
    /// tables in `docs/measurements/` are regenerable with `jq` alone.
    pub reason: ExclusionReason,
    /// The language of the file that wrote the specifier, so a rate can be
    /// audited per language without re-deriving it from the path.
    pub lang: String,
}

/// What became of one file the walker visited. Drives the coverage report:
/// every file under the scan root falls into exactly one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// Claimed by an extractor, read, parsed: it is a node in the graph.
    Analysed,
    /// Source code KOG recognises by extension but has no extractor for.
    /// The honest coverage gap — every one of these is named.
    UnsupportedLanguage,
    /// Not source code: documentation, data, lockfiles, images, binaries.
    /// Never graphed, and not a gap.
    NotSource,
    /// Claimed by an extractor but unreadable or unparseable, so it never
    /// became a node. Also recorded in [`Stats::failures`].
    Failed,
}

impl std::fmt::Display for FileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            FileStatus::Analysed => "analysed",
            FileStatus::UnsupportedLanguage => "unsupported",
            FileStatus::NotSource => "not source",
            FileStatus::Failed => "failed",
        };
        write!(f, "{text}")
    }
}

/// One extension's share of the scanned tree, and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionCoverage {
    /// Without the leading dot, so `jq 'select(.extension == "go")'` works.
    /// For a file that has no extension, this is its whole name.
    pub extension: String,
    /// How to write it in a report: `.go`, or `Dockerfile` for a file whose
    /// identity is its name. Printing `.Dockerfile` would name a thing that
    /// does not exist.
    pub label: String,
    pub count: usize,
    pub status: FileStatus,
    /// The language this extension belongs to, named whether or not KOG
    /// can read it — an unsupported entry is useless if it does not say
    /// *which* language is missing.
    pub lang: Option<String>,
    /// Why this extension is not analysed, when it is not. `None` for
    /// analysed extensions.
    pub note: Option<String>,
}

/// A directory the walker refused to enter, and how often.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedDirectory {
    /// The directory's own name, e.g. `node_modules`.
    pub name: String,
    /// How many directories with that name were refused.
    pub count: usize,
    /// The rule that refused it: `always-skip` or `gitignore`.
    pub rule: String,
}

/// What KOG saw, and what it did with it — file by file, not just for the
/// files it understood.
///
/// The resolution rate answers "of the imports I read, how many resolved?".
/// It says nothing about the files never read at all. A tool that supports
/// one language and silently drops the other nine scores 1.0000 on a
/// polyglot repository. This is the number that stops that from being
/// possible: every file the walker visited is accounted for here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    /// Every file the walker visited, whatever its extension.
    pub files_seen: usize,
    /// Files that became nodes.
    pub files_analysed: usize,
    /// Files whose extension names a language with no extractor yet.
    pub files_unsupported: usize,
    /// Files that are not source code at all.
    pub files_not_source: usize,
    /// One entry per distinct extension, most files first, then by
    /// extension name so two runs on an unchanged tree agree byte for byte.
    pub extensions: Vec<ExtensionCoverage>,
    /// Directories never entered, by name.
    pub skipped_directories: Vec<SkippedDirectory>,
}

impl Coverage {
    /// Analysed files over files that are source code at all (analysed plus
    /// unsupported). Files that are not source — documentation, data,
    /// images — leave the denominator, exactly as external specifiers leave
    /// the resolution rate's.
    ///
    /// 1.0 when there is no source code to speak of, for the same reason
    /// [`Stats::resolution_rate`] is: nothing was missed.
    pub fn source_coverage(&self) -> f64 {
        let denominator = self.files_analysed + self.files_unsupported;
        if denominator == 0 {
            return 1.0;
        }
        self.files_analysed as f64 / denominator as f64
    }
}

/// The same counters as [`Stats`], for one language alone.
///
/// The roadmap's rule is that a language ships when it passes its own
/// resolution gate — which requires the rate to exist per language, not
/// only in aggregate. A repository that is 95 % TypeScript can hide a
/// broken Go resolver behind a healthy overall number.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LangStats {
    pub files: usize,
    pub specifiers_total: usize,
    pub specifiers_internal: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub excluded: usize,
    pub external_specifiers: usize,
    /// Serialised snapshot of [`LangStats::resolution_rate`].
    pub resolution_rate: f64,
    /// Edges whose *source* is a file of this language.
    pub edges: usize,
}

impl LangStats {
    /// Same formula as [`Stats::resolution_rate`], restricted to one
    /// language.
    pub fn resolution_rate(&self) -> f64 {
        let denominator = self.specifiers_internal.saturating_sub(self.excluded);
        if denominator == 0 {
            return 1.0;
        }
        self.resolved as f64 / denominator as f64
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub specifiers_total: usize,
    /// Specifiers whose target looks internal to the project (a relative
    /// import, a resolved alias, or a workspace package): the sum of
    /// `resolved`, `unresolved`, and `excluded`.
    pub specifiers_internal: usize,
    /// Resolved to a real file inside the scanned node set: this specifier
    /// became (or would have become, after edge dedup) a graph edge.
    pub resolved: usize,
    /// No file was found for this specifier at all. See
    /// [`DiagnosticKind::Unresolved`]; each occurrence is also recorded in
    /// `diagnostics` (subject to the cap documented there).
    pub unresolved: usize,
    /// Resolved to a real file that falls outside the scanned node set. See
    /// [`DiagnosticKind::Excluded`]; each occurrence is also recorded in
    /// `diagnostics` (subject to the cap documented there).
    pub excluded: usize,
    /// Serialised snapshot of the resolution rate. The `resolution_rate()` method is the
    /// source of truth; the graph assembler must populate this field from the method.
    pub resolution_rate: f64,
    pub external_specifiers: usize,
    pub external_packages_distinct: usize,
    pub failures: Vec<Failure>,
    /// One entry per `unresolved` or `excluded` specifier, capped at
    /// [`MAX_DIAGNOSTICS`] so a huge broken repo cannot produce an unbounded
    /// `graph.json`. `unresolved` and `excluded` above always hold the true
    /// totals even when this list was capped.
    pub diagnostics: Vec<Diagnostic>,
    /// The same counters, per language. Keyed by `Node::lang`.
    pub by_lang: BTreeMap<String, LangStats>,
    /// What the walker saw and what became of it, whatever the language.
    pub coverage: Coverage,
}

/// Cap on `Stats::diagnostics`. Chosen to comfortably cover a real broken
/// subtree (the acceptance target's own worst case is under 100) while
/// bounding `graph.json` size against a pathological repo with thousands of
/// broken imports; `unresolved`/`excluded` remain exact regardless.
pub const MAX_DIAGNOSTICS: usize = 500;

impl Stats {
    /// Resolved over "in-scope" internal specifiers: internal specifiers
    /// minus those `excluded` (resolved to a real file outside the scanned
    /// set). `excluded` is left out of the denominator for the same reason
    /// external specifiers already are — the resolver worked correctly, the
    /// target is just deliberately out of scope, so counting it against the
    /// rate would understate resolver quality rather than measure it.
    pub fn resolution_rate(&self) -> f64 {
        let denominator = self.specifiers_internal.saturating_sub(self.excluded);
        if denominator == 0 {
            return 1.0;
        }
        self.resolved as f64 / denominator as f64
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub stats: Stats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_computes_the_resolution_rate_over_internal_specifiers_only() {
        let stats = Stats {
            specifiers_internal: 3209,
            resolved: 3140,
            unresolved: 69,
            ..Default::default()
        };
        assert!((stats.resolution_rate() - 0.9785).abs() < 1e-4);
    }

    #[test]
    fn resolution_rate_is_one_when_there_is_nothing_to_resolve() {
        let stats = Stats::default();
        assert_eq!(stats.resolution_rate(), 1.0);
    }

    #[test]
    fn resolution_rate_excludes_out_of_scope_specifiers_from_the_denominator() {
        // 8 resolved out of 10 internal specifiers, 1 of which is merely
        // `excluded` (a real file outside the scanned set, not a parser
        // failure). The old formula (resolved / specifiers_internal) would
        // give 8/10 = 0.8; the correct formula removes the exclusion from
        // the denominator entirely, exactly like an external specifier:
        // 8 resolved out of (10 - 1) = 9 in-scope internal specifiers.
        let stats = Stats {
            specifiers_internal: 10,
            resolved: 8,
            unresolved: 1,
            excluded: 1,
            ..Default::default()
        };
        assert!((stats.resolution_rate() - (8.0 / 9.0)).abs() < 1e-9);
    }

    #[test]
    fn a_language_computes_its_own_rate_with_the_same_formula() {
        let lang = LangStats {
            specifiers_internal: 10,
            resolved: 8,
            unresolved: 1,
            excluded: 1,
            ..Default::default()
        };
        assert!((lang.resolution_rate() - (8.0 / 9.0)).abs() < 1e-9);
    }

    #[test]
    fn source_coverage_leaves_non_source_files_out_of_the_denominator() {
        // 8 analysed, 2 unsupported, and 90 files that are not source code
        // at all: the coverage number is 8/10, not 8/100. Markdown and PNGs
        // are not a coverage gap.
        let coverage = Coverage {
            files_seen: 100,
            files_analysed: 8,
            files_unsupported: 2,
            files_not_source: 90,
            ..Default::default()
        };
        assert!((coverage.source_coverage() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn source_coverage_is_one_when_there_is_no_source_code_at_all() {
        let coverage = Coverage {
            files_seen: 12,
            files_not_source: 12,
            ..Default::default()
        };
        assert_eq!(coverage.source_coverage(), 1.0);
    }

    #[test]
    fn a_graph_serialises_to_the_documented_shape() {
        let graph = Graph {
            nodes: vec![Node {
                id: "src/a.ts".into(),
                path: "src/a.ts".into(),
                lang: "typescript".into(),
                kind: NodeKind::Source,
                loc: 12,
                bytes: 340,
                external_deps: vec!["react".into()],
            }],
            edges: vec![Edge {
                source: "src/a.ts".into(),
                target: "src/b.ts".into(),
                kind: EdgeKind::Import,
            }],
            stats: Stats::default(),
        };
        let json = serde_json::to_value(&graph).unwrap();
        assert_eq!(json["nodes"][0]["external_deps"][0], "react");
        assert_eq!(json["nodes"][0]["kind"], "source");
        assert_eq!(json["nodes"][0]["bytes"], 340);
        assert_eq!(json["edges"][0]["kind"], "import");
    }

    #[test]
    fn every_node_kind_serialises_to_a_stable_name() {
        let kinds = [NodeKind::Source, NodeKind::UnreadSource, NodeKind::Asset];
        let json = serde_json::to_value(kinds).unwrap();
        assert_eq!(json[0], "source");
        assert_eq!(json[1], "unread_source");
        assert_eq!(json[2], "asset");
    }

    #[test]
    fn stats_all_fields_serialise_with_documented_keys() {
        let mut by_lang = BTreeMap::new();
        by_lang.insert(
            "go".to_string(),
            LangStats {
                files: 3,
                specifiers_internal: 4,
                resolved: 4,
                resolution_rate: 1.0,
                edges: 4,
                ..Default::default()
            },
        );
        let stats = Stats {
            files_discovered: 42,
            files_parsed: 40,
            specifiers_total: 500,
            specifiers_internal: 400,
            resolved: 380,
            unresolved: 15,
            excluded: 5,
            resolution_rate: 0.95,
            external_specifiers: 100,
            external_packages_distinct: 15,
            failures: vec![Failure {
                path: "src/bad.ts".into(),
                reason: "parse error".into(),
            }],
            diagnostics: vec![Diagnostic {
                path: "src/a.ts".into(),
                line: 3,
                specifier: "./ghost".into(),
                kind: DiagnosticKind::Unresolved,
                reason: ExclusionReason::NotFound,
                lang: "typescript".into(),
            }],
            by_lang,
            coverage: Coverage {
                files_seen: 60,
                files_analysed: 40,
                files_unsupported: 5,
                files_not_source: 15,
                extensions: vec![ExtensionCoverage {
                    extension: "scala".into(),
                    label: ".scala".into(),
                    count: 5,
                    status: FileStatus::UnsupportedLanguage,
                    lang: Some("Scala".into()),
                    note: Some("no extractor yet".into()),
                }],
                skipped_directories: vec![SkippedDirectory {
                    name: "node_modules".into(),
                    count: 3,
                    rule: "always-skip".into(),
                }],
            },
        };
        let json = serde_json::to_value(&stats).unwrap();

        // Assert every Stats field is present with correct keys
        assert_eq!(json["files_discovered"], 42);
        assert_eq!(json["files_parsed"], 40);
        assert_eq!(json["specifiers_total"], 500);
        assert_eq!(json["specifiers_internal"], 400);
        assert_eq!(json["resolved"], 380);
        assert_eq!(json["unresolved"], 15);
        assert_eq!(json["excluded"], 5);
        assert_eq!(json["resolution_rate"], 0.95);
        assert_eq!(json["external_specifiers"], 100);
        assert_eq!(json["external_packages_distinct"], 15);

        // Assert Failure inside failures vector serialises with correct keys
        assert_eq!(json["failures"][0]["path"], "src/bad.ts");
        assert_eq!(json["failures"][0]["reason"], "parse error");

        // Assert Diagnostic inside diagnostics vector serialises with
        // correct keys, and its kind and reason render as documented
        // snake_case — the measurement documents are regenerated from
        // exactly these two strings.
        assert_eq!(json["diagnostics"][0]["path"], "src/a.ts");
        assert_eq!(json["diagnostics"][0]["line"], 3);
        assert_eq!(json["diagnostics"][0]["specifier"], "./ghost");
        assert_eq!(json["diagnostics"][0]["kind"], "unresolved");
        assert_eq!(json["diagnostics"][0]["reason"], "not_found");
        assert_eq!(json["diagnostics"][0]["lang"], "typescript");

        // Per-language stats and the coverage report.
        assert_eq!(json["by_lang"]["go"]["resolved"], 4);
        assert_eq!(json["by_lang"]["go"]["edges"], 4);
        assert_eq!(json["coverage"]["files_seen"], 60);
        assert_eq!(json["coverage"]["extensions"][0]["extension"], "scala");
        assert_eq!(
            json["coverage"]["extensions"][0]["status"],
            "unsupported_language"
        );
        assert_eq!(json["coverage"]["extensions"][0]["lang"], "Scala");
        assert_eq!(
            json["coverage"]["skipped_directories"][0]["name"],
            "node_modules"
        );
    }
}
