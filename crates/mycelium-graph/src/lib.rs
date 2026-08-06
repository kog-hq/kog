//! Turns a codebase into a file/import graph.

pub mod extractor;
pub mod model;
pub mod tsconfig;

pub use extractor::{ExtractError, Extractor, Resolution, Specifier};
pub use model::{Edge, EdgeKind, Failure, Graph, Node, Stats};
pub use tsconfig::{PathMapping, TsConfigIndex};
