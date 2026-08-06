use crate::tsconfig::SkippedConfig;
use std::path::{Path, PathBuf};

/// One import specifier as written in the source, before any resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specifier {
    /// The literal text between the quotes, e.g. `@/lib/api` or `react`.
    pub raw: String,
    /// One-based line number, for diagnostics.
    pub line: usize,
}

/// What a specifier turned out to point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A file inside the scanned project. Path is absolute.
    Internal(PathBuf),
    /// Several files inside the project, all named by one specifier.
    ///
    /// Not every language's import names a file. Go imports a *package* —
    /// a directory of files, any of which may hold the symbol being used —
    /// and C# imports a namespace, which can be declared across many files.
    /// One specifier, one resolution, many edges: the specifier counts once
    /// towards the rate, and the graph gains an edge per target that is a
    /// node. Paths are absolute.
    InternalSet(Vec<PathBuf>),
    /// A third-party package. Recorded on the node, never an edge.
    External(String),
    /// Looks internal but no file was found. Counted, never dropped silently.
    Unresolved,
}

impl Resolution {
    /// Every internal target, as a slice-like vector: one for
    /// [`Resolution::Internal`], all of them for [`Resolution::InternalSet`],
    /// none for the other two. Lets the graph assembler treat both internal
    /// shapes through one code path.
    pub fn targets(&self) -> &[PathBuf] {
        match self {
            Resolution::Internal(path) => std::slice::from_ref(path),
            Resolution::InternalSet(paths) => paths,
            _ => &[],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("failed to load the {0} grammar")]
    Grammar(&'static str),
    #[error("failed to parse the source")]
    Parse,
}

/// A language front end. One implementation per supported language.
///
/// A language ships only once it passes its own resolution gate — see the design
/// document, section 3.3.
pub trait Extractor: Send + Sync {
    /// Stable language identifier, written into `Node::lang`.
    fn lang(&self) -> &'static str;

    /// The language label for one particular file.
    ///
    /// Defaults to [`Extractor::lang`]. Overridden by extractors that cover
    /// more than one language with one grammar: TypeScript and JavaScript
    /// share an import syntax and a resolver, but a `.js` file is not a
    /// TypeScript file, and per-language statistics that said so would be
    /// wrong.
    fn lang_for(&self, _path: &Path) -> &'static str {
        self.lang()
    }

    /// File extensions this extractor claims, without the leading dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Pull every static import specifier out of one source file.
    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError>;

    /// Turn one specifier into a resolution, given the file that wrote it.
    fn resolve(&self, raw: &str, importer: &Path) -> Resolution;

    /// Config files this extractor found but could not use — e.g. an
    /// unreadable or malformed tsconfig. The subtree such a config governs
    /// silently degrades to weaker resolution, so this must never disappear
    /// (design doc §7). Paths are absolute; `build_graph` turns them into
    /// project-relative `Failure`s like every other entry in
    /// `stats.failures`.
    ///
    /// Empty by default: most extractors have no notion of a skipped config.
    fn skipped_configs(&self) -> &[SkippedConfig] {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct StubExtractor;

    impl Extractor for StubExtractor {
        fn lang(&self) -> &'static str {
            "stub"
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["stub"]
        }
        fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
            Ok(source
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.is_empty())
                .map(|(i, l)| Specifier {
                    raw: l.to_string(),
                    line: i + 1,
                })
                .collect())
        }
        fn resolve(&self, raw: &str, _importer: &Path) -> Resolution {
            if raw.starts_with('.') {
                Resolution::Internal(PathBuf::from(raw))
            } else {
                Resolution::External(raw.to_string())
            }
        }
    }

    #[test]
    fn an_extractor_reports_its_language_and_extensions() {
        let e = StubExtractor;
        assert_eq!(e.lang(), "stub");
        assert_eq!(e.extensions(), &["stub"]);
    }

    #[test]
    fn the_per_file_language_defaults_to_the_extractors_own() {
        assert_eq!(StubExtractor.lang_for(Path::new("a.stub")), "stub");
    }

    #[test]
    fn extraction_carries_the_line_number() {
        let specs = StubExtractor.extract("./a\n./b").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].line, 2);
    }

    #[test]
    fn resolution_separates_internal_from_external() {
        let e = StubExtractor;
        let here = Path::new("src/x.stub");
        assert!(matches!(e.resolve("./a", here), Resolution::Internal(_)));
        assert!(matches!(e.resolve("react", here), Resolution::External(_)));
    }

    #[test]
    fn a_boxed_extractor_stays_usable() {
        let boxed: Box<dyn Extractor> = Box::new(StubExtractor);
        assert_eq!(boxed.lang(), "stub");
    }

    #[test]
    fn both_internal_shapes_expose_their_targets_the_same_way() {
        let one = Resolution::Internal(PathBuf::from("/a.go"));
        let many = Resolution::InternalSet(vec![PathBuf::from("/a.go"), PathBuf::from("/b.go")]);
        assert_eq!(one.targets().len(), 1);
        assert_eq!(many.targets().len(), 2);
        assert!(Resolution::External("fmt".into()).targets().is_empty());
        assert!(Resolution::Unresolved.targets().is_empty());
    }
}
