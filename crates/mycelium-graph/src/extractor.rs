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
    /// A third-party package. Recorded on the node, never an edge.
    External(String),
    /// Looks internal but no file was found. Counted, never dropped silently.
    Unresolved,
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
pub trait Extractor {
    /// Stable language identifier, written into `Node::lang`.
    fn lang(&self) -> &'static str;

    /// File extensions this extractor claims, without the leading dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Pull every static import specifier out of one source file.
    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError>;

    /// Turn one specifier into a resolution, given the file that wrote it.
    fn resolve(&self, raw: &str, importer: &Path) -> Resolution;
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
}
