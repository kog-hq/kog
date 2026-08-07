//! Single-file components: Vue, Svelte and Astro.
//!
//! A `.vue` or `.svelte` file is not one language — it is a template, a
//! stylesheet and a script sharing a file. Only the script part imports
//! anything, and it is TypeScript (or JavaScript, which the same grammar
//! and the same resolver already cover correctly).
//!
//! So this extractor does one thing: find the script regions, hand them to
//! the TypeScript front end, and shift the line numbers back so a
//! diagnostic points at the real line in the real file. Resolution is
//! delegated wholesale — a component importing `@/lib/api` must go through
//! the same tsconfig aliases and workspace packages as every other file in
//! the project, and reimplementing that here would be a second resolver to
//! keep in step with the first.
//!
//! Astro puts its script in `---` frontmatter at the top of the file
//! instead of a `<script>` tag; both shapes are handled.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::TypeScriptExtractor;
use std::path::Path;
use std::sync::Arc;

const EXTENSIONS: &[&str] = &["vue", "svelte", "astro"];

pub struct SfcExtractor {
    /// Shared with the registry's TypeScript extractor rather than built
    /// again: constructing one means walking the project for every
    /// `tsconfig` and workspace package, and two copies could drift.
    typescript: Arc<TypeScriptExtractor>,
}

impl SfcExtractor {
    pub fn new(typescript: Arc<TypeScriptExtractor>) -> Self {
        Self { typescript }
    }

    /// The script regions of a component, each with the zero-based line the
    /// region starts on so extracted line numbers can be shifted back.
    fn script_regions(source: &str) -> Vec<(usize, &str)> {
        let mut regions = Vec::new();

        // Astro frontmatter: a `---` fence as the very first thing in the
        // file, closed by another `---` on its own line.
        if let Some(rest) = source.strip_prefix("---") {
            if let Some(end) = rest.find("\n---") {
                // The body starts on the same line as the closing `---` of
                // the opening fence, i.e. at line 1 of the file: its own
                // first line is the newline right after `---`, so no shift
                // is needed.
                regions.push((0, &rest[..end]));
            }
        }

        let bytes = source.as_bytes();
        let mut cursor = 0usize;
        while let Some(found) = source[cursor..].find("<script") {
            let tag_start = cursor + found;
            // `<scriptsomething` is not a script tag.
            let after = bytes.get(tag_start + 7).copied().unwrap_or(b'>');
            if !after.is_ascii_whitespace() && after != b'>' {
                cursor = tag_start + 7;
                continue;
            }
            let Some(open_end) = source[tag_start..].find('>').map(|i| tag_start + i + 1) else {
                break;
            };
            let Some(close) = source[open_end..].find("</script>").map(|i| open_end + i) else {
                break;
            };
            regions.push((
                source[..open_end].lines().count() - 1,
                &source[open_end..close],
            ));
            cursor = close + "</script>".len();
        }

        regions
    }
}

impl Extractor for SfcExtractor {
    fn lang(&self) -> &'static str {
        "vue"
    }

    fn lang_for(&self, path: &Path) -> &'static str {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "svelte" => "svelte",
            "astro" => "astro",
            _ => "vue",
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let mut out = Vec::new();
        for (offset, region) in Self::script_regions(source) {
            for mut specifier in self.typescript.extract(region)? {
                // `extract` numbers lines from 1 within the region; the
                // region itself starts at `offset` lines into the file.
                specifier.line += offset;
                out.push(specifier);
            }
        }
        Ok(out)
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

    fn extractor(dir: &TempDir) -> SfcExtractor {
        SfcExtractor::new(Arc::new(TypeScriptExtractor::new(dir.path())))
    }

    #[test]
    fn a_vue_components_script_imports_are_extracted_at_their_real_line() {
        let dir = TempDir::new().unwrap();
        let e = extractor(&dir);
        let specifiers = e
            .extract(
                r#"<template>
  <div>{{ title }}</div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import Button from "./Button.vue";
</script>

<style scoped></style>
"#,
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["vue", "./Button.vue"]);
        assert_eq!(
            specifiers[1].line, 7,
            "a diagnostic must point at the line in the component, not in the script block"
        );
    }

    #[test]
    fn a_svelte_component_with_two_script_blocks_yields_both() {
        let dir = TempDir::new().unwrap();
        let e = extractor(&dir);
        let specifiers = e
            .extract(
                r#"<script context="module">
import { load } from "./loader";
</script>

<script>
import Child from "./Child.svelte";
</script>

<h1>hello</h1>
"#,
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["./loader", "./Child.svelte"]);
    }

    #[test]
    fn astro_frontmatter_is_treated_as_the_script() {
        let dir = TempDir::new().unwrap();
        let e = extractor(&dir);
        let specifiers = e
            .extract(
                r#"---
import Layout from "../layouts/Base.astro";
---

<Layout>hello</Layout>
"#,
            )
            .unwrap();
        assert_eq!(specifiers.len(), 1);
        assert_eq!(specifiers[0].raw, "../layouts/Base.astro");
        assert_eq!(specifiers[0].line, 2);
    }

    #[test]
    fn a_component_with_no_script_yields_nothing_rather_than_failing() {
        let dir = TempDir::new().unwrap();
        let e = extractor(&dir);
        assert!(e
            .extract("<template><p>static</p></template>")
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
        write(&dir, "src/components/Card.vue", "");
        write(&dir, "src/lib/api.ts", "");
        let e = extractor(&dir);

        let resolution = e.resolve("@/lib/api", &root(&dir).join("src/components/Card.vue"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/lib/api.ts")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn each_component_dialect_is_labelled_by_its_own_name() {
        let dir = TempDir::new().unwrap();
        let e = extractor(&dir);
        assert_eq!(e.lang_for(Path::new("a.vue")), "vue");
        assert_eq!(e.lang_for(Path::new("a.svelte")), "svelte");
        assert_eq!(e.lang_for(Path::new("a.astro")), "astro");
    }
}
