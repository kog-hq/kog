//! Turns a codebase into a file/import graph.

pub mod discover;
pub mod extractor;
pub mod model;
pub mod tsconfig;

pub use discover::{build_walker, discover};
pub use extractor::{ExtractError, Extractor, Resolution, Specifier};
pub use model::{Edge, EdgeKind, Failure, Graph, Node, Stats};
pub use tsconfig::{PathMapping, SkippedConfig, TsConfigIndex};
