# CLI boundary (Phase 2 core, extended by Phase-3 HTML ingest)

The CLI supports retrieval through `ingest-emit` only until the Phase-5 CLI
rework (audit Task 10).

Persistence semantics are intentionally narrow:

- The CLI uses an in-memory document store.
- Document IDs are process-local and only valid inside the process that created them.
- A document ID printed by `ingest` cannot be reused by a separate CLI invocation.
- For reliable behavior, perform ingest + emit in one process with `ingest-emit`.

## Supported commands

### `ingest`

- Purpose: ingest a local markdown or HTML file into the in-memory store.
- Accepted extensions: `.md`, `.html`, `.htm`.
- Output: prints the in-memory document ID to stdout.
- Important: ID is process-local and ephemeral.
- Re-ingesting identical content returns the existing document ID
  (content-hash deduplication).

### `ingest-emit`

- Purpose: ingest and immediately emit in one process (the retrieval path).
- Required args:
  - `path` (`.md`, `.html`, or `.htm` file)
- Optional args:
  - `--format` (default `markdown`)
  - `--normalize-bare-callouts` (markdown ingest only)
- Supported emit format values:
  - `markdown`
- Output: emitted markdown content to stdout. HTML input is run through
  readability-style content extraction (chrome such as `<nav>`, `<header>`,
  `<footer>`, `<aside>`, `<script>` is stripped) before DOM→IR mapping.

## Unsupported and not implemented commands

### Disabled

- `emit --id <id> --format markdown`
  - Status: disabled (document IDs are process-local).
  - Behavior: returns this error:
    - `emit --id is disabled because document IDs are process-local. use \`ingest-emit <path> --format markdown\``

### Not implemented

- `query`
  - Behavior: `query is not implemented yet (Phase 4); ingest/emit only`
- `context-window`
  - Behavior: `context_window is not implemented yet (Phase 4); ingest/emit only`
- `html`
  - Behavior: `there is no standalone \`html\` command; html ingest is supported via \`ingest\`/\`ingest-emit\` on .html files (html emit arrives in Phase 3, Task 3)`
- `pdf`
  - Behavior: `pdf pipeline is not implemented yet (Phase 6); ingest supports .md and .html files`

## Command matrix

| Command | Status | Expected behavior |
|---|---|---|
| `ingest <path.md\|path.html>` | Supported | Prints in-memory `DocumentId` to stdout (valid only in current process). |
| `ingest-emit <path.md\|path.html> --format markdown` | Supported | Prints emitted markdown to stdout (HTML input is content-extracted first). |
| `ingest-emit <path> --format <other>` | Supported (command), invalid input for format | Returns: `unsupported format '<value>'. supported: markdown`. |
| `ingest-emit <path.docx>` | Supported (command), invalid input for extension | Returns: `unsupported file extension 'docx'. supported: .md, .html, .htm`. |
| `emit --id <id> --format markdown` | Disabled | Returns disabled error describing process-local IDs and recommending `ingest-emit`. |
| `query` | Not Implemented | Returns Phase-4 not-implemented error for query. |
| `context-window` | Not Implemented | Returns Phase-4 not-implemented error for context window. |
| `html` | Not Implemented | Points at `ingest`/`ingest-emit` for HTML ingest; html *emit* is Phase 3 Task 3. |
| `pdf` | Not Implemented | Returns Phase-6 not-implemented error for pdf pipeline. |

All boundary strings are canonical in `src/phase2_contract.rs` and asserted
exactly by `tests/cli_smoke_contract.rs`.

## Verification

Use separate commands so shell newline escapes are not interpreted literally:

```bash
./target/debug/nucklavee ingest-emit tests/corpus/electromagnetic-valence.md --format markdown > /tmp/emv-after.md
diff -u /tmp/emv-before.md /tmp/emv-after.md
./target/debug/nucklavee ingest-emit tests/fixtures/html/docs_site.html --format markdown
```
