//! One graph per project, not one graph per directory.
//!
//! Pointed at a project, KOG maps it. Pointed at a directory that merely
//! *contains* projects — `~/work`, a services directory, a folder of
//! clones — a single graph would be a lie of composition: it would draw one
//! shape out of codebases that share no imports, no resolver configuration
//! and no history. This module decides where the projects are, and the scan
//! then produces one graph for each.
//!
//! The rule, stated once:
//!
//! 1. A directory holding a language manifest (`package.json`, `go.mod`,
//!    `Cargo.toml`, …) is a project. A monorepo root holds one, so a
//!    monorepo is one project — which is required, not incidental: its
//!    workspace packages only resolve from the root.
//! 2. Otherwise, a directory holding a `.git` is a repository, and a
//!    repository with no manifest of its own is examined one level deeper.
//! 3. A directory with neither is a container: its marked children are the
//!    projects, and there must be at least two of them for splitting to be
//!    the better answer.

use crate::discover::ALWAYS_SKIP;
use crate::graph::build_graph;
use crate::model::Graph;
use crate::registry::Registry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// How deep below the scan root a project may be found. Four levels covers
/// `services/api`, `packages/@scope/name` and `clients/web/app` without
/// turning the search into a full tree walk of a machine's home directory.
const MAX_PROJECT_DEPTH: usize = 4;

/// A file whose presence declares "this directory is a project", and the
/// ecosystem it belongs to.
const MANIFESTS: &[(&str, ProjectKind)] = &[
    ("package.json", ProjectKind::Node),
    ("deno.json", ProjectKind::Node),
    ("Cargo.toml", ProjectKind::Rust),
    ("go.mod", ProjectKind::Go),
    ("pyproject.toml", ProjectKind::Python),
    ("setup.py", ProjectKind::Python),
    ("setup.cfg", ProjectKind::Python),
    ("requirements.txt", ProjectKind::Python),
    ("Pipfile", ProjectKind::Python),
    ("pom.xml", ProjectKind::Jvm),
    ("build.gradle", ProjectKind::Jvm),
    ("build.gradle.kts", ProjectKind::Jvm),
    ("settings.gradle", ProjectKind::Jvm),
    ("build.sbt", ProjectKind::Jvm),
    ("composer.json", ProjectKind::Php),
    ("Gemfile", ProjectKind::Ruby),
    ("mix.exs", ProjectKind::Elixir),
    ("pubspec.yaml", ProjectKind::Dart),
    ("Package.swift", ProjectKind::Swift),
    ("CMakeLists.txt", ProjectKind::Cmake),
    ("Makefile", ProjectKind::Make),
];

/// Manifests identified by extension rather than exact name.
const MANIFEST_EXTENSIONS: &[(&str, ProjectKind)] = &[
    ("csproj", ProjectKind::Dotnet),
    ("fsproj", ProjectKind::Dotnet),
    ("sln", ProjectKind::Dotnet),
    ("gemspec", ProjectKind::Ruby),
];

/// The ecosystem a project's manifest declares. Reported so a reader can
/// tell a Go service from a Node app without inspecting the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Node,
    Rust,
    Go,
    Python,
    Jvm,
    Php,
    Ruby,
    Dotnet,
    Elixir,
    Dart,
    Swift,
    Cmake,
    Make,
    /// A git repository with no manifest KOG recognises.
    Repository,
    /// Neither: the directory was scanned because it is what the user
    /// pointed at, not because anything declared it a project.
    Unknown,
}

impl std::fmt::Display for ProjectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            ProjectKind::Node => "node",
            ProjectKind::Rust => "rust",
            ProjectKind::Go => "go",
            ProjectKind::Python => "python",
            ProjectKind::Jvm => "jvm",
            ProjectKind::Php => "php",
            ProjectKind::Ruby => "ruby",
            ProjectKind::Dotnet => "dotnet",
            ProjectKind::Elixir => "elixir",
            ProjectKind::Dart => "dart",
            ProjectKind::Swift => "swift",
            ProjectKind::Cmake => "cmake",
            ProjectKind::Make => "make",
            ProjectKind::Repository => "repository",
            ProjectKind::Unknown => "unknown",
        };
        write!(f, "{text}")
    }
}

/// One project, and the graph of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Path relative to the scan root, `.` when the project *is* the root.
    /// Unique within a workspace, and used as the graph's identity.
    pub id: String,
    /// The project directory's own name, for display.
    pub name: String,
    /// Absolute path on disk.
    pub path: String,
    /// Every manifest kind found in the project directory, sorted. A
    /// directory can hold more than one — a Go service with a `package.json`
    /// for its tooling is still one project.
    pub kinds: Vec<ProjectKind>,
    pub graph: Graph,
}

/// Aggregate numbers over every project in a workspace.
///
/// Deliberately recomputed from the projects rather than accumulated during
/// the scan: a total that cannot be re-derived from the parts it claims to
/// summarise is not auditable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub projects: usize,
    pub nodes: usize,
    pub edges: usize,
    pub files_seen: usize,
    pub files_analysed: usize,
    pub files_unsupported: usize,
    pub specifiers_internal: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub excluded: usize,
    /// Resolved over in-scope internal specifiers, across every project.
    pub resolution_rate: f64,
    /// Analysed files over source files, across every project.
    pub source_coverage: f64,
}

/// What one scan produced: one project, or several.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    /// Absolute path the user pointed at.
    pub root: String,
    /// Whether the root was itself a project (one graph) or a container of
    /// them (one graph each).
    pub split: bool,
    pub projects: Vec<Project>,
    pub totals: Totals,
    /// Files under the root that belong to no project, when the root was
    /// split. Nothing disappears silently: a container directory with loose
    /// files says so rather than pretending the projects account for
    /// everything.
    pub unassigned_files: usize,
}

/// Whether `dir` holds a manifest, and which kinds.
fn manifest_kinds(dir: &Path) -> Vec<ProjectKind> {
    let mut kinds = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return kinds;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some((_, kind)) = MANIFESTS.iter().find(|(m, _)| *m == name) {
            kinds.push(*kind);
            continue;
        }
        if let Some(extension) = Path::new(name).extension().and_then(|e| e.to_str()) {
            if let Some((_, kind)) = MANIFEST_EXTENSIONS
                .iter()
                .find(|(e, _)| e.eq_ignore_ascii_case(extension))
            {
                kinds.push(*kind);
            }
        }
    }
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

fn is_repository(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Directories worth descending into while looking for projects: not a
/// build or dependency directory, and not hidden.
fn is_searchable(name: &str) -> bool {
    !ALWAYS_SKIP.contains(&name) && !name.starts_with('.')
}

/// Every project directory below `dir`, not descending past one that is
/// itself a project — a package inside a monorepo is part of that monorepo,
/// not a separate project.
fn marked_children(dir: &Path, depth: usize, found: &mut Vec<(PathBuf, Vec<ProjectKind>)>) {
    if depth > MAX_PROJECT_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_searchable)
        })
        .collect();
    // Sorted so a workspace's project order does not depend on readdir.
    children.sort();

    for child in children {
        let kinds = manifest_kinds(&child);
        if !kinds.is_empty() {
            found.push((child, kinds));
            continue; // Do not descend into a project.
        }
        if is_repository(&child) {
            found.push((child, vec![ProjectKind::Repository]));
            continue;
        }
        marked_children(&child, depth + 1, found);
    }
}

/// Decide what the scan root is: one project, or a container of several.
///
/// Returns the project directories with their kinds, and whether the root
/// was split.
fn locate_projects(root: &Path) -> (Vec<(PathBuf, Vec<ProjectKind>)>, bool) {
    let root_kinds = manifest_kinds(root);
    if !root_kinds.is_empty() {
        // Rule 1: the root declares itself. A monorepo lands here, and must:
        // its workspace packages and tsconfig aliases only resolve from the
        // root, so splitting it into its packages would break resolution
        // and inflate the unresolved count.
        return (vec![(root.to_path_buf(), root_kinds)], false);
    }

    let mut found = Vec::new();
    marked_children(root, 1, &mut found);

    // Rule 3: splitting is only the better answer when there is genuinely
    // more than one project. A lone project below an unmarked root is
    // scanned from the root, so loose files beside it are still mapped.
    if found.len() < 2 {
        let kinds = if is_repository(root) {
            vec![ProjectKind::Repository]
        } else {
            vec![ProjectKind::Unknown]
        };
        return (vec![(root.to_path_buf(), kinds)], false);
    }

    (found, true)
}

/// Count the files under `root` that fall inside none of `projects`,
/// applying the same directory rules the scan itself does.
fn count_unassigned(root: &Path, projects: &[(PathBuf, Vec<ProjectKind>)]) -> usize {
    crate::discover::survey(root)
        .files
        .iter()
        .filter(|file| !projects.iter().any(|(dir, _)| file.starts_with(dir)))
        .count()
}

/// Scan whatever the user pointed at: one project, or every project it
/// contains, each with its own graph.
pub fn scan_workspace(root: &Path) -> Workspace {
    let root: PathBuf = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let (located, split) = locate_projects(&root);

    let projects: Vec<Project> = located
        .iter()
        .map(|(dir, kinds)| {
            let registry = Registry::for_root(dir);
            let graph = build_graph(dir, &registry);
            let id = match dir.strip_prefix(&root) {
                Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
                Ok(rel) => rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
                Err(_) => dir.to_string_lossy().into_owned(),
            };
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.clone());
            Project {
                id,
                name,
                path: dir.to_string_lossy().into_owned(),
                kinds: kinds.clone(),
                graph,
            }
        })
        .collect();

    let unassigned_files = if split {
        count_unassigned(&root, &located)
    } else {
        0
    };

    Workspace {
        totals: totals_of(&projects),
        root: root.to_string_lossy().into_owned(),
        split,
        projects,
        unassigned_files,
    }
}

/// Re-derive the aggregate numbers from the projects themselves.
fn totals_of(projects: &[Project]) -> Totals {
    let mut totals = Totals {
        projects: projects.len(),
        ..Default::default()
    };
    for project in projects {
        let stats = &project.graph.stats;
        totals.nodes += project.graph.nodes.len();
        totals.edges += project.graph.edges.len();
        totals.files_seen += stats.coverage.files_seen;
        totals.files_analysed += stats.coverage.files_analysed;
        totals.files_unsupported += stats.coverage.files_unsupported;
        totals.specifiers_internal += stats.specifiers_internal;
        totals.resolved += stats.resolved;
        totals.unresolved += stats.unresolved;
        totals.excluded += stats.excluded;
    }
    let denominator = totals.specifiers_internal.saturating_sub(totals.excluded);
    totals.resolution_rate = if denominator == 0 {
        1.0
    } else {
        totals.resolved as f64 / denominator as f64
    };
    let source = totals.files_analysed + totals.files_unsupported;
    totals.source_coverage = if source == 0 {
        1.0
    } else {
        totals.files_analysed as f64 / source as f64
    };
    totals
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn a_directory_with_a_manifest_is_one_project() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", "");
        let workspace = scan_workspace(dir.path());

        assert!(!workspace.split);
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].id, ".");
        assert_eq!(workspace.projects[0].kinds, vec![ProjectKind::Node]);
        // The manifest is a node too — every file is.
        assert_eq!(workspace.projects[0].graph.nodes.len(), 2);
        assert_eq!(workspace.projects[0].graph.stats.coverage.files_analysed, 1);
    }

    /// The case this module exists for: a folder of unrelated projects.
    #[test]
    fn a_folder_of_projects_becomes_one_graph_each() {
        let dir = TempDir::new().unwrap();
        write(&dir, "web/package.json", r#"{"name":"web"}"#);
        write(&dir, "web/src/a.ts", r#"import b from "./b";"#);
        write(&dir, "web/src/b.ts", "");
        write(&dir, "api/go.mod", "module example.com/api\n");
        write(&dir, "api/main.go", "package main");
        let workspace = scan_workspace(dir.path());

        assert!(workspace.split);
        assert_eq!(workspace.projects.len(), 2);
        let ids: Vec<&str> = workspace.projects.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["api", "web"], "projects are ordered by path");
        assert_eq!(workspace.projects[1].kinds, vec![ProjectKind::Node]);
        assert_eq!(workspace.projects[1].graph.nodes.len(), 3);
        assert_eq!(
            workspace.projects[1].graph.edges.len(),
            1,
            "each project's graph is resolved from its own root"
        );
    }

    /// A monorepo must never be split: its packages resolve only from the
    /// root, so one graph per package would break every workspace import.
    #[test]
    fn a_monorepo_root_stays_one_project() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "package.json",
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        );
        write(&dir, "packages/a/package.json", r#"{"name":"@acme/a"}"#);
        write(&dir, "packages/b/package.json", r#"{"name":"@acme/b"}"#);
        let workspace = scan_workspace(dir.path());

        assert!(!workspace.split);
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].id, ".");
    }

    #[test]
    fn a_lone_project_below_an_unmarked_root_is_scanned_from_the_root() {
        // Splitting here would drop `notes.ts`, which belongs to no
        // project. One graph from the root keeps it.
        let dir = TempDir::new().unwrap();
        write(&dir, "app/package.json", r#"{"name":"app"}"#);
        write(&dir, "app/src/a.ts", "");
        write(&dir, "notes.ts", "");
        let workspace = scan_workspace(dir.path());

        assert!(!workspace.split);
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(
            workspace.projects[0].graph.nodes.len(),
            3,
            "the manifest, the app source, and the loose file beside it"
        );
    }

    #[test]
    fn a_git_repository_without_a_manifest_is_a_project() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("one/.git")).unwrap();
        write(&dir, "one/a.ts", "");
        fs::create_dir_all(dir.path().join("two/.git")).unwrap();
        write(&dir, "two/b.ts", "");
        let workspace = scan_workspace(dir.path());

        assert!(workspace.split);
        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(workspace.projects[0].kinds, vec![ProjectKind::Repository]);
    }

    #[test]
    fn projects_nested_deeper_than_one_level_are_found() {
        let dir = TempDir::new().unwrap();
        write(&dir, "services/api/go.mod", "module api\n");
        write(&dir, "services/web/package.json", r#"{"name":"web"}"#);
        let workspace = scan_workspace(dir.path());

        assert!(workspace.split);
        let ids: Vec<&str> = workspace.projects.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["services/api", "services/web"]);
    }

    #[test]
    fn files_belonging_to_no_project_are_counted_not_hidden() {
        let dir = TempDir::new().unwrap();
        write(&dir, "web/package.json", r#"{"name":"web"}"#);
        write(&dir, "api/go.mod", "module api\n");
        write(&dir, "scratch.ts", "");
        write(&dir, "notes/todo.md", "");
        let workspace = scan_workspace(dir.path());

        assert!(workspace.split);
        assert_eq!(
            workspace.unassigned_files, 2,
            "loose files must be reported, not silently dropped"
        );
    }

    #[test]
    fn totals_are_the_sum_of_the_projects_they_summarise() {
        let dir = TempDir::new().unwrap();
        write(&dir, "one/package.json", r#"{"name":"one"}"#);
        write(&dir, "one/a.ts", r#"import b from "./b";"#);
        write(&dir, "one/b.ts", "");
        write(&dir, "two/package.json", r#"{"name":"two"}"#);
        write(&dir, "two/c.ts", r#"import d from "./ghost";"#);
        let workspace = scan_workspace(dir.path());

        assert_eq!(workspace.totals.projects, 2);
        assert_eq!(workspace.totals.nodes, 5, "three sources and two manifests");
        assert_eq!(workspace.totals.edges, 1);
        assert_eq!(workspace.totals.resolved, 1);
        assert_eq!(workspace.totals.unresolved, 1);
        assert!((workspace.totals.resolution_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn an_empty_directory_is_still_one_scannable_project() {
        let dir = TempDir::new().unwrap();
        let workspace = scan_workspace(dir.path());
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].kinds, vec![ProjectKind::Unknown]);
        assert_eq!(workspace.totals.resolution_rate, 1.0);
    }

    #[test]
    fn dependency_directories_are_never_mistaken_for_projects() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "node_modules/left-pad/package.json",
            r#"{"name":"lp"}"#,
        );
        write(
            &dir,
            "node_modules/right-pad/package.json",
            r#"{"name":"rp"}"#,
        );
        write(&dir, "src/a.ts", "");
        let workspace = scan_workspace(dir.path());

        assert!(
            !workspace.split,
            "packages inside node_modules are not projects"
        );
        assert_eq!(workspace.projects.len(), 1);
    }
}
