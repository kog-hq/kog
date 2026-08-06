# Security Policy

mycelium reads your complete AI assistant history. That history contains API keys,
`.env` contents, database credentials and proprietary source code. Security is
not a peripheral concern for this project — it is the concern.

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Use GitHub's private vulnerability reporting on this repository
(*Security* → *Report a vulnerability*).

Please include what you found, how to reproduce it, and what an attacker could
obtain. You'll get an acknowledgement within 72 hours.

This is a hobby project maintained by one person, so please don't expect a
commercial SLA — but secret leakage will be treated as the highest priority.

## What counts as a vulnerability here

Above all: **a secret reaching a place the user didn't consent to.** Concretely,

- A credential format that defeats the redaction rules and ends up in a window
  sent to an LLM, or written into a note in the vault.
- A session in a denied project being read past its header.
- The search index or state files exposing content the vault doesn't.
- Path traversal in note writing that escapes the configured vault subfolder.

If you have a secret format that slips through redaction, that is a valid report
even without a full exploit. Include the *shape* of the secret, never a real one.

## What is not a vulnerability

- **The distiller sends data to an LLM provider.** This is the documented design:
  the `cli` backend shells out to a tool like `claude -p`, which transmits the
  window to its provider. It's stated in the README, in the design doc, and at
  `mycelium init`. Use project deny lists for anything that must not leave.
- **Notes in the vault are plaintext Markdown.** By design — the vault is meant
  to be read by Obsidian and searched by you. Protect it as you protect the rest
  of your notes.
- Vulnerabilities in Obsidian, Claude Code, Codex or other upstream tools. Please
  report those to their maintainers.

## Design-level mitigations

Described in §6.2 of [`docs/design/v1-design.md`](../docs/design/v1-design.md).
Redaction runs before any data leaves the machine and again on the produced note
— deliberately redundant. Project filtering is applied earlier still, at read
time. Reports that these layers are insufficient are exactly what this policy is
for.
