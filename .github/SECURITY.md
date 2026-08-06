# Security Policy

KOG reads TypeScript source code and produces a file/import graph. The code you scan
may contain proprietary information, credentials in comments, or other sensitive content.
Security is a legitimate concern for this project.

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Use GitHub's private vulnerability reporting on this repository
(*Security* → *Report a vulnerability*).

Please include what you found, how to reproduce it, and what an attacker could
obtain. You'll get an acknowledgement within 72 hours.

This is an open-source project maintained by volunteers, so please don't expect a
commercial SLA — but security issues will be treated as high priority.

## What counts as a vulnerability here

- **Output exposure**: The graph is written to a file or stream. If that output leaks
  proprietary code structure or embedded credentials from comments, that's a problem.
- **Parser crashes on malicious input**: A crafted TypeScript file that crashes the
  parser or causes unbounded resource use.
- **Incorrect graph construction**: A bug that silently produces incorrect edges or
  misses imports. This could lead to wrong conclusions if the graph is used for
  dependency analysis or security scanning.

## What is not a vulnerability

- **You must review your own code before scanning**. KOG reads whatever files
  you point it to. If you scan a directory containing secrets, those secrets can
  appear in the output. This is your responsibility.
- **Malformed TypeScript, by itself**. The parser is built on tree-sitter,
  which is fault-tolerant by design: it never rejects input, it recovers from
  syntax errors and extracts whatever it still can. A crafted file that
  merely confuses the parser into missing or misattributing an import is a
  correctness bug (see "Incorrect graph construction" above) — only a file
  that crashes the process or exhausts resources is a security issue.
- Vulnerabilities in tree-sitter or other upstream Rust crates. Please
  report those to their maintainers.

## Design-level mitigations

Described in the design document [`docs/design/v0-design.md`](../docs/design/v0-design.md).
The parser runs locally on your machine. No data is sent anywhere. What you see in the
output is exactly what KOG found in your source — no filtering, no external calls.
