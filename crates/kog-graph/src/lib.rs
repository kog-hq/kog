//! Turns a codebase into a file/import graph.

pub mod catalogue;
pub mod discover;
pub mod extractor;
pub mod extractors;
pub mod graph;
pub mod model;
pub mod project;
pub mod registry;
pub mod tsconfig;

pub use catalogue::{classify, Classification, Kind};
pub use discover::{build_walker, discover, is_in_skipped_directory, survey, Survey, ALWAYS_SKIP};
pub use extractor::{ExtractError, Extractor, Resolution, Specifier};
pub use extractors::TypeScriptExtractor;
pub use graph::build_graph;
pub use model::{
    Coverage, Diagnostic, DiagnosticKind, Edge, EdgeKind, ExclusionReason, ExtensionCoverage,
    Failure, FileStatus, Graph, LangStats, Node, SkippedDirectory, Stats, MAX_DIAGNOSTICS,
};
pub use project::{scan_workspace, Project, ProjectKind, Workspace};
pub use registry::Registry;
pub use tsconfig::{PathMapping, SkippedConfig, TsConfigIndex};
