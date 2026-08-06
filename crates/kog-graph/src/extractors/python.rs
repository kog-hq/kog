//! Python: dotted module names, and the relative imports that make a
//! package's own shape legible.
//!
//! Two forms carry a target: `import a.b.c` and `from a.b import x`. Only
//! the module part is resolved — `x` may be a symbol rather than a module,
//! and guessing would produce edges that are not imports.
//!
//! Relative imports (`from . import x`, `from ..pkg import y`) are exact:
//! the dots count directory levels from the importing file, so they always
//! resolve or genuinely do not exist. Absolute imports are ambiguous by
//! design — Python resolves them against `sys.path`, which is a runtime
//! value — so they are probed against the source roots a project actually
//! uses, nearest first, and anything not found is a dependency.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::path::{Path, PathBuf};

const IMPORT_QUERY: &str = r#"
(import_statement name: (dotted_name) @spec)
(import_statement name: (aliased_import name: (dotted_name) @spec))
(import_from_statement module_name: (dotted_name) @spec)
(import_from_statement module_name: (relative_import) @spec)
"#;

const EXTENSIONS: &[&str] = &["py", "pyi"];
const INDEX_NAMES: &[&str] = &["__init__.py", "__init__.pyi"];

/// Directories a project conventionally puts its top-level packages in.
/// Probed in addition to the root itself and the importer's own ancestors.
const SOURCE_DIRS: &[&str] = &["src", "lib", "app"];

pub struct PythonExtractor {
    root: PathBuf,
}

impl PythonExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    /// Resolve `from .x import y` and friends: the leading dots are a
    /// directory walk, not a guess. One dot is the importer's own package,
    /// each further dot one level up.
    fn resolve_relative(&self, raw: &str, importer: &Path) -> Resolution {
        let dots = raw.chars().take_while(|c| *c == '.').count();
        let rest = &raw[dots..];

        let Some(mut base) = importer.parent().map(Path::to_path_buf) else {
            return Resolution::Unresolved;
        };
        for _ in 1..dots {
            match base.parent() {
                Some(parent) => base = parent.to_path_buf(),
                None => return Resolution::Unresolved,
            }
        }
        if !support::contained(&self.root, &base) {
            return Resolution::Unresolved;
        }

        // `from . import x` has no module part at all: the target is the
        // package directory the importer sits in.
        let candidate = if rest.is_empty() {
            base
        } else {
            support::normalise(&base.join(rest.replace('.', "/")))
        };
        match support::probe(&candidate, EXTENSIONS, INDEX_NAMES) {
            Some(path) => Resolution::Internal(path),
            None => Resolution::Unresolved,
        }
    }

    /// Where an absolute dotted name might live, nearest to the importing
    /// file first. Python's real answer is `sys.path`, which only exists at
    /// runtime; these are the directories a project actually lays its
    /// packages out in.
    fn source_roots(&self, importer: &Path) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        // The importer's own directory and every ancestor up to the scan
        // root: this is what makes `services/api/app/main.py` importing
        // `app.models` work without any configuration.
        let mut current = importer.parent();
        while let Some(dir) = current {
            if !support::contained(&self.root, dir) {
                break;
            }
            roots.push(dir.to_path_buf());
            if dir == self.root {
                break;
            }
            current = dir.parent();
        }
        for name in SOURCE_DIRS {
            roots.push(self.root.join(name));
        }
        roots.push(self.root.clone());
        roots.dedup();
        roots
    }
}

impl Extractor for PythonExtractor {
    fn lang(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        Ok(support::specifiers_from(support::captures(
            &language,
            "python",
            IMPORT_QUERY,
            source,
        )?))
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        if raw.starts_with('.') {
            return self.resolve_relative(raw, importer);
        }

        let relative = raw.replace('.', "/");
        for root in self.source_roots(importer) {
            let candidate = support::normalise(&root.join(&relative));
            if !support::contained(&self.root, &candidate) {
                continue;
            }
            if let Some(path) = support::probe(&candidate, EXTENSIONS, INDEX_NAMES) {
                return Resolution::Internal(path);
            }
        }

        // Not found anywhere in the project: the standard library or a
        // dependency. Reduced to the distributed package's name — `os.path`
        // is the `os` module, and `django.db.models` is one dependency.
        Resolution::External(raw.split('.').next().unwrap_or(raw).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// `TempDir` hands back a non-canonical path on macOS (`/var` is a
    /// symlink to `/private/var`). `build_graph` always passes canonical
    /// paths to an extractor, so tests must too, or every containment check
    /// fails for a reason production never sees.
    fn root(dir: &TempDir) -> PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = root(dir).join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn both_import_forms_are_extracted_with_their_lines() {
        let dir = TempDir::new().unwrap();
        let e = PythonExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "import os\nimport app.models\nfrom app.db import session\nfrom . import sibling\nfrom ..pkg.deep import thing\nimport numpy as np\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["os", "app.models", "app.db", ".", "..pkg.deep", "numpy"]
        );
        assert_eq!(specifiers[1].line, 2);
    }

    #[test]
    fn a_relative_import_resolves_from_the_importing_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/__init__.py", "");
        write(&dir, "app/main.py", "from .models import User");
        write(&dir, "app/models.py", "");
        let e = PythonExtractor::new(dir.path());

        let resolution = e.resolve(".models", &root(&dir).join("app/main.py"));
        assert!(matches!(resolution, Resolution::Internal(ref p) if p.ends_with("app/models.py")));
    }

    #[test]
    fn a_bare_relative_import_resolves_to_the_packages_init() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/__init__.py", "");
        write(&dir, "app/main.py", "from . import config");
        let e = PythonExtractor::new(dir.path());

        let resolution = e.resolve(".", &root(&dir).join("app/main.py"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("app/__init__.py")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_double_dot_import_climbs_one_package() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/__init__.py", "");
        write(&dir, "app/util.py", "");
        write(&dir, "app/api/__init__.py", "");
        write(&dir, "app/api/routes.py", "from ..util import helper");
        let e = PythonExtractor::new(dir.path());

        let resolution = e.resolve("..util", &root(&dir).join("app/api/routes.py"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("app/util.py")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_relative_import_climbing_past_the_scan_root_is_unresolved_not_a_traversal() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/main.py", "");
        let e = PythonExtractor::new(dir.path());
        assert_eq!(
            e.resolve("....secrets", &root(&dir).join("app/main.py")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn an_absolute_import_resolves_against_a_src_layout() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/app/__init__.py", "");
        write(&dir, "src/app/models.py", "");
        write(&dir, "tests/test_models.py", "from app.models import User");
        let e = PythonExtractor::new(dir.path());

        let resolution = e.resolve("app.models", &root(&dir).join("tests/test_models.py"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/app/models.py")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn an_absolute_import_resolves_to_a_package_directory_via_its_init() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/__init__.py", "");
        write(&dir, "app/db/__init__.py", "");
        write(&dir, "main.py", "from app.db import session");
        let e = PythonExtractor::new(dir.path());

        let resolution = e.resolve("app.db", &root(&dir).join("main.py"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("app/db/__init__.py")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn the_standard_library_and_dependencies_reduce_to_their_package_name() {
        let dir = TempDir::new().unwrap();
        write(&dir, "main.py", "");
        let e = PythonExtractor::new(dir.path());
        assert_eq!(
            e.resolve("os.path", &root(&dir).join("main.py")),
            Resolution::External("os".into())
        );
        assert_eq!(
            e.resolve("django.db.models", &root(&dir).join("main.py")),
            Resolution::External("django".into()),
            "one dependency, not three"
        );
    }

    #[test]
    fn a_type_stub_beside_a_module_is_a_valid_target() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/types.pyi", "");
        write(&dir, "app/main.py", "");
        let e = PythonExtractor::new(dir.path());
        assert!(matches!(
            e.resolve(".types", &root(&dir).join("app/main.py")),
            Resolution::Internal(_)
        ));
    }
}
