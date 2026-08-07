//! Asking the graph questions.
//!
//! The graph already knows which files import `packages/prisma/index.ts`.
//! Until this module existed it had no way of being asked. Every question an
//! agent, the command line, or the desktop shell can put to a scan lives
//! here — once — so that three callers cannot drift into three different
//! answers to the same question.
//!
//! Two rules shape every type below, and they are the same two rules the
//! rest of the crate is built on:
//!
//! 1. **A total is never capped.** A listing names at most `limit` files and
//!    reports exactly how many it left out. A truncated answer that does not
//!    say it is truncated reads as a complete one, and a reader has no way
//!    to tell the difference.
//! 2. **A path that names nothing, or names more than one thing, says so.**
//!    Quietly picking one candidate would make a wrong answer look like a
//!    right one — the precise failure this project exists to argue against.

use crate::model::{FileStatus, Graph};
use crate::project::{scan_workspace, Project, ProjectKind, Totals, Workspace};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// How many files a listing names before it starts counting instead. An
/// answer is meant to cost tens of tokens: the *number* of dependents is
/// the finding, the full list of 484 paths rarely is.
pub const DEFAULT_LIMIT: usize = 50;

/// How far [`Atlas::blast_radius`] walks when the caller does not say.
pub const DEFAULT_DEPTH: usize = 3;

/// Hard ceiling on the requested depth. A reverse walk is bounded by the
/// graph itself, so this is not a safety limit — it stops a typo'd
/// `depth: 100000` from reading as a meaningful request.
pub const MAX_DEPTH: usize = 32;

/// How many hubs [`Atlas::summary`] names.
const HUBS: usize = 5;

/// How many near-misses a failed lookup offers.
const SUGGESTIONS: usize = 5;

/// How many candidates an ambiguous path names before it starts counting.
///
/// `locate` itself returns every candidate — that is the honest primitive,
/// and a caller reading the structured answer wants them all. This bounds
/// only the *rendered* list: `index.ts` names 51 files in a real monorepo,
/// and printing all 51 costs more tokens than the answer it is refusing to
/// guess at is worth.
const CANDIDATES_SHOWN: usize = 10;

/// The question forms [`Question::parse`] understands, in the words a caller
/// would actually type. Published as a constant so the error message, the
/// documentation and the tests cannot disagree about what is accepted.
pub const QUESTION_FORMS: &[&str] = &[
    "what depends on <path>",
    "what does <path> depend on",
    "blast radius of <path>",
    "files touching <package>",
    "summary",
];

/// One file inside a workspace: which project holds it, and its id there.
///
/// A node id alone is not an identity across a split scan — two projects can
/// both contain `src/index.ts`, and answering about the wrong one would be
/// wrong in a way nothing downstream could detect.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileRef {
    /// The project's id within the workspace, `.` when the scanned root is
    /// itself the project.
    pub project: String,
    /// The file's id inside that project, i.e. `Node::id`.
    pub id: String,
}

impl std::fmt::Display for FileRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.project == "." {
            write!(f, "{}", self.id)
        } else {
            write!(f, "{}/{}", self.project, self.id)
        }
    }
}

/// What a path the caller typed turned out to name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Located {
    One {
        file: FileRef,
    },
    /// More than one file matches. Every candidate is named: the caller
    /// picks, this module never guesses.
    Ambiguous {
        candidates: Vec<FileRef>,
    },
    /// Nothing matches. Files with a similar name are offered so a typo does
    /// not read as "that file has no dependents".
    NotFound {
        suggestions: Vec<FileRef>,
    },
}

/// A list that always says what it left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing<T> {
    /// The exact number of matches. Never capped, whatever `shown` holds.
    pub total: usize,
    pub shown: Vec<T>,
    /// `total - shown.len()`, stated rather than left to be derived.
    pub omitted: usize,
}

impl<T> Listing<T> {
    fn new(mut items: Vec<T>, limit: usize) -> Self {
        let total = items.len();
        items.truncate(limit);
        Listing {
            total,
            omitted: total - items.len(),
            shown: items,
        }
    }
}

/// One file in an answer's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHit {
    pub project: String,
    pub id: String,
    pub lang: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Files that import the subject.
    Dependents,
    /// Files the subject imports.
    Dependencies,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Dependents => write!(f, "dependents"),
            Direction::Dependencies => write!(f, "dependencies"),
        }
    }
}

/// The direct neighbours of one file, in one direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neighbours {
    pub file: FileRef,
    pub lang: String,
    pub direction: Direction,
    pub files: Listing<FileHit>,
}

/// One file reached by a blast-radius walk, and how far out it sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reached {
    pub project: String,
    pub id: String,
    pub lang: String,
    /// 1 for a direct dependent, 2 for a dependent of one of those, and so
    /// on. The shortest such distance, since the walk is breadth-first.
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepthCount {
    pub depth: usize,
    pub files: usize,
}

/// Everything that transitively imports one file, out to a depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlastRadius {
    pub file: FileRef,
    pub lang: String,
    /// The depth the walk actually used, after clamping to [`MAX_DEPTH`].
    pub depth: usize,
    /// Distinct files reached, not counting the subject itself.
    pub reached: usize,
    pub by_depth: Vec<DepthCount>,
    pub files: Listing<Reached>,
    /// True when files were still being found at the depth limit, so
    /// `reached` is a floor and not the whole radius. Without this flag a
    /// depth-capped walk and an exhausted one produce the same shape of
    /// answer, and the first would silently understate the second.
    pub stopped_at_depth: bool,
}

/// Every file that imports one external package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageUse {
    pub package: String,
    pub files: Listing<FileHit>,
    /// Package names containing the query, offered only when the exact name
    /// matched nothing — otherwise a misspelling reads as "unused".
    pub near_misses: Vec<String>,
    /// Distinct external package names across the whole workspace.
    ///
    /// What makes a zero auditable: without it, "nothing imports this" is
    /// indistinguishable from "this is not the kind of name I index". A
    /// workspace package such as `@documenso/prisma` is precisely that
    /// second case — it resolves to a *file*, so it never appears here even
    /// though 484 files import it.
    pub indexed: usize,
}

/// One language's share of a project, and the rate it publishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LangRate {
    pub lang: String,
    pub files: usize,
    pub edges: usize,
    pub resolution_rate: f64,
}

/// One extension the scan saw and could not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// How to write it in a report: `.sql`, or `Dockerfile`.
    pub label: String,
    pub lang: Option<String>,
    pub files: usize,
}

/// A file many others import. The single most useful thing to know about a
/// codebase you have not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hub {
    pub project: String,
    pub id: String,
    pub lang: String,
    pub dependents: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub kinds: Vec<ProjectKind>,
    pub nodes: usize,
    pub edges: usize,
    pub files_analysed: usize,
    pub files_unsupported: usize,
    pub resolution_rate: f64,
    pub source_coverage: f64,
    /// Most files first, so the table reads as a description of the project.
    pub by_lang: Vec<LangRate>,
    /// Most files first: the coverage gap as a priority list.
    pub gaps: Vec<Gap>,
}

/// What one scan measured, in the shape an agent should read first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub root: String,
    pub split: bool,
    pub projects: Vec<ProjectSummary>,
    pub totals: Totals,
    pub hubs: Vec<Hub>,
}

/// A question that can be put to a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    Dependents { path: String },
    Dependencies { path: String },
    BlastRadius { path: String, depth: usize },
    PackageUsers { package: String },
    Summary,
}

/// The text did not match any form in [`QUESTION_FORMS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAQuestion;

impl std::fmt::Display for NotAQuestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a question this scan can answer")
    }
}

impl std::error::Error for NotAQuestion {}

/// What a [`Question`] produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    Neighbours(Neighbours),
    BlastRadius(BlastRadius),
    PackageUsers(PackageUse),
    Summary(Summary),
    /// The path named nothing, or named more than one file. Not an error:
    /// naming the candidates is the answer.
    Unlocated {
        query: String,
        located: Located,
    },
}

/// A scan, plus the adjacency needed to answer questions about it quickly.
///
/// Built once and queried many times — which is exactly how the MCP server
/// uses it, and how the desktop shell will.
pub struct Atlas {
    workspace: Workspace,
    /// One entry per `workspace.projects`, in the same order.
    indices: Vec<ProjectIndex>,
}

#[derive(Default)]
struct ProjectIndex {
    /// Node id to its position in `Project::graph.nodes`.
    node: HashMap<String, usize>,
    /// Base file name to every node position carrying it.
    by_name: HashMap<String, Vec<usize>>,
    /// Target id to the ids that import it, ascending.
    dependents: HashMap<String, Vec<String>>,
    /// Source id to the ids it imports, ascending.
    dependencies: HashMap<String, Vec<String>>,
}

impl ProjectIndex {
    fn build(graph: &Graph) -> Self {
        let mut index = ProjectIndex::default();
        for (position, node) in graph.nodes.iter().enumerate() {
            index.node.insert(node.id.clone(), position);
            index
                .by_name
                .entry(base_name(&node.id).to_string())
                .or_default()
                .push(position);
        }
        // `Graph::edges` arrives sorted by (source, target) and
        // `Graph::nodes` by id, so every list built here comes out ascending
        // without a second sort — and two runs over an unchanged tree answer
        // in the same order, which is what makes an answer quotable.
        for edge in &graph.edges {
            index
                .dependents
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone());
            index
                .dependencies
                .entry(edge.source.clone())
                .or_default()
                .push(edge.target.clone());
        }
        index
    }
}

impl Atlas {
    pub fn new(workspace: Workspace) -> Self {
        let indices = workspace
            .projects
            .iter()
            .map(|project| ProjectIndex::build(&project.graph))
            .collect();
        Atlas { workspace, indices }
    }

    /// Scan `root` and index the result.
    pub fn scan(root: &Path) -> Self {
        Atlas::new(scan_workspace(root))
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Answer one question, naming at most `limit` files in any list.
    ///
    /// The single entry point every caller goes through. The MCP server, the
    /// `kog query` subcommand and the desktop shell all land here, so a
    /// question cannot be answered one way over stdio and another way in a
    /// terminal.
    pub fn answer(&self, question: &Question, limit: usize) -> Answer {
        match question {
            Question::Summary => Answer::Summary(self.summary()),
            Question::PackageUsers { package } => {
                Answer::PackageUsers(self.files_touching_package(package, limit))
            }
            Question::Dependents { path } => self.on_located(path, |atlas, file| {
                Answer::Neighbours(atlas.dependents(file, limit))
            }),
            Question::Dependencies { path } => self.on_located(path, |atlas, file| {
                Answer::Neighbours(atlas.dependencies(file, limit))
            }),
            Question::BlastRadius { path, depth } => self.on_located(path, |atlas, file| {
                Answer::BlastRadius(atlas.blast_radius(file, *depth, limit))
            }),
        }
    }

    /// Resolve `path` and hand the file to `f`, or return the reason it could
    /// not be resolved — which is itself an answer, never an error.
    fn on_located(&self, path: &str, f: impl Fn(&Self, &FileRef) -> Answer) -> Answer {
        match self.locate(path) {
            Located::One { file } => f(self, &file),
            located => Answer::Unlocated {
                query: path.to_string(),
                located,
            },
        }
    }

    /// Turn a path the caller typed into a file in the graph.
    ///
    /// Four rules are tried in order of decreasing precision, and the first
    /// one that matches anything decides. Mixing them would let a loose
    /// basename match outrank an exact path, which is the wrong way round.
    pub fn locate(&self, path: &str) -> Located {
        let query = normalise(path);
        if query.is_empty() {
            return Located::NotFound {
                suggestions: Vec::new(),
            };
        }

        // An absolute path means nothing until it is made relative to
        // something. Both readings are tried alongside the raw query: below
        // a project's own root (giving a node id), and below the workspace
        // root (giving a project-qualified id).
        let mut forms: Vec<String> = vec![query.clone()];
        if Path::new(&query).is_absolute() {
            for project in &self.workspace.projects {
                if let Some(rest) = strip_dir(&query, &project.path) {
                    forms.push(rest.to_string());
                }
            }
            if let Some(rest) = strip_dir(&query, &self.workspace.root) {
                forms.push(rest.to_string());
            }
        }

        let rules: [fn(&Self, &str, &mut Vec<FileRef>); 4] =
            [Self::exact, Self::qualified, Self::suffix, Self::named];
        for rule in rules {
            let mut hits: Vec<FileRef> = Vec::new();
            for form in &forms {
                rule(self, form, &mut hits);
            }
            hits.sort();
            hits.dedup();
            match hits.len() {
                0 => continue,
                1 => {
                    return Located::One {
                        file: hits.remove(0),
                    }
                }
                _ => return Located::Ambiguous { candidates: hits },
            }
        }

        Located::NotFound {
            suggestions: self.suggest(&query),
        }
    }

    /// The query is a node id, verbatim.
    fn exact(&self, form: &str, hits: &mut Vec<FileRef>) {
        for (project, index) in self.pairs() {
            if index.node.contains_key(form) {
                hits.push(FileRef {
                    project: project.id.clone(),
                    id: form.to_string(),
                });
            }
        }
    }

    /// The query is a project id followed by a node id — how a file is named
    /// when a scan was split across several projects.
    fn qualified(&self, form: &str, hits: &mut Vec<FileRef>) {
        for (project, index) in self.pairs() {
            if project.id == "." {
                continue;
            }
            let Some(rest) = strip_dir(form, &project.id) else {
                continue;
            };
            if index.node.contains_key(rest) {
                hits.push(FileRef {
                    project: project.id.clone(),
                    id: rest.to_string(),
                });
            }
        }
    }

    /// The query is the tail of a node id, cut at a directory boundary:
    /// `prisma/index.ts` for `packages/prisma/index.ts`. The boundary matters
    /// — `isma/index.ts` must not match.
    fn suffix(&self, form: &str, hits: &mut Vec<FileRef>) {
        let tail = format!("/{form}");
        for (project, index) in self.pairs() {
            let Some(positions) = index.by_name.get(base_name(form)) else {
                continue;
            };
            for &position in positions {
                let id = &project.graph.nodes[position].id;
                if id.ends_with(&tail) {
                    hits.push(FileRef {
                        project: project.id.clone(),
                        id: id.clone(),
                    });
                }
            }
        }
    }

    /// The query is a bare file name. Deliberately last, and deliberately
    /// refused for anything containing a slash: `index.ts` in a monorepo
    /// names dozens of files, and answering about one of them would be worse
    /// than saying so.
    fn named(&self, form: &str, hits: &mut Vec<FileRef>) {
        if form.contains('/') {
            return;
        }
        for (project, index) in self.pairs() {
            let Some(positions) = index.by_name.get(form) else {
                continue;
            };
            for &position in positions {
                hits.push(FileRef {
                    project: project.id.clone(),
                    id: project.graph.nodes[position].id.clone(),
                });
            }
        }
    }

    /// Files whose name is close to the query's, so a wrong extension or a
    /// stray directory is recoverable.
    ///
    /// Matched on the *stem*, which is what makes `client.tsx` offer
    /// `client.ts`: comparing whole file names would compare the very
    /// characters that differ. The three-character floor stops a one-letter
    /// query from "suggesting" five arbitrary files.
    fn suggest(&self, query: &str) -> Vec<FileRef> {
        let needle = stem(base_name(query)).to_ascii_lowercase();
        if needle.len() < 3 {
            return Vec::new();
        }
        let mut found = Vec::new();
        for (project, _) in self.pairs() {
            for node in &project.graph.nodes {
                let name = base_name(&node.id).to_ascii_lowercase();
                if stem(&name) == needle || name.contains(&needle) {
                    found.push(FileRef {
                        project: project.id.clone(),
                        id: node.id.clone(),
                    });
                    if found.len() >= SUGGESTIONS {
                        return found;
                    }
                }
            }
        }
        found
    }

    /// Files that import `file`.
    ///
    /// The query the whole MCP server exists for: on documenso,
    /// `packages/prisma/index.ts` answers 484 here, and grepping the
    /// repository for that path answers 0 — every one of those imports is
    /// written `@documenso/prisma`.
    pub fn dependents(&self, file: &FileRef, limit: usize) -> Neighbours {
        self.neighbours(file, Direction::Dependents, limit)
    }

    /// Files `file` imports.
    pub fn dependencies(&self, file: &FileRef, limit: usize) -> Neighbours {
        self.neighbours(file, Direction::Dependencies, limit)
    }

    fn neighbours(&self, file: &FileRef, direction: Direction, limit: usize) -> Neighbours {
        let mut lang = String::new();
        let mut ids: Vec<String> = Vec::new();
        if let Some((project, index)) = self.project_of(file) {
            if let Some(&position) = index.node.get(&file.id) {
                lang = project.graph.nodes[position].lang.clone();
            }
            let adjacency = match direction {
                Direction::Dependents => &index.dependents,
                Direction::Dependencies => &index.dependencies,
            };
            if let Some(found) = adjacency.get(&file.id) {
                ids = found.clone();
            }
        }
        let hits = ids
            .into_iter()
            .map(|id| self.hit(&file.project, id))
            .collect();
        Neighbours {
            file: file.clone(),
            lang,
            direction,
            files: Listing::new(hits, limit),
        }
    }

    /// Everything that would be touched by changing `file`: its dependents,
    /// their dependents, and so on out to `depth`.
    pub fn blast_radius(&self, file: &FileRef, depth: usize, limit: usize) -> BlastRadius {
        let depth = depth.clamp(1, MAX_DEPTH);
        let mut lang = String::new();
        let mut reached: Vec<Reached> = Vec::new();
        let mut by_depth: Vec<DepthCount> = Vec::new();
        let mut stopped_at_depth = false;

        if let Some((project, index)) = self.project_of(file) {
            if let Some(&position) = index.node.get(&file.id) {
                lang = project.graph.nodes[position].lang.clone();
            }

            // Breadth-first, so every file is recorded at its *shortest*
            // distance from the subject. A depth-first walk would report a
            // direct dependent as three hops away if it happened to be
            // reached the long way round first.
            let mut seen: HashSet<&str> = HashSet::new();
            seen.insert(file.id.as_str());
            let mut frontier: Vec<&str> = vec![file.id.as_str()];

            for level in 1..=depth {
                let mut next: Vec<&str> = Vec::new();
                for current in &frontier {
                    let Some(sources) = index.dependents.get(*current) else {
                        continue;
                    };
                    for source in sources {
                        if seen.insert(source.as_str()) {
                            next.push(source.as_str());
                        }
                    }
                }
                if next.is_empty() {
                    break;
                }
                next.sort_unstable();
                by_depth.push(DepthCount {
                    depth: level,
                    files: next.len(),
                });
                for id in &next {
                    reached.push(Reached {
                        project: file.project.clone(),
                        id: (*id).to_string(),
                        lang: self.lang_of(&file.project, id).to_string(),
                        depth: level,
                    });
                }
                frontier = next;
            }

            // Did the walk stop because it ran out of files, or because it
            // ran out of depth? Two different answers, and only one of them
            // means `reached` is the whole radius.
            stopped_at_depth = frontier.iter().any(|current| {
                index.dependents.get(*current).is_some_and(|sources| {
                    sources.iter().any(|source| !seen.contains(source.as_str()))
                })
            });
        }

        BlastRadius {
            file: file.clone(),
            lang,
            depth,
            reached: reached.len(),
            by_depth,
            files: Listing::new(reached, limit),
            stopped_at_depth,
        }
    }

    /// Files that import the external package `package`.
    ///
    /// External means *not resolved to a file in this repository*. A
    /// workspace package or a path alias resolves to a file and so answers
    /// zero here however heavily it is used — which is why the size of the
    /// index searched travels back with the answer.
    pub fn files_touching_package(&self, package: &str, limit: usize) -> PackageUse {
        let needle = package.trim();
        let mut hits: Vec<FileHit> = Vec::new();
        let mut indexed: HashSet<&str> = HashSet::new();
        for (project, _) in self.pairs() {
            for node in &project.graph.nodes {
                for dep in &node.external_deps {
                    indexed.insert(dep.as_str());
                }
                if node.external_deps.iter().any(|dep| dep == needle) {
                    hits.push(FileHit {
                        project: project.id.clone(),
                        id: node.id.clone(),
                        lang: node.lang.clone(),
                    });
                }
            }
        }
        let near_misses = if hits.is_empty() {
            self.packages_like(needle)
        } else {
            Vec::new()
        };
        PackageUse {
            package: needle.to_string(),
            files: Listing::new(hits, limit),
            near_misses,
            indexed: indexed.len(),
        }
    }

    /// Distinct external package names containing `needle`, capped.
    fn packages_like(&self, needle: &str) -> Vec<String> {
        let needle = needle.to_ascii_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut names: Vec<&str> = Vec::new();
        for (project, _) in self.pairs() {
            for node in &project.graph.nodes {
                for dep in &node.external_deps {
                    if dep.to_ascii_lowercase().contains(&needle) {
                        names.push(dep.as_str());
                    }
                }
            }
        }
        names.sort_unstable();
        names.dedup();
        names.truncate(SUGGESTIONS);
        names.into_iter().map(str::to_string).collect()
    }

    /// What the scan measured: the two published numbers, per project and
    /// per language, plus the files everything else points at.
    pub fn summary(&self) -> Summary {
        let projects = self
            .workspace
            .projects
            .iter()
            .map(|project| {
                let stats = &project.graph.stats;
                let mut by_lang: Vec<LangRate> = stats
                    .by_lang
                    .iter()
                    .map(|(lang, lang_stats)| LangRate {
                        lang: lang.clone(),
                        files: lang_stats.files,
                        edges: lang_stats.edges,
                        resolution_rate: lang_stats.resolution_rate,
                    })
                    .collect();
                by_lang.sort_by(|a, b| b.files.cmp(&a.files).then(a.lang.cmp(&b.lang)));

                let gaps = stats
                    .coverage
                    .extensions
                    .iter()
                    .filter(|entry| entry.status == FileStatus::UnsupportedLanguage)
                    .map(|entry| Gap {
                        label: entry.label.clone(),
                        lang: entry.lang.clone(),
                        files: entry.count,
                    })
                    .collect();

                ProjectSummary {
                    id: project.id.clone(),
                    kinds: project.kinds.clone(),
                    nodes: project.graph.nodes.len(),
                    edges: project.graph.edges.len(),
                    files_analysed: stats.coverage.files_analysed,
                    files_unsupported: stats.coverage.files_unsupported,
                    resolution_rate: stats.resolution_rate,
                    source_coverage: stats.coverage.source_coverage(),
                    by_lang,
                    gaps,
                }
            })
            .collect();

        Summary {
            root: self.workspace.root.clone(),
            split: self.workspace.split,
            projects,
            totals: self.workspace.totals.clone(),
            hubs: self.hubs(),
        }
    }

    /// The most depended-upon files in the workspace.
    fn hubs(&self) -> Vec<Hub> {
        let mut hubs: Vec<Hub> = Vec::new();
        for (project, index) in self.pairs() {
            for (id, sources) in &index.dependents {
                hubs.push(Hub {
                    project: project.id.clone(),
                    id: id.clone(),
                    lang: self.lang_of(&project.id, id).to_string(),
                    dependents: sources.len(),
                });
            }
        }
        // Most depended-upon first; ties broken by name so a `HashMap`'s
        // iteration order never reaches the caller.
        hubs.sort_by(|a, b| {
            b.dependents
                .cmp(&a.dependents)
                .then(a.project.cmp(&b.project))
                .then(a.id.cmp(&b.id))
        });
        hubs.truncate(HUBS);
        hubs
    }

    fn pairs(&self) -> impl Iterator<Item = (&Project, &ProjectIndex)> {
        self.workspace.projects.iter().zip(&self.indices)
    }

    fn project_of(&self, file: &FileRef) -> Option<(&Project, &ProjectIndex)> {
        self.pairs().find(|(project, _)| project.id == file.project)
    }

    fn lang_of(&self, project_id: &str, id: &str) -> &str {
        self.pairs()
            .find(|(project, _)| project.id == project_id)
            .and_then(|(project, index)| {
                index
                    .node
                    .get(id)
                    .map(|&position| project.graph.nodes[position].lang.as_str())
            })
            .unwrap_or("unknown")
    }

    fn hit(&self, project_id: &str, id: String) -> FileHit {
        FileHit {
            lang: self.lang_of(project_id, &id).to_string(),
            project: project_id.to_string(),
            id,
        }
    }
}

impl Question {
    /// Read a question written the way someone would type it.
    ///
    /// Deliberately a small, closed set of forms rather than a guess at
    /// intent: a phrase this does not recognise is refused and the accepted
    /// forms are printed, which is a better outcome than answering a
    /// different question than the one asked.
    pub fn parse(text: &str) -> Result<Question, NotAQuestion> {
        let cleaned = text.trim().trim_end_matches(['?', '.', '!']).trim();
        // `to_ascii_lowercase` preserves byte length, so an offset found in
        // the lowered copy indexes the original correctly.
        let lower = cleaned.to_ascii_lowercase();

        if matches!(
            lower.as_str(),
            "summary" | "scan summary" | "scan_summary" | "overview" | "stats" | "what is this"
        ) {
            return Ok(Question::Summary);
        }

        // Checked before the dependents forms: "what does X depend on" and
        // "what depends on X" both open with "what", and only the tail tells
        // the two directions apart.
        for (prefix, suffix) in [
            ("what does ", " depend on"),
            ("what does ", " import"),
            ("what ", " depends on"),
        ] {
            if let Some(path) = between(&lower, cleaned, prefix, suffix) {
                return Ok(Question::Dependencies { path });
            }
        }
        for prefix in ["dependencies of ", "what does "] {
            if let Some(path) = after(&lower, cleaned, prefix) {
                return Ok(Question::Dependencies { path });
            }
        }

        for prefix in [
            "what depends on ",
            "who depends on ",
            "what imports ",
            "who imports ",
            "dependents of ",
        ] {
            if let Some(path) = after(&lower, cleaned, prefix) {
                return Ok(Question::Dependents { path });
            }
        }

        for prefix in [
            "blast radius of ",
            "blast radius ",
            "blast_radius ",
            "what breaks if i change ",
            "what breaks if we change ",
        ] {
            if let Some(path) = after(&lower, cleaned, prefix) {
                return Ok(Question::BlastRadius {
                    path,
                    depth: DEFAULT_DEPTH,
                });
            }
        }

        // Longest first: "files touching package X" must not be read as a
        // package literally named "package X".
        for prefix in [
            "which files touch package ",
            "files touching package ",
            "which files touch ",
            "files touching ",
            "who uses ",
            "what uses ",
        ] {
            if let Some(package) = after(&lower, cleaned, prefix) {
                return Ok(Question::PackageUsers { package });
            }
        }

        Err(NotAQuestion)
    }
}

/// The one rendering of an [`Answer`], shared by every caller.
///
/// Compact on purpose. The finding is usually the number — "484 dependents"
/// — and a caller that wants all 484 paths asks for them with a limit.
pub fn render(answer: &Answer) -> String {
    match answer {
        Answer::Neighbours(neighbours) => render_neighbours(neighbours),
        Answer::BlastRadius(radius) => render_blast_radius(radius),
        Answer::PackageUsers(usage) => render_package(usage),
        Answer::Summary(summary) => render_summary(summary),
        Answer::Unlocated { query, located } => render_unlocated(query, located),
    }
}

fn render_neighbours(neighbours: &Neighbours) -> String {
    let mut out = format!(
        "{} ({})\n{} {}\n",
        neighbours.file,
        label(&neighbours.lang),
        neighbours.files.total,
        neighbours.direction
    );
    for hit in &neighbours.files.shown {
        out.push_str(&format!(
            "  {}\n",
            path_of(&neighbours.file.project, &hit.project, &hit.id)
        ));
    }
    push_omitted(&mut out, neighbours.files.omitted);
    out
}

fn render_blast_radius(radius: &BlastRadius) -> String {
    let mut out = format!(
        "{} ({})\n{} files reached within depth {}\n",
        radius.file,
        label(&radius.lang),
        radius.reached,
        radius.depth
    );
    for step in &radius.by_depth {
        out.push_str(&format!("  depth {}  {}\n", step.depth, step.files));
    }
    if radius.stopped_at_depth {
        out.push_str(&format!(
            "still expanding at depth {} — {} is a floor, not the whole radius\n",
            radius.depth, radius.reached
        ));
    }
    for hit in &radius.files.shown {
        out.push_str(&format!(
            "  {}  {}\n",
            hit.depth,
            path_of(&radius.file.project, &hit.project, &hit.id)
        ));
    }
    push_omitted(&mut out, radius.files.omitted);
    out
}

fn render_package(usage: &PackageUse) -> String {
    if usage.files.total == 0 {
        // Never "nothing imports this". The honest statement is narrower and
        // says which index was searched, because the commonest reason for a
        // zero here is that the name is a workspace package — which resolves
        // to a file and is therefore not in this index at all.
        let mut out = format!(
            "no file imports \"{}\" as an external package ({} distinct packages indexed)\n",
            usage.package, usage.indexed
        );
        if !usage.near_misses.is_empty() {
            out.push_str(&format!("did you mean: {}\n", usage.near_misses.join(", ")));
        }
        out.push_str(
            "a workspace package or path alias resolves to a file rather than a package — \
ask what depends on that file instead\n",
        );
        return out;
    }
    let mut out = format!(
        "{} — imported by {} files\n",
        usage.package, usage.files.total
    );
    for hit in &usage.files.shown {
        out.push_str(&format!("  {}\n", qualified_path(&hit.project, &hit.id)));
    }
    push_omitted(&mut out, usage.files.omitted);
    out
}

fn render_summary(summary: &Summary) -> String {
    let totals = &summary.totals;
    let mut out = format!(
        "{}\n{} project(s), {} nodes, {} edges\nresolution rate {:.4}   source coverage {:.4}\nfiles analysed {}   not read {}\n",
        summary.root,
        totals.projects,
        totals.nodes,
        totals.edges,
        totals.resolution_rate,
        totals.source_coverage,
        totals.files_analysed,
        totals.files_unsupported,
    );

    for project in &summary.projects {
        if summary.split {
            let kinds: Vec<String> = project.kinds.iter().map(ProjectKind::to_string).collect();
            out.push_str(&format!(
                "\n{} ({})\n  {} nodes, {} edges, rate {:.4}, coverage {:.4}\n",
                project.id,
                kinds.join(", "),
                project.nodes,
                project.edges,
                project.resolution_rate,
                project.source_coverage,
            ));
        }
        // A language ships when it passes its own gate, so every language
        // publishes its own rate here too: an aggregate can be healthy while
        // one resolver is broken.
        for lang in &project.by_lang {
            out.push_str(&format!(
                "  {:<14} {:.4}  ({} files, {} edges)\n",
                lang.lang, lang.resolution_rate, lang.files, lang.edges
            ));
        }
        if !project.gaps.is_empty() {
            out.push_str(&format!(
                "  not read ({} files)\n",
                project.files_unsupported
            ));
            for gap in &project.gaps {
                out.push_str(&format!(
                    "    {:<12} {:>5}  {}\n",
                    gap.label,
                    gap.files,
                    gap.lang.as_deref().unwrap_or("unrecognised")
                ));
            }
        }
    }

    if !summary.hubs.is_empty() {
        out.push_str("\nmost depended upon\n");
        for hub in &summary.hubs {
            out.push_str(&format!(
                "  {:>5}  {}\n",
                hub.dependents,
                qualified_path(&hub.project, &hub.id)
            ));
        }
    }
    out
}

fn render_unlocated(query: &str, located: &Located) -> String {
    match located {
        Located::One { file } => format!("{file}\n"),
        Located::Ambiguous { candidates } => {
            let mut out = format!(
                "\"{}\" names {} files — say which:\n",
                query,
                candidates.len()
            );
            for candidate in candidates.iter().take(CANDIDATES_SHOWN) {
                out.push_str(&format!("  {candidate}\n"));
            }
            if let Some(rest) = candidates.len().checked_sub(CANDIDATES_SHOWN) {
                if rest > 0 {
                    out.push_str(&format!(
                        "  … {rest} more — give enough of the path to pick one\n"
                    ));
                }
            }
            out
        }
        Located::NotFound { suggestions } => {
            let mut out = format!("nothing in the graph is named \"{query}\"\n");
            if !suggestions.is_empty() {
                out.push_str("close names:\n");
                for suggestion in suggestions {
                    out.push_str(&format!("  {suggestion}\n"));
                }
            }
            out
        }
    }
}

fn push_omitted(out: &mut String, omitted: usize) {
    if omitted > 0 {
        out.push_str(&format!("  … {omitted} more not shown (raise `limit`)\n"));
    }
}

fn label(lang: &str) -> &str {
    if lang.is_empty() {
        "unknown"
    } else {
        lang
    }
}

/// A neighbour's path, qualified only when it sits in another project than
/// the file being asked about — repeating the project on every line of 50
/// would be tokens that carry no information.
fn path_of<'a>(subject: &str, project: &str, id: &'a str) -> std::borrow::Cow<'a, str> {
    if project == subject {
        std::borrow::Cow::Borrowed(id)
    } else {
        std::borrow::Cow::Owned(qualified_path(project, id))
    }
}

fn qualified_path(project: &str, id: &str) -> String {
    if project == "." {
        id.to_string()
    } else {
        format!("{project}/{id}")
    }
}

fn base_name(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

/// A file name without its final extension. A name that is all extension
/// (`.gitignore`) keeps it — the dot is part of the identity there.
fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((head, _)) if !head.is_empty() => head,
        _ => name,
    }
}

/// Trim a caller-supplied path down to the shape node ids use.
fn normalise(path: &str) -> String {
    let slashed = path.trim().replace('\\', "/");
    let mut view: &str = slashed.trim_end_matches('/');
    while let Some(rest) = view.strip_prefix("./") {
        view = rest;
    }
    view.to_string()
}

/// `path` with `dir` and the separator after it removed — but only when
/// `path` is genuinely below `dir`, so `/a/bc` is not read as being inside
/// `/a/b`.
fn strip_dir<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(dir.trim_end_matches('/'))?;
    let rest = rest.strip_prefix('/')?;
    (!rest.is_empty()).then_some(rest)
}

/// The text between `prefix` and `suffix`, matched case-insensitively
/// against `lower` and cut out of `original`.
fn between(lower: &str, original: &str, prefix: &str, suffix: &str) -> Option<String> {
    if !lower.starts_with(prefix) || !lower.ends_with(suffix) {
        return None;
    }
    let start = prefix.len();
    let end = lower.len().checked_sub(suffix.len())?;
    if end <= start {
        return None;
    }
    let middle = original[start..end].trim();
    (!middle.is_empty()).then(|| middle.to_string())
}

/// The text after `prefix`, matched case-insensitively.
fn after(lower: &str, original: &str, prefix: &str) -> Option<String> {
    if !lower.starts_with(prefix) {
        return None;
    }
    let rest = original[prefix.len()..].trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn atlas(dir: &TempDir) -> Atlas {
        Atlas::scan(dir.path())
    }

    /// The file the caller meant, or a panic naming what was found instead —
    /// every query test below is about the *answer*, not about lookup, and a
    /// silent `Ambiguous` would otherwise turn into a confusing zero.
    fn one(atlas: &Atlas, path: &str) -> FileRef {
        match atlas.locate(path) {
            Located::One { file } => file,
            other => panic!("expected {path:?} to name exactly one file, got {other:?}"),
        }
    }

    // --- Dependents and dependencies ---

    #[test]
    fn a_files_dependents_are_the_files_that_import_it() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/lib.ts", "export const x = 1;");
        write(&dir, "src/a.ts", r#"import { x } from "./lib";"#);
        write(&dir, "src/b.ts", r#"import { x } from "./lib";"#);
        write(&dir, "src/unrelated.ts", "export const y = 2;");
        let atlas = atlas(&dir);

        let answer = atlas.dependents(&one(&atlas, "src/lib.ts"), DEFAULT_LIMIT);

        assert_eq!(answer.files.total, 2);
        assert_eq!(answer.lang, "typescript");
        let ids: Vec<&str> = answer.files.shown.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/b.ts"], "answers are ordered");
    }

    /// The argument the MCP server exists to make, in miniature: the import
    /// is written as an alias, so the importing file does not contain the
    /// target's path anywhere. `grep` for `packages/db.ts` finds nothing;
    /// the graph finds the dependent.
    #[test]
    fn an_alias_resolved_import_is_a_dependent_grep_could_never_find() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"root"}"#);
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@acme/*": ["./packages/*"] } } }"#,
        );
        write(&dir, "packages/db.ts", "export const db = 1;");
        write(&dir, "apps/web.ts", r#"import { db } from "@acme/db";"#);
        let atlas = atlas(&dir);

        let source = fs::read_to_string(dir.path().join("apps/web.ts")).unwrap();
        assert!(
            !source.contains("packages/db.ts"),
            "the importing file must not contain the target's path, or this \
             test is not testing what it claims to"
        );

        let answer = atlas.dependents(&one(&atlas, "packages/db.ts"), DEFAULT_LIMIT);

        assert_eq!(answer.files.total, 1);
        assert_eq!(answer.files.shown[0].id, "apps/web.ts");
    }

    #[test]
    fn dependencies_are_the_other_direction() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "import \"./b\";\nimport \"./c\";");
        write(&dir, "src/b.ts", "");
        write(&dir, "src/c.ts", "");
        let atlas = atlas(&dir);

        let answer = atlas.dependencies(&one(&atlas, "src/a.ts"), DEFAULT_LIMIT);

        assert_eq!(answer.direction, Direction::Dependencies);
        let ids: Vec<&str> = answer.files.shown.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["src/b.ts", "src/c.ts"]);
    }

    /// Rule 1 of this module: the total is the finding, and it survives the
    /// cap. A listing that reported 3 because it was asked for 3 would make
    /// "how many things import this?" unanswerable.
    #[test]
    fn a_listing_reports_the_exact_total_even_when_it_names_only_a_few() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/lib.ts", "");
        for i in 0..12 {
            write(&dir, &format!("src/f{i}.ts"), r#"import "./lib";"#);
        }
        let atlas = atlas(&dir);

        let answer = atlas.dependents(&one(&atlas, "src/lib.ts"), 3);

        assert_eq!(answer.files.total, 12, "the total is never capped");
        assert_eq!(answer.files.shown.len(), 3);
        assert_eq!(answer.files.omitted, 9);
        assert!(
            render(&Answer::Neighbours(answer)).contains("12 dependents"),
            "the rendered answer must lead with the true total"
        );
    }

    // --- Blast radius ---

    #[test]
    fn blast_radius_reaches_transitively_and_records_the_shortest_distance() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/core.ts", "");
        write(&dir, "src/mid.ts", r#"import "./core";"#);
        // Reaches core both directly and through mid: the direct hop is the
        // one that must be reported.
        write(&dir, "src/top.ts", "import \"./mid\";\nimport \"./core\";");
        let atlas = atlas(&dir);

        let answer = atlas.blast_radius(&one(&atlas, "src/core.ts"), 5, DEFAULT_LIMIT);

        assert_eq!(answer.reached, 2);
        assert_eq!(
            answer.by_depth,
            vec![DepthCount { depth: 1, files: 2 }],
            "both importers are one hop away, so there is no second level"
        );
        let top = answer
            .files
            .shown
            .iter()
            .find(|f| f.id == "src/top.ts")
            .unwrap();
        assert_eq!(top.depth, 1, "the shortest distance, not the longest");
        assert!(!answer.stopped_at_depth);
    }

    #[test]
    fn blast_radius_counts_a_second_level_separately() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/core.ts", "");
        write(&dir, "src/mid.ts", r#"import "./core";"#);
        write(&dir, "src/top.ts", r#"import "./mid";"#);
        let atlas = atlas(&dir);

        let answer = atlas.blast_radius(&one(&atlas, "src/core.ts"), 5, DEFAULT_LIMIT);

        assert_eq!(answer.reached, 2);
        assert_eq!(
            answer.by_depth,
            vec![
                DepthCount { depth: 1, files: 1 },
                DepthCount { depth: 2, files: 1 }
            ]
        );
    }

    /// Rule 1 again, in the one place it is easiest to get wrong: a walk cut
    /// short by its depth limit and a walk that ran out of graph produce the
    /// same shape of answer, and only one of them is complete.
    #[test]
    fn blast_radius_says_when_the_depth_stopped_it_rather_than_the_graph() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/core.ts", "");
        write(&dir, "src/one.ts", r#"import "./core";"#);
        write(&dir, "src/two.ts", r#"import "./one";"#);
        write(&dir, "src/three.ts", r#"import "./two";"#);
        let atlas = atlas(&dir);
        let core = one(&atlas, "src/core.ts");

        let cut = atlas.blast_radius(&core, 2, DEFAULT_LIMIT);
        assert_eq!(cut.reached, 2);
        assert!(
            cut.stopped_at_depth,
            "a walk that still had files to visit must say so"
        );
        assert!(render(&Answer::BlastRadius(cut)).contains("still expanding"));

        let whole = atlas.blast_radius(&core, 5, DEFAULT_LIMIT);
        assert_eq!(whole.reached, 3);
        assert!(
            !whole.stopped_at_depth,
            "a walk that exhausted the graph must not claim to be truncated"
        );
    }

    #[test]
    fn blast_radius_terminates_on_a_cycle() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", r#"import "./b";"#);
        write(&dir, "src/b.ts", r#"import "./a";"#);
        let atlas = atlas(&dir);

        let answer = atlas.blast_radius(&one(&atlas, "src/a.ts"), MAX_DEPTH, DEFAULT_LIMIT);

        assert_eq!(answer.reached, 1, "the subject is never its own dependent");
        assert_eq!(answer.files.shown[0].id, "src/b.ts");
    }

    #[test]
    fn a_requested_depth_is_clamped_and_the_depth_used_is_reported() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "");
        let atlas = atlas(&dir);

        let answer = atlas.blast_radius(&one(&atlas, "src/a.ts"), 100_000, DEFAULT_LIMIT);
        assert_eq!(answer.depth, MAX_DEPTH);

        let answer = atlas.blast_radius(&one(&atlas, "src/a.ts"), 0, DEFAULT_LIMIT);
        assert_eq!(answer.depth, 1, "a zero-depth walk would answer nothing");
    }

    // --- Packages ---

    #[test]
    fn files_touching_a_package_names_every_importer() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", r#"import React from "react";"#);
        write(&dir, "src/b.ts", r#"import React from "react";"#);
        write(&dir, "src/c.ts", r#"import { z } from "zod";"#);
        let atlas = atlas(&dir);

        let answer = atlas.files_touching_package("react", DEFAULT_LIMIT);

        assert_eq!(answer.files.total, 2);
        let ids: Vec<&str> = answer.files.shown.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/b.ts"]);
        assert!(answer.near_misses.is_empty());
    }

    /// A misspelled package must not answer "nothing imports it" and leave
    /// it there: that reads as a finding about the codebase when it is a
    /// finding about the query.
    #[test]
    fn a_package_that_matches_nothing_offers_the_names_that_are_close() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(
            &dir,
            "src/a.ts",
            r#"import { p } from "@documenso/prisma";"#,
        );
        let atlas = atlas(&dir);

        let answer = atlas.files_touching_package("prisma", DEFAULT_LIMIT);

        assert_eq!(answer.files.total, 0);
        assert_eq!(answer.near_misses, vec!["@documenso/prisma".to_string()]);
        assert!(render(&Answer::PackageUsers(answer)).contains("did you mean"));
    }

    /// Found by running this against documenso: `@documenso/prisma` is a
    /// workspace package, so its 484 imports resolve to a *file* and it
    /// never enters the external-package index. Answering "nothing imports
    /// it" would be a statement about the repository when it is a statement
    /// about which index was searched — and it would be wrong by 484.
    #[test]
    fn a_workspace_package_answers_zero_and_the_answer_says_why() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"root"}"#);
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@acme/*": ["./packages/*"] } } }"#,
        );
        write(&dir, "packages/db.ts", "export const db = 1;");
        write(
            &dir,
            "apps/web.ts",
            "import { db } from \"@acme/db\";\nimport \"react\";",
        );
        let atlas = atlas(&dir);

        // The import really did resolve — to a file, which is the point.
        assert_eq!(
            atlas
                .dependents(&one(&atlas, "packages/db.ts"), DEFAULT_LIMIT)
                .files
                .total,
            1
        );

        let answer = atlas.files_touching_package("@acme/db", DEFAULT_LIMIT);
        assert_eq!(answer.files.total, 0);
        assert_eq!(
            answer.indexed, 1,
            "only `react` is an external package here, and the answer must \
             say how small the index it searched was"
        );

        let text = render(&Answer::PackageUsers(answer));
        assert!(
            text.contains("as an external package"),
            "the zero must be scoped to the index it came from, got {text:?}"
        );
        assert!(
            text.contains("ask what depends on that file instead"),
            "and it must point at the query that does answer, got {text:?}"
        );
    }

    // --- Locating a file ---

    /// Found by asking a real monorepo about `index.ts`: it named 51 files
    /// and printed all 51. Refusing to guess is right; spending five hundred
    /// tokens on the refusal is not, and the same rule that caps every other
    /// list has to apply here too.
    #[test]
    fn a_long_list_of_candidates_is_capped_and_says_how_many_it_kept_back() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        for i in 0..14 {
            write(&dir, &format!("src/f{i}/index.ts"), "");
        }
        let atlas = atlas(&dir);

        let Located::Ambiguous { candidates } = atlas.locate("index.ts") else {
            panic!("14 files with the same name must be ambiguous");
        };
        assert_eq!(
            candidates.len(),
            14,
            "the structured answer keeps every candidate"
        );

        let text = render(&Answer::Unlocated {
            query: "index.ts".to_string(),
            located: Located::Ambiguous { candidates },
        });
        assert!(text.contains("names 14 files"), "got {text:?}");
        assert_eq!(
            text.lines().filter(|l| l.contains("index.ts")).count(),
            CANDIDATES_SHOWN + 1,
            "the ten shown, plus the line naming the query itself"
        );
        assert!(text.contains("… 4 more"), "got {text:?}");
    }

    #[test]
    fn a_bare_file_name_that_names_two_files_is_ambiguous_not_a_guess() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/one/index.ts", "");
        write(&dir, "src/two/index.ts", "");
        let atlas = atlas(&dir);

        match atlas.locate("index.ts") {
            Located::Ambiguous { candidates } => {
                let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
                assert_eq!(ids, vec!["src/one/index.ts", "src/two/index.ts"]);
            }
            other => panic!("expected an ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn a_more_precise_form_wins_over_a_looser_one() {
        // `index.ts` exists at the root *and* as a basename inside two
        // directories. The exact node id must win outright: were the rules
        // pooled instead of tried in order, this would be a three-way
        // ambiguity and the exact path would be unanswerable.
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "index.ts", "");
        write(&dir, "src/one/index.ts", "");
        write(&dir, "src/two/index.ts", "");
        let atlas = atlas(&dir);

        assert_eq!(one(&atlas, "index.ts").id, "index.ts");
    }

    #[test]
    fn a_path_suffix_only_matches_at_a_directory_boundary() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "packages/prisma/index.ts", "");
        let atlas = atlas(&dir);

        assert_eq!(
            one(&atlas, "prisma/index.ts").id,
            "packages/prisma/index.ts"
        );
        assert!(
            matches!(atlas.locate("isma/index.ts"), Located::NotFound { .. }),
            "a suffix cut mid-segment must not match"
        );
    }

    #[test]
    fn an_absolute_path_is_resolved_against_the_project_root() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "");
        let atlas = atlas(&dir);

        // macOS hands every `TempDir` a `/var` path that canonicalises to
        // `/private/var`; the scan stores the canonical one, so a test that
        // compared against `dir.path()` directly would fail for a reason
        // production never sees.
        let absolute = dir.path().canonicalize().unwrap().join("src/a.ts");

        assert_eq!(one(&atlas, &absolute.to_string_lossy()).id, "src/a.ts");
    }

    #[test]
    fn a_project_qualified_path_finds_the_file_in_a_split_scan() {
        let dir = TempDir::new().unwrap();
        write(&dir, "web/package.json", r#"{"name":"web"}"#);
        write(&dir, "web/src/index.ts", "");
        write(&dir, "api/package.json", r#"{"name":"api"}"#);
        write(&dir, "api/src/index.ts", "");
        let atlas = atlas(&dir);
        assert!(atlas.workspace().split, "test setup must produce a split");

        let file = one(&atlas, "web/src/index.ts");
        assert_eq!(file.project, "web");
        assert_eq!(file.id, "src/index.ts");
        assert_eq!(file.to_string(), "web/src/index.ts");

        assert!(
            matches!(atlas.locate("src/index.ts"), Located::Ambiguous { .. }),
            "the same node id in two projects is ambiguous without the project"
        );
    }

    #[test]
    fn a_path_that_names_nothing_suggests_close_names() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/client.ts", "");
        let atlas = atlas(&dir);

        match atlas.locate("clint.ts") {
            Located::NotFound { suggestions } => assert!(
                suggestions.is_empty(),
                "no shared substring, so nothing to offer"
            ),
            other => panic!("expected NotFound, got {other:?}"),
        }
        match atlas.locate("client.tsx") {
            Located::NotFound { suggestions } => {
                assert_eq!(suggestions[0].id, "src/client.ts")
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_path_names_nothing_rather_than_everything() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "");
        let atlas = atlas(&dir);

        for query in ["", "   ", "/", "./"] {
            assert!(
                matches!(atlas.locate(query), Located::NotFound { .. }),
                "{query:?} must not resolve to a file"
            );
        }
    }

    // --- Summary ---

    #[test]
    fn the_summary_publishes_the_same_two_numbers_as_the_scan() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "import \"./b\";\nimport \"./ghost\";");
        write(&dir, "src/b.ts", "");
        write(&dir, "src/main.hs", "module Main where");
        let atlas = atlas(&dir);

        let summary = atlas.summary();
        let stats = &atlas.workspace().projects[0].graph.stats;

        assert_eq!(summary.totals.resolution_rate, stats.resolution_rate);
        assert_eq!(
            summary.projects[0].source_coverage,
            stats.coverage.source_coverage()
        );
        assert_eq!(summary.projects[0].resolution_rate, 0.5);

        // The coverage gap is named with its language, not merely counted.
        let gap = &summary.projects[0].gaps[0];
        assert_eq!(gap.label, ".hs");
        assert_eq!(gap.files, 1);
        assert_eq!(gap.lang.as_deref(), Some("Haskell"));
    }

    #[test]
    fn the_summary_names_the_most_depended_upon_files_first() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/hub.ts", "");
        write(&dir, "src/side.ts", "");
        for i in 0..4 {
            write(&dir, &format!("src/f{i}.ts"), r#"import "./hub";"#);
        }
        write(&dir, "src/lonely.ts", r#"import "./side";"#);
        let atlas = atlas(&dir);

        let hubs = atlas.summary().hubs;

        assert_eq!(hubs[0].id, "src/hub.ts");
        assert_eq!(hubs[0].dependents, 4);
        assert_eq!(hubs[1].id, "src/side.ts");
        assert_eq!(hubs[1].dependents, 1);
    }

    // --- Reading a question ---

    #[test]
    fn every_published_question_form_parses() {
        assert_eq!(
            Question::parse("what depends on src/a.ts").unwrap(),
            Question::Dependents {
                path: "src/a.ts".into()
            }
        );
        assert_eq!(
            Question::parse("what does src/a.ts depend on?").unwrap(),
            Question::Dependencies {
                path: "src/a.ts".into()
            }
        );
        assert_eq!(
            Question::parse("blast radius of src/a.ts").unwrap(),
            Question::BlastRadius {
                path: "src/a.ts".into(),
                depth: DEFAULT_DEPTH
            }
        );
        assert_eq!(
            Question::parse("files touching react").unwrap(),
            Question::PackageUsers {
                package: "react".into()
            }
        );
        assert_eq!(Question::parse("summary").unwrap(), Question::Summary);
        assert_eq!(
            QUESTION_FORMS.len(),
            5,
            "every published form must have a case above"
        );
    }

    /// "what depends on X" and "what does X depend on" are the same words in
    /// a different order and mean opposite things. Getting this backwards
    /// would answer confidently and wrongly, which is the worst outcome
    /// available.
    #[test]
    fn the_two_directions_are_not_confused_with_each_other() {
        assert_eq!(
            Question::parse("what depends on a.ts").unwrap(),
            Question::Dependents {
                path: "a.ts".into()
            }
        );
        assert_eq!(
            Question::parse("what a.ts depends on").unwrap(),
            Question::Dependencies {
                path: "a.ts".into()
            }
        );
    }

    #[test]
    fn a_question_is_read_whatever_its_case_or_punctuation() {
        assert_eq!(
            Question::parse("  What Depends On src/A.ts?  ").unwrap(),
            Question::Dependents {
                path: "src/A.ts".into()
            },
            "the phrase is case-insensitive; the path keeps its own case"
        );
    }

    #[test]
    fn a_package_question_is_not_read_as_a_package_named_package() {
        assert_eq!(
            Question::parse("files touching package react").unwrap(),
            Question::PackageUsers {
                package: "react".into()
            }
        );
    }

    #[test]
    fn an_unrecognised_phrase_is_refused_rather_than_guessed() {
        for text in ["", "hello", "delete everything", "what depends on"] {
            assert_eq!(
                Question::parse(text),
                Err(NotAQuestion),
                "{text:?} must not be answered"
            );
        }
    }

    // --- The shared answer path ---

    #[test]
    fn an_unlocatable_path_is_an_answer_and_not_an_error() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "");
        let atlas = atlas(&dir);

        let answer = atlas.answer(
            &Question::Dependents {
                path: "nowhere.ts".into(),
            },
            DEFAULT_LIMIT,
        );

        match &answer {
            Answer::Unlocated { query, located } => {
                assert_eq!(query, "nowhere.ts");
                assert!(matches!(located, Located::NotFound { .. }));
            }
            other => panic!("expected Unlocated, got {other:?}"),
        }
        assert!(render(&answer).contains("nothing in the graph is named"));
    }

    #[test]
    fn answer_routes_each_question_to_its_own_shape() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", r#"import "./b";"#);
        write(&dir, "src/b.ts", "");
        let atlas = atlas(&dir);

        let cases = [
            (
                Question::Dependents {
                    path: "src/b.ts".into(),
                },
                "neighbours",
            ),
            (
                Question::Dependencies {
                    path: "src/a.ts".into(),
                },
                "neighbours",
            ),
            (
                Question::BlastRadius {
                    path: "src/b.ts".into(),
                    depth: DEFAULT_DEPTH,
                },
                "blast_radius",
            ),
            (
                Question::PackageUsers {
                    package: "react".into(),
                },
                "package_users",
            ),
            (Question::Summary, "summary"),
        ];

        for (question, tag) in cases {
            let answer = atlas.answer(&question, DEFAULT_LIMIT);
            let json = serde_json::to_value(&answer).unwrap();
            assert_eq!(
                json["answer"], tag,
                "{question:?} must serialise under its own tag"
            );
            assert!(
                !render(&answer).is_empty(),
                "{question:?} must render to something"
            );
        }
    }

    /// A neighbour in another project is qualified; one in the same project
    /// is not. Repeating the project on every line of fifty would be tokens
    /// that carry no information, and this tool is measured in tokens.
    #[test]
    fn a_rendered_path_names_its_project_only_when_it_differs() {
        assert_eq!(path_of("web", "web", "src/a.ts"), "src/a.ts");
        assert_eq!(path_of("web", "api", "src/a.ts"), "api/src/a.ts");
        assert_eq!(path_of(".", ".", "src/a.ts"), "src/a.ts");
    }
}
