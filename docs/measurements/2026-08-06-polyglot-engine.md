# Measurement — the polyglot engine, on two public repositories

Measured on 2026-08-06 with the multi-language engine, on the same two
repositories as the v0 gate so the numbers are comparable. Both are public and
pinned to a commit you can check out.

| Repository | Commit |
| --- | --- |
| [`documenso/documenso`](https://github.com/documenso/documenso) | `f0ab7c112e3c39656b0153b67fbf25fd9616e96f` |
| [`TanStack/query`](https://github.com/TanStack/query) | `46d7f02f1c7b9fcd3255082cc7103e8bfa3dab76` |

Reproduce:

```bash
git clone --depth 1 https://github.com/documenso/documenso
kog scan documenso -o documenso.json
```

Everything below is derived from that JSON with `jq`. No step of this document
required looking at the filesystem by hand — that was the v0.1 requirement, and
it is what `stats.diagnostics[].reason` and `stats.coverage` are for.

---

## Two numbers, not one

A resolution rate answers *of the imports I read, how many resolved?* It says
nothing about the files never read at all. A tool that supports one language and
silently drops the other nine scores 1.0000 on a polyglot repository.

So every scan now publishes both:

- **resolution rate** — `resolved / (internal − excluded)`, unchanged from v0
- **source coverage** — `analysed / (analysed + unsupported)`: of the files that
  are source code, how many did an extractor actually read? Documentation, data
  and images leave the denominator, exactly as external specifiers leave the
  rate's.

| Repository | Files seen | Analysed | Not read | Not source | Source coverage | Resolution rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| documenso | 2,833 | 2,100 | 307 | 426 | **0.8725** | **0.9779** |
| TanStack/query | 2,314 | 1,276 | 0 | 1,038 | **1.0000** | **0.9926** |

Every file the walker visited is in exactly one of those columns, and every file
is a node in the graph — including the 426 and 1,038 that are not source at all.
An image and a PDF are never opened; they are measured in bytes and placed.

---

## Per language

A language ships when it passes its own gate, so each publishes its own rate. An
aggregate lets a broken resolver hide behind a majority language that works.

### documenso

| Language | Files | Internal | Resolved | Unresolved | Rate | Edges |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| typescript | 2,071 | 10,343 | 10,117 | 226 | 0.9781 | 9,986 |
| javascript | 13 | 3 | 0 | 3 | **0.0000** | 0 |
| shell | 11 | 0 | 0 | 0 | 1.0000 | 0 |
| css | 5 | 0 | 0 | 0 | 1.0000 | 0 |

JavaScript's 0.0000 is three specifiers in one file,
`apps/remix/server/main.js`, which opens with:

> This file will be copied to the build folder during build time. Running this
> file will not work without a build.

Its three relative imports are relative to the *build* directory, not to the
source tree. They are unresolvable from a fresh clone, and the file says so
itself. Published as 0.0000 rather than hidden inside a 0.9779 average — with 3
in the denominator, the number is the honest one.

### TanStack/query

| Language | Files | Internal | Resolved | Unresolved | Excluded | Rate | Edges |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| typescript | 943 | 1,554 | 1,534 | 8 | 12 | 0.9948 | 1,381 |
| javascript | 138 | 40 | 14 | 1 | 25 | 0.9333 | 14 |
| svelte | 77 | 93 | 93 | 0 | 0 | 1.0000 | 85 |
| html | 77 | 72 | 68 | 4 | 0 | 0.9444 | 68 |
| vue | 20 | 22 | 22 | 0 | 0 | 1.0000 | 22 |
| css | 19 | 0 | 0 | 0 | 0 | 1.0000 | 0 |
| astro | 2 | 2 | 2 | 0 | 0 | 1.0000 | 2 |

---

## What was not read, and why

`stats.coverage.extensions` names every extension with no extractor, with the
language it belongs to. On documenso:

| | Files | Language |
| --- | ---: | --- |
| `.sql` | 163 | SQL |
| `.mdx` | 143 | MDX |
| `Dockerfile` | 1 | Dockerfile |

TanStack/query has none: every source file in it is in a language KOG reads.

```bash
jq '.projects[0].graph.stats.coverage.extensions[]
    | select(.status == "unsupported_language")' documenso.json
```

---

## Why each specifier did not become an edge

Every diagnostic carries a machine-readable `reason`, so the categorisation
below is a `group_by`, not an afternoon of filesystem archaeology.

```bash
jq '[.projects[0].graph.stats.diagnostics[].reason] | group_by(.)
    | map({reason: .[0], count: length})' documenso.json
```

| Repository | `not_found` | `gitignored` | Total |
| --- | ---: | ---: | ---: |
| documenso | 229 | 0 | 229 |
| TanStack/query | 13 | 37 | 50 |

documenso's 229 are all `not_found`, and 225 of them are imports into code that
`prisma generate` and `react-router typegen` produce — absent from a fresh
clone, present after running those generators. TanStack/query's 37 `gitignored`
are imports into build output that exists on disk but is excluded by
`.gitignore`; they are counted `excluded`, so they leave the rate's denominator
rather than depressing it.

---

## Two defects this measurement found

Both were invisible before per-language rates existed, and both are fixed with
a test:

1. **`.d.ts` was not probed.** TanStack/query's Vue examples import `./types`,
   which is `types.d.ts` on disk. Ten real imports were reported broken; Vue's
   rate was 0.5455. A declaration file is now the last extension probed — last,
   so a real `types.ts` beside it still wins.
2. **Build-time template placeholders were probed as paths.** SvelteKit writes
   `%sveltekit.assets%/favicon.png` in its HTML; no file of that name has ever
   existed. HTML's rate was 0.8608.

After both: Vue 0.5455 → **1.0000**, HTML 0.8608 → **0.9444**, and the
repository's rate 0.9829 → **0.9926**.

That is the argument for publishing a rate per language rather than one number:
the one number moved by less than a point, and hid two real bugs.
