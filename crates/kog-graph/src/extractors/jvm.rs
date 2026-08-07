//! Java: an import names a fully qualified type, and the language's own
//! rule is that the type's file sits at the matching path under a source
//! root.
//!
//! `import com.acme.billing.Invoice;` is `com/acme/billing/Invoice.java`
//! under `src/main/java`, `src/`, or whatever directory the project treats
//! as its root of packages. That correspondence is enforced by the compiler,
//! which makes it the rare case where resolution needs no heuristics — only
//! an index built the same way, once, at construction.
//!
//! A wildcard import (`import com.acme.billing.*;`) names a package rather
//! than a type, so it resolves to every file in it — one specifier, a set of
//! targets, exactly like a Go package.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Both forms have the same shape: `import a.b.C;` and
/// `import static a.b.C.method;` differ only by a keyword the query ignores,
/// and the trailing `.*` of a wildcard is a sibling node, not part of the
/// identifier — so it is recovered from the statement text.
const IMPORT_QUERY: &str = r#"
(import_declaration) @import
"#;

const EXTENSIONS: &[&str] = &["java"];

/// Path segments that mark the start of a package hierarchy. Everything
/// after one of these is the package path; `com/acme/Foo.java` under
/// `src/main/java` is `com.acme.Foo`.
const SOURCE_ROOT_MARKERS: &[&str] = &["java", "kotlin", "src"];

pub struct JavaExtractor {
    /// Fully qualified name to the file that declares it.
    types: BTreeMap<String, PathBuf>,
}

impl JavaExtractor {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut types = BTreeMap::new();
        for file in crate::discover::discover(&root, EXTENSIONS) {
            if let Some(fqn) = Self::qualified_name(&root, &file) {
                types.insert(fqn, file);
            }
        }
        Self { types }
    }

    /// The fully qualified name a file's *path* implies.
    ///
    /// Derived from the path rather than from the `package` declaration
    /// inside the file, because the index has to exist before any file is
    /// parsed. The two agree in any project that compiles: `javac` rejects a
    /// public type whose file path does not match its package.
    fn qualified_name(root: &Path, file: &Path) -> Option<String> {
        let relative = file.strip_prefix(root).ok()?;
        let segments: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        // Everything after the last source-root marker; if there is none,
        // the path from the scan root, which is what a flat project has.
        let start = segments
            .iter()
            .rposition(|s| SOURCE_ROOT_MARKERS.contains(&s.as_str()))
            .map(|index| index + 1)
            .unwrap_or(0);
        let mut path: Vec<&str> = segments[start..].iter().map(String::as_str).collect();
        let last = path.pop()?;
        path.push(last.strip_suffix(".java")?);
        Some(path.join("."))
    }
}

impl Extractor for JavaExtractor {
    fn lang(&self) -> &'static str {
        "java"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_java::LANGUAGE.into();
        let captures = support::captures(&language, "java", IMPORT_QUERY, source)?;
        Ok(captures
            .into_iter()
            .filter_map(|capture| {
                // `import static com.acme.Assertions.assertThat;` — the
                // keywords and the semicolon are noise, the dotted path is
                // the specifier.
                let raw = capture
                    .text
                    .trim_start_matches("import")
                    .trim()
                    .trim_start_matches("static")
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                (!raw.is_empty()).then_some(Specifier {
                    raw,
                    line: capture.line,
                })
            })
            .collect())
    }

    fn resolve(&self, raw: &str, _importer: &Path) -> Resolution {
        // A wildcard names a package: every type in it.
        if let Some(package) = raw.strip_suffix(".*") {
            let prefix = format!("{package}.");
            let files: Vec<PathBuf> = self
                .types
                .iter()
                .filter(|(fqn, _)| {
                    // Direct members only: Java's `*` does not import
                    // sub-packages, so `com.acme.*` must not pull in
                    // `com.acme.billing.Invoice`.
                    fqn.strip_prefix(&prefix)
                        .is_some_and(|rest| !rest.contains('.'))
                })
                .map(|(_, path)| path.clone())
                .collect();
            return if files.is_empty() {
                Resolution::External(package.to_string())
            } else {
                Resolution::InternalSet(files)
            };
        }

        if let Some(path) = self.types.get(raw) {
            return Resolution::Internal(path.clone());
        }
        // A static import names a member of a type: drop the last segment
        // and try the type itself.
        if let Some((owner, _)) = raw.rsplit_once('.') {
            if let Some(path) = self.types.get(owner) {
                return Resolution::Internal(path.clone());
            }
        }

        // Not a type in this project: the JDK or a dependency. Reduced to
        // the package, which is what a dependency is in Java — `java.util`,
        // not `java.util.List`.
        Resolution::External(match raw.rsplit_once('.') {
            Some((package, _)) => package.to_string(),
            None => raw.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn root(dir: &TempDir) -> PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = root(dir).join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn every_import_form_is_extracted_as_a_dotted_path() {
        let dir = TempDir::new().unwrap();
        let e = JavaExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "package com.acme;\n\nimport com.acme.billing.Invoice;\nimport static org.junit.Assert.assertEquals;\nimport java.util.*;\n\nclass Main {}\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec![
                "com.acme.billing.Invoice",
                "org.junit.Assert.assertEquals",
                "java.util.*"
            ]
        );
        assert_eq!(specifiers[0].line, 3);
    }

    #[test]
    fn a_type_resolves_through_the_maven_source_layout() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main/java/com/acme/Main.java", "");
        write(&dir, "src/main/java/com/acme/billing/Invoice.java", "");
        let e = JavaExtractor::new(dir.path());

        let resolution = e.resolve(
            "com.acme.billing.Invoice",
            &root(&dir).join("src/main/java/com/acme/Main.java"),
        );
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("billing/Invoice.java")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_wildcard_import_resolves_to_every_type_in_that_package_only() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main/java/com/acme/Main.java", "");
        write(&dir, "src/main/java/com/acme/billing/Invoice.java", "");
        write(&dir, "src/main/java/com/acme/billing/Ledger.java", "");
        write(&dir, "src/main/java/com/acme/billing/tax/Vat.java", "");
        let e = JavaExtractor::new(dir.path());

        let resolution = e.resolve("com.acme.billing.*", &root(&dir).join("x.java"));
        assert_eq!(
            resolution.targets().len(),
            2,
            "`*` imports the package's own types, never its sub-packages"
        );
    }

    #[test]
    fn a_static_import_resolves_to_the_type_that_owns_the_member() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main/java/com/acme/Assertions.java", "");
        let e = JavaExtractor::new(dir.path());

        let resolution = e.resolve(
            "com.acme.Assertions.assertThat",
            &root(&dir).join("src/main/java/com/acme/Main.java"),
        );
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("Assertions.java")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn the_jdk_and_dependencies_reduce_to_their_package() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main/java/com/acme/Main.java", "");
        let e = JavaExtractor::new(dir.path());
        let importer = root(&dir).join("src/main/java/com/acme/Main.java");
        assert_eq!(
            e.resolve("java.util.List", &importer),
            Resolution::External("java.util".into())
        );
        assert_eq!(
            e.resolve("java.util.*", &importer),
            Resolution::External("java.util".into())
        );
    }

    #[test]
    fn a_flat_project_with_no_source_root_still_indexes() {
        let dir = TempDir::new().unwrap();
        write(&dir, "com/acme/Main.java", "");
        write(&dir, "com/acme/Helper.java", "");
        let e = JavaExtractor::new(dir.path());

        let resolution = e.resolve("com.acme.Helper", &root(&dir).join("com/acme/Main.java"));
        assert!(
            matches!(resolution, Resolution::Internal(_)),
            "got {resolution:?}"
        );
    }
}
