//! Turns a codebase into a file/import graph.

pub mod discover;
pub mod extractor;
pub mod extractors;
pub mod graph;
pub mod model;
pub mod tsconfig;

pub use discover::{build_walker, discover};
pub use extractor::{ExtractError, Extractor, Resolution, Specifier};
pub use extractors::TypeScriptExtractor;
pub use graph::build_graph;
pub use model::{Edge, EdgeKind, Failure, Graph, Node, Stats};
pub use tsconfig::{PathMapping, SkippedConfig, TsConfigIndex};
