//! MDX: Markdown with real ESM imports in it.
//!
//! An `.mdx` file is prose, JSX and ESM sharing a document. Only the ESM
//! part imports anything, and it is the same module system, resolved the
//! same way, as every `.ts` file in the project — so resolution is delegated
//! wholesale to the TypeScript front end, exactly as [`super::SfcExtractor`]
//! does for Vue and Svelte. A second resolver would be a second thing to
//! keep in step with tsconfig aliases and workspace packages.
//!
//! ## The one thing this file exists to get right
//!
//! Documentation is full of code that is *shown*, not run:
//!
//! ~~~markdown
//! Install it, then:
//!
//! ```ts
//! import { readFile } from "fs";
//! import { helper } from "./helper";
//! ```
//! ~~~
//!
//! Neither of those is an import of the document. Handing the raw file to a
//! TypeScript parser reports both: the first invents a dependency on `fs`
//! that the page does not have, and the second — a relative specifier that
//! resolves to nothing, because the sample is illustrative — is counted
//! `unresolved` and **depresses the published resolution rate with fiction**.
//! That is the worst defect available here: a number that is unattackable
//! and wrong.
//!
//! Measured on [documenso](https://github.com/documenso/documenso) at the
//! commit in `docs/measurements/`: of the 300 specifiers written inside its
//! 144 `.mdx` files, roughly 15 (`fs`, `crypto`, `express`, `form-data`,
//! `express-rate-limit`, `@angular/core`) appear only inside fenced samples.
//!
//! So the parser is handed the ESM statements and nothing else — everything
//! around them blanked, not removed, so every line number a diagnostic
//! reports still points at the real line in the real file.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::TypeScriptExtractor;
use std::path::Path;
use std::sync::Arc;

const EXTENSIONS: &[&str] = &["mdx"];

pub struct MdxExtractor {
    /// Shared with the registry's TypeScript extractor rather than built
    /// again: constructing one means walking the project for every
    /// `tsconfig` and workspace package, and two copies could drift.
    typescript: Arc<TypeScriptExtractor>,
}

/// A fence opener: its character, its length, and the indent it sits at.
struct Fence {
    marker: u8,
    length: usize,
}

/// Read a fence opener or closer out of a line, if it is one.
///
/// CommonMark allows up to three spaces of indent and a run of at least
/// three backticks or tildes.
fn fence_of(line: &str) -> Option<Fence> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = match trimmed.as_bytes().first() {
        Some(&b'`') => b'`',
        Some(&b'~') => b'~',
        _ => return None,
    };
    let length = trimmed.bytes().take_while(|b| *b == marker).count();
    (length >= 3).then_some(Fence { marker, length })
}

/// A statement that never ends is a bug somewhere; stop copying rather than
/// swallow the document.
const MAX_STATEMENT_LINES: usize = 24;

/// Whether a line opens a top-level ESM statement.
///
/// Column zero is the whole test, and it does more work than it looks like.
/// MDX only treats `import`/`export` as ESM when they start a top-level
/// block, so an indented line is prose or data by definition — which is also
/// what keeps an inline `` `import x from "./y"` `` in the middle of a
/// sentence from ever being read as an import.
fn opens_statement(line: &str) -> bool {
    let rest = line
        .strip_prefix("import")
        .or_else(|| line.strip_prefix("export"));
    rest.is_some_and(|rest| rest.starts_with([' ', '\t', '{', '*', '"', '\'']) || rest.is_empty())
}

/// Keep the ESM statements, blank everything else.
///
/// The first version of this handed the whole document to the TSX grammar
/// with only the code fences blanked, on the assumption that tree-sitter
/// would recover around the Markdown. It does not, reliably: a line of prose
/// — or a YAML frontmatter block — produces an error node that swallows the
/// `import` immediately after it, so real imports went missing depending on
/// what was written above them. Handing the parser nothing but ESM removes
/// the question.
///
/// Every line's length and position is preserved rather than removed, so a
/// diagnostic still points at the line the specifier was written on.
fn esm_only(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut fence: Option<Fence> = None;
    let mut statement: Option<usize> = None;
    let mut depth = 0i32;

    for (index, line) in source.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }

        // Inside a fence, everything is a sample: code that is shown, not
        // run. This is the case the whole module exists for.
        if let Some(open) = &fence {
            let closes = fence_of(line).is_some_and(|f| {
                f.marker == open.marker
                    && f.length >= open.length
                    && line.trim().bytes().all(|b| b == open.marker)
            });
            blank_line(&mut out, line);
            if closes {
                fence = None;
            }
            continue;
        }
        if statement.is_none() {
            if let Some(open) = fence_of(line) {
                fence = Some(open);
                blank_line(&mut out, line);
                continue;
            }
        }

        let carrying = match statement {
            Some(started) => index - started < MAX_STATEMENT_LINES,
            None => opens_statement(line),
        };
        if !carrying {
            statement = None;
            blank_line(&mut out, line);
            continue;
        }

        if statement.is_none() {
            statement = Some(index);
            depth = 0;
        }
        out.push_str(line);

        let (delta, quoted) = scan(line);
        depth += delta;
        // The statement is done once its brackets are balanced and the line
        // either carried the specifier or ended in a semicolon.
        if depth <= 0 && (quoted || line.trim_end().ends_with(';')) {
            statement = None;
            depth = 0;
        }
    }

    // `lines()` drops a trailing newline; put it back so the blanked source
    // is the same shape as the original.
    if source.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Bracket balance for one line, and whether it held a quoted string —
/// counting neither while inside a string, so `from "a{b"` stays balanced.
fn scan(line: &str) -> (i32, bool) {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for character in line.chars() {
        match quote {
            Some(open) => {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == open {
                    quote = None;
                }
            }
            None => match character {
                '"' | '\'' | '`' => {
                    quote = Some(character);
                    quoted = true;
                }
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            },
        }
    }
    (depth, quoted)
}

/// Spaces, one per character, so line lengths and positions survive.
fn blank_line(out: &mut String, line: &str) {
    for _ in line.chars() {
        out.push(' ');
    }
}

impl MdxExtractor {
    pub fn new(typescript: Arc<TypeScriptExtractor>) -> Self {
        Self { typescript }
    }
}

impl Extractor for MdxExtractor {
    fn lang(&self) -> &'static str {
        "mdx"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        // The whole document, minus its samples, handed to the TSX grammar.
        // Prose cannot produce a false positive: the import query matches an
        // `import_statement` or `export_statement` with a string source, and
        // tree-sitter is fault-tolerant enough to recover around the
        // Markdown it does not understand.
        self.typescript.extract(&esm_only(source))
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        self.typescript.resolve(raw, importer)
    }

    fn skipped_configs(&self) -> &[crate::tsconfig::SkippedConfig] {
        // Reported once, by the TypeScript extractor that owns the index.
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn root(dir: &TempDir) -> PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = root(dir).join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn extractor(dir: &TempDir) -> MdxExtractor {
        MdxExtractor::new(Arc::new(TypeScriptExtractor::new(dir.path())))
    }

    fn raws(specifiers: &[Specifier]) -> Vec<&str> {
        specifiers.iter().map(|s| s.raw.as_str()).collect()
    }

    #[test]
    fn a_top_level_esm_import_is_extracted() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                r#"---
title: Guide
---

import { Callout } from "fumadocs-ui/components/callout";
import Local from "./local-component";

# Heading

<Callout>hello</Callout>
"#,
            )
            .unwrap();
        assert_eq!(
            raws(&specifiers),
            vec!["fumadocs-ui/components/callout", "./local-component"]
        );
        assert_eq!(
            specifiers[1].line, 6,
            "a diagnostic must point at the real line in the document"
        );
    }

    /// The reason this extractor is not three lines long. Both imports below
    /// are being *shown*, not performed: reporting them invents a dependency
    /// the page does not have, and the relative one would resolve to nothing
    /// and drag the published rate down with a sample.
    #[test]
    fn an_import_inside_a_fenced_sample_is_not_an_import_of_the_document() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                r#"import { Callout } from "fumadocs-ui/components/callout";

Read a file:

```ts
import { readFile } from "fs";
import { helper } from "./helper";
```

Done.
"#,
            )
            .unwrap();
        assert_eq!(raws(&specifiers), vec!["fumadocs-ui/components/callout"]);
    }

    #[test]
    fn a_tilde_fence_hides_code_the_same_way_a_backtick_fence_does() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                r#"~~~js
import express from "express";
~~~

import { Real } from "./real";
"#,
            )
            .unwrap();
        assert_eq!(raws(&specifiers), vec!["./real"]);
    }

    /// A fence closes only on its own marker, so a backtick fence containing
    /// tildes — or a longer fence containing a shorter one — stays open.
    #[test]
    fn a_fence_closes_only_on_its_own_marker() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                r#"````md
```
import { Nested } from "./nested";
```
````

import { Real } from "./real";
"#,
            )
            .unwrap();
        assert_eq!(
            raws(&specifiers),
            vec!["./real"],
            "a nested fence must not close the outer one"
        );
    }

    #[test]
    fn an_inline_code_span_is_prose_about_code_not_code() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                "Write `import x from \"./ghost\"` at the top.\n\nimport { Real } from \"./real\";\n",
            )
            .unwrap();
        assert_eq!(raws(&specifiers), vec!["./real"]);
    }

    #[test]
    fn an_unclosed_fence_swallows_the_rest_rather_than_leaking_samples() {
        // A document whose fence is never closed is malformed. Treating the
        // remainder as prose would report every sample in it as an import;
        // treating it as code loses at most some real imports, and loses them
        // in the direction that cannot invent a dependency.
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                "```ts\nimport { Sample } from \"./sample\";\n\nimport { Also } from \"./also\";\n",
            )
            .unwrap();
        assert!(raws(&specifiers).is_empty());
    }

    #[test]
    fn a_multi_line_import_survives_being_blanked_around() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                r#"import {
  Tab,
  Tabs,
} from "fumadocs-ui/components/tabs";
"#,
            )
            .unwrap();
        assert_eq!(raws(&specifiers), vec!["fumadocs-ui/components/tabs"]);
        assert_eq!(
            specifiers[0].line, 4,
            "the line reported is the one the specifier itself sits on, as \
             every other extractor reports it"
        );
    }

    /// The first version of this extractor lost the import directly below a
    /// frontmatter block: the TSX grammar turned `title: Guide` into an error
    /// node that swallowed the statement after it. Every documentation page
    /// in the acceptance repository opens with frontmatter, so this was not
    /// an edge case — it was most of them.
    #[test]
    fn an_import_below_yaml_frontmatter_is_not_swallowed_by_it() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract(
                r#"---
title: Guide
description: How to do the thing
---

import { First } from "./first";
"#,
            )
            .unwrap();
        assert_eq!(raws(&specifiers), vec!["./first"]);
        assert_eq!(specifiers[0].line, 6);
    }

    /// MDX only reads `import` as ESM when it opens a top-level block, so an
    /// indented one is prose or data. Reporting it would invent a dependency
    /// out of an example someone wrote in a list item.
    #[test]
    fn an_indented_import_is_not_a_top_level_statement() {
        let dir = TempDir::new().unwrap();
        let specifiers = extractor(&dir)
            .extract("1. First, write:\n\n    import x from \"./ghost\";\n\nimport { Real } from \"./real\";\n")
            .unwrap();
        assert_eq!(raws(&specifiers), vec!["./real"]);
    }

    #[test]
    fn a_document_with_no_imports_at_all_yields_nothing_rather_than_failing() {
        let dir = TempDir::new().unwrap();
        assert!(extractor(&dir)
            .extract("# Just a heading\n\nSome prose, and a [link](./other.mdx).\n")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn resolution_goes_through_the_typescript_resolver_aliases_and_all() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        write(&dir, "src/components/Callout.tsx", "");
        write(&dir, "docs/guide.mdx", "");
        let e = extractor(&dir);

        let resolution = e.resolve("@/components/Callout", &root(&dir).join("docs/guide.mdx"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/components/Callout.tsx")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn blanking_preserves_the_shape_of_the_document() {
        let source = "one\n```\ntwo\n```\nfour\n";
        let blanked = esm_only(source);
        assert_eq!(
            blanked.lines().count(),
            source.lines().count(),
            "every line must survive, or line numbers shift"
        );
        assert!(blanked.ends_with('\n'), "a trailing newline is preserved");
        for (before, after) in source.lines().zip(blanked.lines()) {
            assert_eq!(
                before.chars().count(),
                after.chars().count(),
                "line lengths must survive too"
            );
        }
    }
}
