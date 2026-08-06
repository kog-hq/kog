//! Which extractor reads which file.
//!
//! One scan, many languages: the registry is what turns a set of
//! single-language front ends into an engine that maps a polyglot
//! repository. It owns the extractors, and it owns the one rule that
//! matters when two of them claim the same extension.

use crate::extractor::Extractor;
use crate::extractors::{
    CLikeExtractor, CssExtractor, GoExtractor, HtmlExtractor, PythonExtractor, RustExtractor,
    SfcExtractor, ShellExtractor, TypeScriptExtractor,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Every extractor a scan will use, indexed by the extensions they claim.
pub struct Registry {
    extractors: Vec<Box<dyn Extractor>>,
    /// Lowercased extension (no leading dot) to an index into `extractors`.
    by_extension: BTreeMap<String, usize>,
    /// Extensions more than one extractor claimed, with the language that
    /// lost. Never silent: a claim that was overridden changes what the
    /// graph contains, so it is inspectable and asserted against in tests.
    conflicts: Vec<(String, &'static str)>,
}

impl Registry {
    /// Build a registry from an explicit list, in priority order: the first
    /// extractor to claim an extension keeps it.
    ///
    /// Order is the whole contract here. `.h` is claimed by both C and C++,
    /// `.js` by both TypeScript (which resolves it correctly, because a
    /// `.js` specifier so often names a `.ts` file) and any future
    /// JavaScript-only front end. Deciding by list order makes the winner a
    /// reviewable line in one place rather than an accident of iteration.
    pub fn new(extractors: Vec<Box<dyn Extractor>>) -> Self {
        let mut by_extension: BTreeMap<String, usize> = BTreeMap::new();
        let mut conflicts = Vec::new();
        for (index, extractor) in extractors.iter().enumerate() {
            for extension in extractor.extensions() {
                let key = extension.to_ascii_lowercase();
                if by_extension.contains_key(&key) {
                    conflicts.push((key, extractor.lang()));
                    continue;
                }
                by_extension.insert(key, index);
            }
        }
        Self {
            extractors,
            by_extension,
            conflicts,
        }
    }

    /// A registry holding one extractor. Used by tests, and by any caller
    /// that deliberately wants a single-language scan.
    pub fn single(extractor: Box<dyn Extractor>) -> Self {
        Self::new(vec![extractor])
    }

    /// Every language KOG can read, configured for one project root.
    ///
    /// Extractors are constructed here rather than lazily because several
    /// of them index the project up front — TypeScript reads every
    /// `tsconfig` and the workspace layout, and the languages whose imports
    /// name a symbol rather than a path have to know where the source roots
    /// are before they can resolve anything.
    pub fn for_root(root: &Path) -> Self {
        // Built once and shared: the single-file-component front end
        // delegates every resolution to it, and a second copy would walk the
        // project again for tsconfigs and workspace packages.
        let typescript = Arc::new(TypeScriptExtractor::new(root));
        Self::new(vec![
            // TypeScript first: it claims `.js` and `.jsx` deliberately, and
            // resolves them better than a JavaScript-only front end could —
            // a `.js` specifier so often names a `.ts` file on disk.
            Box::new(Arc::clone(&typescript)),
            Box::new(SfcExtractor::new(Arc::clone(&typescript))),
            Box::new(GoExtractor::new(root)),
            Box::new(PythonExtractor::new(root)),
            Box::new(RustExtractor::new(root)),
            Box::new(CLikeExtractor::new(root)),
            Box::new(HtmlExtractor::new(root)),
            Box::new(CssExtractor::new(root)),
            Box::new(ShellExtractor::new(root)),
        ])
    }

    /// The extractor that claims this extension, if any. The extension is
    /// given without a leading dot; matching is case-insensitive.
    pub fn for_extension(&self, extension: &str) -> Option<&dyn Extractor> {
        let key = extension.to_ascii_lowercase();
        self.by_extension
            .get(&key)
            .map(|index| self.extractors[*index].as_ref())
    }

    /// Every extractor, in priority order.
    pub fn extractors(&self) -> impl Iterator<Item = &dyn Extractor> {
        self.extractors.iter().map(|e| e.as_ref())
    }

    /// Every extension any extractor claims, sorted.
    pub fn extensions(&self) -> Vec<&str> {
        self.by_extension.keys().map(String::as_str).collect()
    }

    /// Claims that were overridden by an earlier extractor, as
    /// `(extension, losing language)`.
    pub fn conflicts(&self) -> &[(String, &'static str)] {
        &self.conflicts
    }

    /// Every language the registry can read, sorted and deduplicated.
    pub fn languages(&self) -> Vec<&'static str> {
        let mut langs: Vec<&'static str> = self.extractors.iter().map(|e| e.lang()).collect();
        langs.sort_unstable();
        langs.dedup();
        langs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractor::{ExtractError, Resolution, Specifier};
    use std::path::PathBuf;

    struct Stub(&'static str, &'static [&'static str]);

    impl Extractor for Stub {
        fn lang(&self) -> &'static str {
            self.0
        }
        fn extensions(&self) -> &'static [&'static str] {
            self.1
        }
        fn extract(&self, _source: &str) -> Result<Vec<Specifier>, ExtractError> {
            Ok(Vec::new())
        }
        fn resolve(&self, _raw: &str, _importer: &Path) -> Resolution {
            Resolution::Unresolved
        }
    }

    #[test]
    fn an_extension_maps_to_the_extractor_that_claims_it() {
        let registry = Registry::new(vec![
            Box::new(Stub("go", &["go"])),
            Box::new(Stub("rust", &["rs"])),
        ]);
        assert_eq!(registry.for_extension("go").unwrap().lang(), "go");
        assert_eq!(registry.for_extension("rs").unwrap().lang(), "rust");
        assert!(registry.for_extension("py").is_none());
    }

    #[test]
    fn extension_lookup_ignores_case() {
        let registry = Registry::single(Box::new(Stub("go", &["go"])));
        assert!(registry.for_extension("GO").is_some());
    }

    #[test]
    fn the_first_extractor_to_claim_an_extension_keeps_it() {
        let registry = Registry::new(vec![
            Box::new(Stub("c", &["h", "c"])),
            Box::new(Stub("cpp", &["h", "cpp"])),
        ]);
        assert_eq!(registry.for_extension("h").unwrap().lang(), "c");
        assert_eq!(registry.conflicts(), &[("h".to_string(), "cpp")]);
    }

    #[test]
    fn a_registry_reports_every_extension_and_language_it_covers() {
        let registry = Registry::new(vec![
            Box::new(Stub("go", &["go"])),
            Box::new(Stub("rust", &["rs"])),
        ]);
        assert_eq!(registry.extensions(), vec!["go", "rs"]);
        assert_eq!(registry.languages(), vec!["go", "rust"]);
    }

    /// The shipped registry must not contain a silent override: every
    /// conflict is a deliberate line in `for_root`, and a new extractor
    /// that quietly steals `.js` from TypeScript would change every
    /// JavaScript project's graph without anyone reading a diff.
    #[test]
    fn the_shipped_registry_has_no_unintended_extension_conflicts() {
        let registry = Registry::for_root(&PathBuf::from("."));
        assert_eq!(
            registry.conflicts(),
            &[],
            "an extension is claimed twice; give it to one extractor \
             deliberately or reorder `Registry::for_root`"
        );
    }

    /// A tree-sitter query naming a node type its grammar does not have
    /// fails at *query compile* time, which happens once per file — so a
    /// single typo silently zeroes out a whole language across an entire
    /// scan while every other language still reports a healthy rate. This
    /// test compiles every shipped query once, so that failure is a red
    /// test rather than a missing third of a graph.
    #[test]
    fn every_shipped_extractor_compiles_its_query() {
        let registry = Registry::for_root(&PathBuf::from("."));
        for extractor in registry.extractors() {
            let result = extractor.extract("");
            assert!(
                result.is_ok(),
                "{}: {:?} — the grammar or the query is wrong",
                extractor.lang(),
                result.err()
            );
        }
    }

    /// Every extension an extractor claims must be one the catalogue knows
    /// is source code. A claim on an unlisted extension would produce a
    /// coverage report that contradicts itself: analysed here, "not source"
    /// in the table.
    #[test]
    fn every_claimed_extension_is_catalogued_as_source() {
        let registry = Registry::for_root(&PathBuf::from("."));
        for extension in registry.extensions() {
            let classification = crate::catalogue::classify(extension, "x");
            assert_eq!(
                classification.kind,
                crate::catalogue::Kind::Source,
                ".{extension} is claimed by an extractor but is not listed as \
                 source in the catalogue"
            );
        }
    }
}
