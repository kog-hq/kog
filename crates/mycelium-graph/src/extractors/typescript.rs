use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::tsconfig::TsConfigIndex;
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Captures the string literal of every static import and re-export.
/// Dynamic `import()` and `require()` are deliberately out of scope in v0.
const IMPORT_QUERY: &str = r#"
(import_statement source: (string) @spec)
(export_statement source: (string) @spec)
"#;

/// Extension probes, in order, for a specifier that has none.
const EXTENSION_ORDER: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts"];

pub struct TypeScriptExtractor {
    root: PathBuf,
    tsconfig: TsConfigIndex,
}

impl TypeScriptExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            tsconfig: TsConfigIndex::build(root),
        }
    }

    /// Probe a resolution candidate: exact file, then each extension,
    /// then the directory's index file.
    fn probe(&self, candidate: &Path) -> Option<PathBuf> {
        if candidate.is_file() {
            return Some(candidate.to_path_buf());
        }

        // `./b.js` is written by ESM/NodeNext style but `./b.ts` is on disk.
        if let Some(stem) = candidate
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| matches!(*e, "js" | "jsx" | "mjs" | "cjs"))
            .map(|_| candidate.with_extension(""))
        {
            for ext in EXTENSION_ORDER {
                let probed = stem.with_extension(ext);
                if probed.is_file() {
                    return Some(probed);
                }
            }
        }

        for ext in EXTENSION_ORDER {
            let mut probed = candidate.to_path_buf();
            let name = probed.file_name()?.to_str()?.to_string();
            probed.set_file_name(format!("{name}.{ext}"));
            if probed.is_file() {
                return Some(probed);
            }
        }

        if candidate.is_dir() {
            for ext in EXTENSION_ORDER {
                let probed = candidate.join(format!("index.{ext}"));
                if probed.is_file() {
                    return Some(probed);
                }
            }
        }

        None
    }

    /// Expand one alias mapping against a specifier, if it matches.
    fn expand(pattern: &str, target: &Path, raw: &str) -> Option<PathBuf> {
        match pattern.split_once('*') {
            // Wildcard mapping: `@/*` -> `./src/*`.
            Some((prefix, suffix)) => {
                let rest = raw.strip_prefix(prefix)?.strip_suffix(suffix)?;
                let target_str = target.to_str()?;
                Some(PathBuf::from(target_str.replace('*', rest)))
            }
            // Exact mapping: `@mastore/shared-types` -> one file.
            None if pattern == raw => Some(target.to_path_buf()),
            None => None,
        }
    }
}

impl Extractor for TypeScriptExtractor {
    fn lang(&self) -> &'static str {
        "typescript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx", "mts", "cts"]
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        // TSX is a superset for import syntax, so one grammar covers both.
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|_| ExtractError::Grammar("typescript"))?;
        let tree = parser.parse(source, None).ok_or(ExtractError::Parse)?;

        let query =
            Query::new(&language, IMPORT_QUERY).map_err(|_| ExtractError::Grammar("typescript"))?;

        let mut cursor = QueryCursor::new();
        // `matches` is a StreamingIterator: a `for` loop does not compile.
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        let mut out = Vec::new();
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let text = &source[capture.node.byte_range()];
                let raw = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');
                if raw.is_empty() {
                    continue;
                }
                out.push(Specifier {
                    raw: raw.to_string(),
                    line: capture.node.start_position().row + 1,
                });
            }
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        // 1. Relative.
        if raw.starts_with('.') {
            let base = match importer.parent() {
                Some(d) => d,
                None => return Resolution::Unresolved,
            };
            let candidate = crate::tsconfig::normalise_public(&base.join(raw));
            return match self.probe(&candidate) {
                Some(path) => Resolution::Internal(path),
                None => Resolution::Unresolved,
            };
        }

        // 2. tsconfig alias. A matching pattern means the author meant an
        //    internal file, so a miss is Unresolved, never External.
        let mut matched_an_alias = false;
        for mapping in self.tsconfig.mappings_for(importer) {
            for target in &mapping.targets {
                if let Some(candidate) = Self::expand(&mapping.pattern, target, raw) {
                    matched_an_alias = true;
                    if let Some(path) = self.probe(&candidate) {
                        return Resolution::Internal(path);
                    }
                }
            }
        }
        if matched_an_alias {
            return Resolution::Unresolved;
        }

        // 3. Anything else is a third-party package.
        let _ = &self.root;
        Resolution::External(raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn raws(source: &str) -> Vec<String> {
        let dir = TempDir::new().unwrap();
        TypeScriptExtractor::new(dir.path())
            .extract(source)
            .unwrap()
            .into_iter()
            .map(|s| s.raw)
            .collect()
    }

    #[test]
    fn plain_imports_are_extracted() {
        let got = raws(r#"import React from "react";"#);
        assert_eq!(got, vec!["react"]);
    }

    #[test]
    fn type_only_imports_are_extracted_too() {
        let got = raws(r#"import type { T } from "@/types/user";"#);
        assert_eq!(got, vec!["@/types/user"]);
    }

    #[test]
    fn re_exports_are_extracted() {
        let got = raws(
            r#"export { x } from "@common/utils";
export * from "./barrel";"#,
        );
        assert_eq!(got, vec!["@common/utils", "./barrel"]);
    }

    #[test]
    fn single_quotes_are_handled() {
        let got = raws("import a from './local';");
        assert_eq!(got, vec!["./local"]);
    }

    #[test]
    fn dynamic_imports_are_out_of_scope_in_v0() {
        let got = raws(r#"const m = await import("./dynamic");"#);
        assert!(got.is_empty());
    }

    #[test]
    fn a_syntactically_broken_file_still_yields_what_it_can() {
        // tree-sitter recovers from errors; extraction must not blow up.
        let got = raws(r#"import a from "./ok"; function ( { unclosed"#);
        assert_eq!(got, vec!["./ok"]);
    }

    #[test]
    fn the_line_number_is_one_based() {
        let dir = TempDir::new().unwrap();
        let specs = TypeScriptExtractor::new(dir.path())
            .extract("\n\nimport a from \"./x\";")
            .unwrap();
        assert_eq!(specs[0].line, 3);
    }

    #[test]
    fn a_relative_import_resolves_next_to_the_importer() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("./b", &dir.path().join("src/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/b.ts")));
    }

    #[test]
    fn a_parent_relative_import_resolves() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/deep/a.ts", "");
        write(&dir, "src/b.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("../b", &dir.path().join("src/deep/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/b.ts")));
    }

    #[test]
    fn a_directory_import_falls_back_to_index() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/lib/index.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("./lib", &dir.path().join("src/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("src/lib/index.ts"))
        );
    }

    #[test]
    fn a_js_specifier_falls_back_to_the_ts_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("./b.js", &dir.path().join("src/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/b.ts")));
    }

    #[test]
    fn a_wildcard_alias_resolves_through_tsconfig() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        write(&dir, "src/lib/api.ts", "");
        write(&dir, "src/app/page.tsx", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("@/lib/api", &dir.path().join("src/app/page.tsx"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/lib/api.ts")));
    }

    #[test]
    fn an_exact_alias_resolves_to_its_single_target() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": {
                "@mastore/shared-types": ["./packages/shared-types/src/index.ts"]
            } } }"#,
        );
        write(&dir, "packages/shared-types/src/index.ts", "");
        write(&dir, "apps/web/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/src/index.ts"))
        );
    }

    #[test]
    fn a_bare_package_name_is_external() {
        let dir = TempDir::new().unwrap();
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("react", &dir.path().join("a.ts")),
            Resolution::External("react".into())
        );
        assert_eq!(
            e.resolve("@nestjs/common", &dir.path().join("a.ts")),
            Resolution::External("@nestjs/common".into())
        );
    }

    #[test]
    fn an_alias_whose_target_is_missing_is_unresolved_not_external() {
        // Mirrors `@prisma/generated` on the reference monorepo: the mapping
        // exists but the generated directory has never been built.
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": {
                "@prisma/generated": ["./src/generated/prisma/browser"]
            } } }"#,
        );
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("@prisma/generated", &dir.path().join("src/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn a_relative_import_pointing_nowhere_is_unresolved() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("./ghost", &dir.path().join("src/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn an_asset_import_is_unresolved_and_counted() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("./logo.png", &dir.path().join("src/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn the_extractor_declares_its_language_and_extensions() {
        let dir = TempDir::new().unwrap();
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(e.lang(), "typescript");
        assert_eq!(e.extensions(), &["ts", "tsx", "js", "jsx", "mts", "cts"]);
    }
}
