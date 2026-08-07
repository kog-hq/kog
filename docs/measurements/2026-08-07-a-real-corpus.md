# Measurement — eleven repositories, and what measuring them found

Measured on 2026-08-07. Every repository is public and pinned to a commit you
can check out.

## Why this document exists

Until today KOG published its numbers on two repositories: `documenso` and
`TanStack/query`. Both are TypeScript monorepos. KOG claims **sixteen
languages**, so nine of them — Go, Python, Rust, C, C++, Java, C#, Ruby, PHP —
had never been run against real code in anything published.

That is a hole of exactly the shape this project exists to argue against. The
whole reason a rate is published *per language* is that an aggregate lets a
broken resolver hide behind a majority language that works. A language that has
never met real code does not have a rate at all; it has a passing unit test.

The roadmap's own rule is that **a language ships when it passes its own
resolution gate, not when its grammar compiles**. Nine languages had shipped
without one.

The first scan of this corpus found a resolver returning **0.1261**.

---

## The corpus

| Repository | Commit | Why it is here |
| --- | --- | --- |
| [`withastro/docs`](https://github.com/withastro/docs) | `61d9e861ef0023126d542a381ac01cb334979356` | Astro and MDX, a real documentation site |
| [`cli/cli`](https://github.com/cli/cli) | `83c6321b8faba2ec6202af70b1cc0e2ed936495e` | Go, deep package structure |
| [`pallets/flask`](https://github.com/pallets/flask) | `6a2f545bfd8ed31e19066a299296917e034aca58` | Python |
| [`BurntSushi/ripgrep`](https://github.com/BurntSushi/ripgrep) | `3fce3b5bb0236da2df6d99672afb8a719642eca7` | Rust, a cargo workspace |
| [`google/gson`](https://github.com/google/gson) | `9d5d6a8f457f3e60f2e84bf39b904c9a774d2365` | Java, package-path imports |
| [`slimphp/Slim`](https://github.com/slimphp/Slim) | `80900fb39cafce3ae53b18a2c4f642a122f03095` | PHP, PSR-4 namespaces |
| [`sinatra/sinatra`](https://github.com/sinatra/sinatra) | `cb22afd7902b566b6eaba6c4ea89739494a65d12` | Ruby |
| [`fmtlib/fmt`](https://github.com/fmtlib/fmt) | `60ccad511fd680cb91d8b60a315759f71c67bef9` | C++ |
| [`curl/curl`](https://github.com/curl/curl) | `7f6a75664f9fb45390193e492ee3361fb795d098` | C |
| [`JamesNK/Newtonsoft.Json`](https://github.com/JamesNK/Newtonsoft.Json) | `4f73e74372445108d2c1bda37b36e6f5e43402e0` | C#, namespaces |
| [`documenso/documenso`](https://github.com/documenso/documenso) | `f0ab7c112e3c39656b0153b67fbf25fd9616e96f` | the existing baseline |

Reproduce any row:

```bash
git clone --depth 1 https://github.com/cli/cli
kog scan cli -o cli.json
```

Everything below is derived from those JSON files with `jq`. No step of this
document required looking at a filesystem by hand.

---

## Per repository

| Repository | Files seen | Analysed | Not read | Source coverage | Resolution rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| withastro/docs | 2,941 | 2,701 | 0 | **1.0000** | **0.9975** |
| cli/cli | 1,338 | 927 | 8 | **0.9914** | **1.0000** |
| pallets/flask | 236 | 106 | 4 | **0.9636** | **0.9970** |
| BurntSushi/ripgrep | 236 | 114 | 2 | **0.9828** | **0.9734** |
| google/gson | 314 | 264 | 4 | **0.9851** | **1.0000** |
| slimphp/Slim | 145 | 125 | 0 | **1.0000** | **0.9807** |
| sinatra/sinatra | 292 | 155 | 76 | **0.6710** | **1.0000** |
| fmtlib/fmt | 142 | 79 | 5 | **0.9405** | **0.8571** |
| curl/curl | 4,437 | 1,097 | 152 | **0.8783** | **0.9705** |
| JamesNK/Newtonsoft.Json | 988 | 945 | 0 | **1.0000** | **1.0000** |
| documenso/documenso | 2,833 | 2,243 | 164 | **0.9319** | **0.9779** |

## Per language, summed across the corpus

| Language | Files | Internal | Resolved | Unresolved | Excluded | Rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| mdx | 2,693 | 3,343 | 3,337 | 6 | 0 | **0.9982** |
| typescript | 2,133 | 10,435 | 10,209 | 226 | 0 | **0.9783** |
| c | 1,040 | 3,164 | 3,063 | 101 | 0 | **0.9681** |
| csharp | 945 | 453 | 453 | 0 | 0 | **1.0000** |
| go | 908 | 3,427 | 3,211 | 0 | 216 | **1.0000** |
| java | 264 | 1,082 | 1,082 | 0 | 0 | **1.0000** |
| ruby | 151 | 76 | 76 | 0 | 0 | **1.0000** |
| python | 139 | 414 | 414 | 0 | 0 | **1.0000** |
| php | 125 | 311 | 305 | 6 | 0 | **0.9807** |
| rust | 110 | 526 | 512 | 14 | 0 | **0.9734** |
| astro | 81 | 125 | 122 | 3 | 0 | **0.9760** |
| shell | 58 | 9 | 7 | 2 | 0 | 0.7778 |
| cpp | 48 | 142 | 124 | 18 | 0 | **0.8732** |
| javascript | 24 | 4 | 1 | 3 | 0 | 0.2500 |
| html | 21 | 3 | 3 | 0 | 0 | 1.0000 |
| css / sass / stylus | 16 | 0 | 0 | 0 | 0 | 1.0000 |

---

## What it found: Go resolved 12 % of its own imports

The first scan of `cli/cli` published **0.1261** for Go: 2,995 of 3,427
internal specifiers unresolved, on a repository whose imports are entirely
ordinary. Every single recorded diagnostic was an import of the repository's
*own* module.

The cause is worth writing down because nothing smaller than a real repository
would have produced it. `cli/cli` ships a CodeQL test fixture at
`.github/codeql/tests/unsanitized-response-to-terminal/go.mod` whose `module`
line repeats the repository's own, verbatim:

```
module github.com/cli/cli/v2
```

So two `go.mod` files declared the same module path. The resolver ordered
modules by path length — a tie — and returned on the first one whose prefix
matched. Every self-import in the repository was therefore resolved against a
fixture directory containing none of them, and because a matched module prefix
is deliberately *fail-closed* (a self-import that names nothing must be
`unresolved`, never quietly downgraded to an external dependency), each one
became a broken import with no second chance.

The fix is Go's actual rule, which the resolver was not implementing: **a file
belongs to the module of its nearest enclosing `go.mod`**. That module is tried
first, then every other module whose path matches, and only if none of them
holds the package is the import unresolved.

| | Internal | Resolved | Unresolved | Rate |
| --- | ---: | ---: | ---: | ---: |
| before | 3,427 | 432 | 2,995 | **0.1261** |
| after | 3,427 | 3,211 | 0 | **1.0000** |

Edges on `cli/cli` went from 432 to 26,086.

The 216 `excluded` are generated code the repository gitignores — resolved to a
real file outside the scanned set, which is a policy decision and not a parser
failure, so they leave the denominator like any other exclusion.

---

## MDX ships, and needed a real repository to be judged

MDX is Markdown with ESM in it, resolved through the same TypeScript front end
as everything else. Two measurements, and they look contradictory until you
read them:

| Repository | MDX files | Internal specifiers | Edges | Rate |
| --- | ---: | ---: | ---: | ---: |
| withastro/docs | 2,550 | 3,343 | 3,337 | **0.9982** |
| documenso | 143 | 0 | 0 | 1.0000 |

On `documenso` every MDX import names a package, so MDX contributes **zero
edges** there — it only moves that repository's source coverage from 0.8725 to
0.9319. Measured on documenso alone, MDX looks exactly like the kind of
extractor the roadmap forbids: one that raises coverage without adding
information.

`withastro/docs` settles it. There, MDX resolves 3,337 internal imports into
3,337 real edges. The mechanism is real; documenso simply does not use it.

### What the extractor has to get right

Documentation is full of code that is *shown*, not run. Handing an `.mdx` file
straight to a TypeScript parser reports every sample in it as an import.

On documenso that is not hypothetical. Its embedding guides contain, inside
fenced blocks:

```jsx
import { EmbedCreateDocumentV1 } from '@documenso/embed-react';
```

`@documenso/embed-react` is a **real workspace package**. Left unhandled, those
fourteen samples would have resolved to real files and put **fourteen fabricated
edges into the published graph** — a documentation page appearing to depend on
the embed package's source. Samples naming things that do *not* exist are worse
still: they land as `unresolved` and depress the published rate with fiction.

So the parser is handed the top-level ESM statements and nothing else, with
everything around them blanked rather than removed, so a diagnostic still points
at the real line. Verified on documenso after the fact: of the packages that
appear only inside samples — `fs`, `crypto`, `express`, `form-data`,
`express-rate-limit`, `@angular/core` — **none** appears on any MDX node, and
**no** diagnostic is attributed to MDX.

---

## SQL is deliberately not read

SQL is the largest remaining gap on documenso: 163 files, and adding an
extractor would move its source coverage from 0.9319 to roughly 0.98.

It is not being added, because the measurement says it would buy nothing:

| Question | documenso | ~/Mastore (private, 9 projects) |
| --- | ---: | ---: |
| `.sql` files | 163 | 56 |
| in psql include form (`\i`, `\ir`) | 0 | 0 |
| naming another `.sql` file, any syntax | 0 | 0 |

All 163 are Prisma migrations. Migrations do not reference one another — that
is what a migration *is*. An extractor here would add 163 nodes with zero edges
and move a published number by five points without adding one piece of
information, which the roadmap names explicitly as cheating on the number.

The condition for revisiting is a mechanism, not a file count: a repository
using psql `\i`/`\ir` includes, or dbt's `{{ ref('model') }}`, has real
cross-file references and would deserve an extractor. Prisma migrations do not.

---

## What auditing *every extension* found: coverage was overstated

The corpus above was chosen one repository per *language KOG already
supports*. That is still too narrow a question. The coverage report classifies
every extension it meets — `.json`, `.md`, `.yaml`, `.png` and the rest — and
that classification had never been audited against anything.

Auditing it across all eleven repositories found source code filed as **not
source**, which is not a cosmetic mistake: `not_source` leaves the coverage
denominator entirely. Documentation and images leave it for a good reason — a
repository is not worse mapped for containing a README. But an extension KOG
has simply never heard of defaulted into the same bucket, so **anything
unfamiliar silently improved the published number.**

That is the defect this tool exists to catch, pointing the other way.

Ruby template languages were the worst case. A template renders other
templates, so it carries real cross-file references and is source by any
reading:

| Repository | Hidden files | Published | Honest |
| --- | ---: | ---: | ---: |
| sinatra/sinatra | 68 (`.erb`, `.haml`, `.slim`, `.erubis`, `.hamlit`) | 0.9627 | **0.6710** |
| curl/curl | 62 (`.m4`, `.am`, `.in`) | 0.9104 | **0.8783** |
| cli/cli | 1 | 0.9925 | **0.9914** |

Sinatra's published coverage was overstated by **29 points**. Templates and
build-system source are now catalogued as source, so they count against the
number instead of vanishing from it, and each is named in the gap list.

Still open from the same audit, and not yet decided: `.inc` (11 files —
ambiguous between C, PHP and assembler), `.txtar` (143 — Go test archives),
`.erb`-adjacent formats not seen in this corpus. Extensions genuinely not
source, such as `.zip`, `.png` and `LICENSE`, are correctly excluded; the
report names them as unrecognised rather than pretending to know.

## Still weak, stated plainly

- **C++ at 0.8732.** All 18 unresolved specifiers on `fmtlib/fmt` are
  third-party headers — `gtest/gtest.h`, `gmock/gmock.h`, `absl/...`. They are
  dependencies and should leave the denominator as `external`; instead the
  C-like resolver probes them as internal and fails closed. Same class of
  defect as the Go bug above, one order of magnitude smaller. Not yet fixed.
- **shell at 0.7778 and javascript at 0.2500** are 9 and 4 specifiers
  respectively. Neither is a gate; both are samples too small to mean anything,
  and both are reported rather than hidden. Shell's two are
  `${SCRIPTDIR}/config400.override` and `.venv/bin/activate` — one genuinely
  undecidable, one genuinely absent.
- **Eleven repositories is a better corpus than two. It is still not a corpus.**
  One repository per language, chosen by the author of the tool, is a start.
