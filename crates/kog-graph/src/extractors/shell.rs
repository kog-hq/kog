//! Shell: `source` and `.`, the only two constructs that pull one script
//! into another.
//!
//! A shell script's path is resolved at run time against the caller's
//! working directory, which no static tool can know. What every real script
//! does instead is anchor the path to its own location —
//! `source "$(dirname "$0")/lib/util.sh"` — and that idiom is exactly what
//! this resolver understands: everything up to the last closing `)` or `}`
//! of a command or parameter substitution is dropped, and what remains is
//! resolved relative to the sourcing script.
//!
//! A path that is still not anchored (`source lib/util.sh`) is tried
//! against the script's own directory and then the project root, in that
//! order. Anything left is honestly unresolved rather than guessed at.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::path::{Path, PathBuf};

const IMPORT_QUERY: &str = r#"
(command name: (command_name) @name argument: (_) @arg)
"#;

const EXTENSIONS: &[&str] = &["sh", "bash", "zsh", "ksh"];

pub struct ShellExtractor {
    root: PathBuf,
}

impl ShellExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    /// Strip the leading substitution that anchors a path to the script's
    /// own directory. Returns the path as written when there is none.
    fn strip_anchor(raw: &str) -> &str {
        // The anchor always ends in `)` or `}` immediately followed by `/`:
        // `$(dirname "$0")/`, `${BASH_SOURCE%/*}/`, `$(cd "$(dirname …)")/`.
        match raw.rfind(")/").or_else(|| raw.rfind("}/")) {
            Some(index) => &raw[index + 2..],
            None => raw,
        }
    }

    /// Whether what remains still contains a shell expansion, in which case
    /// no static path can be derived from it and probing would be theatre.
    fn is_dynamic(path: &str) -> bool {
        path.contains('$') || path.contains('*') || path.contains('`')
    }
}

impl Extractor for ShellExtractor {
    fn lang(&self) -> &'static str {
        "shell"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_bash::LANGUAGE.into();
        let groups = support::grouped_captures(&language, "bash", IMPORT_QUERY, source)?;

        let mut out = Vec::new();
        for group in groups {
            let (Some(name), Some(arg)) = (
                support::named(&group, "name"),
                support::named(&group, "arg"),
            ) else {
                continue;
            };
            if name.text != "source" && name.text != "." {
                continue;
            }
            let raw = support::unquote(arg.text.trim()).trim().to_string();
            if raw.is_empty() {
                continue;
            }
            out.push(Specifier {
                raw,
                line: arg.line,
            });
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        let path = Self::strip_anchor(raw);
        if path.is_empty() || Self::is_dynamic(path) {
            // A path assembled at run time from a variable. Reporting it as
            // a broken import would be wrong — it is not broken, it is
            // undecidable — so it leaves the denominator like an external.
            return Resolution::External(raw.to_string());
        }

        let bases = [
            importer.parent().map(Path::to_path_buf),
            Some(self.root.clone()),
        ];
        for base in bases.into_iter().flatten() {
            let candidate = support::normalise(&base.join(path.trim_start_matches("./")));
            if !support::contained(&self.root, &candidate) {
                continue;
            }
            if candidate.is_file() {
                return Resolution::Internal(candidate);
            }
        }
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
    fn source_and_dot_are_both_extracted_and_nothing_else_is() {
        let dir = TempDir::new().unwrap();
        let e = ShellExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "#!/bin/bash\nsource ./lib/util.sh\n. ./lib/colours.sh\necho ./not-an-import.sh\ncat ./file.sh\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["./lib/util.sh", "./lib/colours.sh"]);
        assert_eq!(specifiers[0].line, 2);
    }

    #[test]
    fn a_relative_source_resolves_beside_the_sourcing_script() {
        let dir = TempDir::new().unwrap();
        write(&dir, "scripts/deploy.sh", "source ./lib/util.sh");
        write(&dir, "scripts/lib/util.sh", "");
        let e = ShellExtractor::new(dir.path());

        let resolution = e.resolve("./lib/util.sh", &root(&dir).join("scripts/deploy.sh"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("scripts/lib/util.sh")),
            "got {resolution:?}"
        );
    }

    /// The idiom every real script uses. Without this rule, a shell
    /// codebase resolves at close to zero.
    #[test]
    fn the_dirname_anchor_idiom_resolves() {
        let dir = TempDir::new().unwrap();
        write(&dir, "scripts/deploy.sh", "");
        write(&dir, "scripts/lib/util.sh", "");
        let e = ShellExtractor::new(dir.path());
        let importer = root(&dir).join("scripts/deploy.sh");

        for raw in [
            "$(dirname \"$0\")/lib/util.sh",
            "${BASH_SOURCE%/*}/lib/util.sh",
            "$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)/lib/util.sh",
        ] {
            let resolution = e.resolve(raw, &importer);
            assert!(
                matches!(resolution, Resolution::Internal(ref p) if p.ends_with("lib/util.sh")),
                "{raw} should resolve, got {resolution:?}"
            );
        }
    }

    #[test]
    fn a_path_that_is_still_a_variable_is_external_not_broken() {
        // `source "$CONFIG_FILE"` is not a broken import; it is one no
        // static resolver can decide. Counting it against the rate would
        // punish the tool for the language's semantics.
        let dir = TempDir::new().unwrap();
        write(&dir, "run.sh", "");
        let e = ShellExtractor::new(dir.path());
        assert!(matches!(
            e.resolve("$CONFIG_FILE", &root(&dir).join("run.sh")),
            Resolution::External(_)
        ));
    }

    #[test]
    fn a_missing_script_is_unresolved() {
        let dir = TempDir::new().unwrap();
        write(&dir, "run.sh", "");
        let e = ShellExtractor::new(dir.path());
        assert_eq!(
            e.resolve("./ghost.sh", &root(&dir).join("run.sh")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn a_source_climbing_out_of_the_scan_root_is_never_followed() {
        let dir = TempDir::new().unwrap();
        write(&dir, "run.sh", "");
        let e = ShellExtractor::new(dir.path());
        assert_eq!(
            e.resolve("../../../etc/profile", &root(&dir).join("run.sh")),
            Resolution::Unresolved
        );
    }
}
