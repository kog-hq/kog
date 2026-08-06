//! C and C++: the preprocessor's `#include`, which is a file path and
//! nothing else.
//!
//! `#include "foo.h"` is resolved relative to the including file first —
//! that is what every compiler does — then against the project's include
//! directories. `#include <foo.h>` asks for a system or dependency header,
//! so it is only searched inside the project (many codebases include their
//! own public headers in angle brackets) and otherwise counted as external.
//!
//! One grammar covers both languages: C++ is a superset for preprocessor
//! syntax, and the include directive parses identically in either. The
//! per-file language label still distinguishes them, so each publishes its
//! own rate.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::path::{Path, PathBuf};

const IMPORT_QUERY: &str = r#"
(preproc_include path: (string_literal) @quoted)
(preproc_include path: (system_lib_string) @system)
"#;

const EXTENSIONS: &[&str] = &[
    "c", "h", "cc", "cpp", "cxx", "hh", "hpp", "hxx", "ipp", "inl",
];

/// Directories a C or C++ project conventionally puts its public headers
/// in, tried after the including file's own directory.
const INCLUDE_DIRS: &[&str] = &["include", "inc", "src", "lib"];

/// Marks a specifier that was written in angle brackets. Carried in the
/// text so `resolve` knows which of the two search rules applies, and so
/// the diagnostic shows the include exactly as the author wrote it.
const SYSTEM_PREFIX: &str = "<";

pub struct CLikeExtractor {
    root: PathBuf,
}

impl CLikeExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    /// Every directory an include is searched in, nearest first.
    fn search_paths(&self, importer: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut current = importer.parent();
        while let Some(dir) = current {
            if !support::contained(&self.root, dir) {
                break;
            }
            paths.push(dir.to_path_buf());
            if dir == self.root {
                break;
            }
            current = dir.parent();
        }
        for name in INCLUDE_DIRS {
            paths.push(self.root.join(name));
        }
        paths.push(self.root.clone());
        paths.dedup();
        paths
    }
}

impl Extractor for CLikeExtractor {
    fn lang(&self) -> &'static str {
        "c"
    }

    /// `.h` is claimed by C: a header shared between the two is more often
    /// C than C++, and the alternative — deciding by what includes it —
    /// would make one file's language depend on another's.
    fn lang_for(&self, path: &Path) -> &'static str {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "c" | "h" => "c",
            _ => "cpp",
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_cpp::LANGUAGE.into();
        let captures = support::captures(&language, "cpp", IMPORT_QUERY, source)?;
        Ok(captures
            .into_iter()
            .filter_map(|capture| {
                let raw = support::unquote(&capture.text).trim().to_string();
                if raw.is_empty() {
                    return None;
                }
                let raw = if capture.name == "system" {
                    format!("{SYSTEM_PREFIX}{raw}")
                } else {
                    raw
                };
                Some(Specifier {
                    raw,
                    line: capture.line,
                })
            })
            .collect())
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        let (path, is_system) = match raw.strip_prefix(SYSTEM_PREFIX) {
            Some(rest) => (rest, true),
            None => (raw, false),
        };

        for base in self.search_paths(importer) {
            let candidate = support::normalise(&base.join(path));
            if !support::contained(&self.root, &candidate) {
                continue;
            }
            if candidate.is_file() {
                return Resolution::Internal(candidate);
            }
        }

        if is_system {
            // A system or dependency header: no file of ours to point at,
            // so it leaves the denominator like any external import.
            return Resolution::External(path.to_string());
        }
        // A quoted include names a file the author expected to find beside
        // their own source. Not finding it is a genuine miss.
        Resolution::Unresolved
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
    fn both_include_forms_are_extracted_and_told_apart() {
        let dir = TempDir::new().unwrap();
        let e = CLikeExtractor::new(dir.path());
        let specifiers = e
            .extract("#include <stdio.h>\n#include \"local.h\"\n")
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["<stdio.h", "local.h"]
                .as_slice()
                .to_vec()
                .as_slice()
                .to_vec()
        );
        assert_eq!(specifiers[1].line, 2);
    }

    #[test]
    fn a_quoted_include_resolves_beside_the_including_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.c", "#include \"util.h\"");
        write(&dir, "src/util.h", "");
        let e = CLikeExtractor::new(dir.path());

        let resolution = e.resolve("util.h", &root(&dir).join("src/main.c"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/util.h")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn an_include_resolves_through_the_projects_include_directory() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.c", "");
        write(&dir, "include/app/config.h", "");
        let e = CLikeExtractor::new(dir.path());

        let resolution = e.resolve("app/config.h", &root(&dir).join("src/main.c"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("include/app/config.h")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_projects_own_header_included_in_angle_brackets_still_resolves() {
        // Common in libraries: the public header is included the same way a
        // consumer would include it. Treating every `<…>` as external would
        // drop a real edge.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.cpp", "");
        write(&dir, "include/mylib/api.hpp", "");
        let e = CLikeExtractor::new(dir.path());

        let resolution = e.resolve("<mylib/api.hpp", &root(&dir).join("src/main.cpp"));
        assert!(
            matches!(resolution, Resolution::Internal(_)),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_system_header_is_external_and_a_missing_local_one_is_unresolved() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.c", "");
        let e = CLikeExtractor::new(dir.path());
        let importer = root(&dir).join("src/main.c");

        assert_eq!(
            e.resolve("<stdio.h", &importer),
            Resolution::External("stdio.h".into())
        );
        assert_eq!(e.resolve("ghost.h", &importer), Resolution::Unresolved);
    }

    #[test]
    fn an_include_climbing_out_of_the_scan_root_is_never_followed() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/main.c", "");
        let e = CLikeExtractor::new(dir.path());
        assert_eq!(
            e.resolve("../../../etc/passwd", &root(&dir).join("src/main.c")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn the_language_label_distinguishes_c_from_cpp() {
        let dir = TempDir::new().unwrap();
        let e = CLikeExtractor::new(dir.path());
        assert_eq!(e.lang_for(Path::new("a.c")), "c");
        assert_eq!(e.lang_for(Path::new("a.h")), "c");
        assert_eq!(e.lang_for(Path::new("a.cpp")), "cpp");
        assert_eq!(e.lang_for(Path::new("a.hpp")), "cpp");
    }
}
