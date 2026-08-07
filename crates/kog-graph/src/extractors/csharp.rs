//! C#: `using` names a namespace, and a namespace is not a file.
//!
//! Nothing in a C# project maps a namespace to a path — the compiler does
//! not require the two to agree, and large codebases regularly disagree on
//! purpose. So the mapping is built by reading what each file *declares*:
//! every `namespace X.Y` (block or file-scoped) is indexed at construction,
//! and a `using X.Y` resolves to every file that declares it.
//!
//! That makes one `using` an edge to each file of the namespace, the same
//! shape as a Go package import. It is coarse, and it is what a file-level
//! graph of C# means: the alternative is a type resolver, which needs the
//! compiler.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const IMPORT_QUERY: &str = r#"
(using_directive) @using
"#;

const EXTENSIONS: &[&str] = &["cs"];

pub struct CSharpExtractor {
    /// Namespace to the files that declare it.
    namespaces: BTreeMap<String, Vec<PathBuf>>,
}

impl CSharpExtractor {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut namespaces: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for file in crate::discover::discover(&root, EXTENSIONS) {
            for declared in Self::declared_namespaces(&file) {
                namespaces.entry(declared).or_default().push(file.clone());
            }
        }
        for files in namespaces.values_mut() {
            files.sort();
            files.dedup();
        }
        Self { namespaces }
    }

    /// The namespaces one file declares.
    ///
    /// Read line by line rather than parsed: the index has to exist before
    /// any file is handed to an extractor, and a `namespace` declaration is
    /// a single line in both of C#'s forms — `namespace X.Y {` and the
    /// file-scoped `namespace X.Y;`. Reading the whole file with a grammar
    /// here would parse the project twice for one token.
    fn declared_namespaces(file: &Path) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(file) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix("namespace ")?;
                let name = rest
                    .split(|c: char| c == '{' || c == ';' || c.is_whitespace())
                    .next()?
                    .trim();
                (!name.is_empty()).then(|| name.to_string())
            })
            .collect()
    }
}

impl Extractor for CSharpExtractor {
    fn lang(&self) -> &'static str {
        "csharp"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
        let captures = support::captures(&language, "csharp", IMPORT_QUERY, source)?;
        Ok(captures
            .into_iter()
            .filter_map(|capture| {
                // `using static System.Math;`, `global using X;` and
                // `using Alias = A.B.C;` all reduce to the namespace path:
                // the keywords are dropped, and an alias keeps its target.
                let text = capture.text.trim().trim_end_matches(';');
                let text = text
                    .trim_start_matches("global")
                    .trim()
                    .trim_start_matches("using")
                    .trim()
                    .trim_start_matches("static")
                    .trim();
                let raw = match text.split_once('=') {
                    Some((_, target)) => target.trim(),
                    None => text,
                };
                (!raw.is_empty()).then_some(Specifier {
                    raw: raw.to_string(),
                    line: capture.line,
                })
            })
            .collect())
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        if let Some(files) = self.namespaces.get(raw) {
            // A file's own namespace is not an import of itself; the graph
            // assembler drops a self-edge, but the specifier still counts
            // as resolved, which it is.
            let targets: Vec<PathBuf> = files.iter().filter(|f| *f != importer).cloned().collect();
            if !targets.is_empty() {
                return Resolution::InternalSet(targets);
            }
        }
        // `System`, `System.Text.Json`, a NuGet package: nothing in this
        // project declares it.
        Resolution::External(raw.to_string())
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
    fn every_using_form_reduces_to_the_namespace_it_names() {
        let dir = TempDir::new().unwrap();
        let e = CSharpExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "using System;\nglobal using System.Linq;\nusing static System.Math;\nusing Json = System.Text.Json;\n\nnamespace App;\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["System", "System.Linq", "System.Math", "System.Text.Json"]
        );
    }

    #[test]
    fn a_using_resolves_to_every_file_declaring_that_namespace() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/Program.cs",
            "using App.Billing;\nnamespace App;\n",
        );
        write(&dir, "src/Billing/Invoice.cs", "namespace App.Billing;\n");
        write(
            &dir,
            "src/Billing/Ledger.cs",
            "namespace App.Billing\n{\n}\n",
        );
        write(&dir, "src/Other.cs", "namespace App.Other;\n");
        let e = CSharpExtractor::new(dir.path());

        let resolution = e.resolve("App.Billing", &root(&dir).join("src/Program.cs"));
        assert_eq!(
            resolution.targets().len(),
            2,
            "both declaration forms must be indexed, got {resolution:?}"
        );
    }

    #[test]
    fn a_file_does_not_import_its_own_namespace() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/Only.cs", "using App;\nnamespace App;\n");
        let e = CSharpExtractor::new(dir.path());

        // The only file declaring `App` is the importer itself, so there is
        // nothing to point at and the specifier is external rather than a
        // self-edge.
        assert!(matches!(
            e.resolve("App", &root(&dir).join("src/Only.cs")),
            Resolution::External(_)
        ));
    }

    #[test]
    fn the_framework_and_packages_stay_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/Program.cs", "namespace App;\n");
        let e = CSharpExtractor::new(dir.path());
        assert_eq!(
            e.resolve("System.Text.Json", &root(&dir).join("src/Program.cs")),
            Resolution::External("System.Text.Json".into())
        );
    }
}
