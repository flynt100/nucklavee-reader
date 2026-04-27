# CLI Phase 2 boundary

Phase 2 supports retrieval through `ingest-emit` only.

Persistence semantics are intentionally narrow:

- The CLI uses an in-memory document store.
- Document IDs are process-local and only valid inside the process that created them.
- A document ID printed by `ingest` cannot be reused by a separate CLI invocation.
- For reliable behavior, perform ingest + emit in one process with `ingest-emit`.

## Supported commands

### `ingest`

- Purpose: ingest a local markdown file into the in-memory store.
- Output: prints the in-memory document ID to stdout.
- Important: ID is process-local and ephemeral.

### `ingest-emit`

- Purpose: ingest and immediately emit in one process (Phase 2 retrieval path).
- Required args:
  - `path`
- Optional args:
  - `--format` (default `markdown`)
- Supported format values:
  - `markdown`
- Output: emitted markdown content to stdout.

## Unsupported and not implemented commands

### Disabled

- `emit --id <id> --format markdown`
  - Status: disabled in Phase 2.
  - Behavior: returns this error:
    - `emit --id is disabled in Phase 2 because document IDs are process-local. use \`ingest-emit <path> --format markdown\``

### Not implemented

- `query`
  - Behavior: `query is not implemented in Phase 2 (markdown ingest/emit only)`
- `context-window`
  - Behavior: `context_window is not implemented in Phase 2 (markdown ingest/emit only)`
- `html`
  - Behavior: `html pipeline is not implemented in Phase 2; markdown only`
- `pdf`
  - Behavior: `pdf pipeline is not implemented in Phase 2; markdown only`

## Command matrix

| Command | Status | Expected behavior |
|---|---|---|
| `ingest <path>` | Supported | Prints in-memory `DocumentId` to stdout (valid only in current process). |
| `ingest-emit <path> --format markdown` | Supported | Prints emitted markdown to stdout. |
| `ingest-emit <path> --format <other>` | Supported (command), invalid input for format | Returns: `unsupported format '<value>'. supported: markdown`. |
| `emit --id <id> --format markdown` | Disabled | Returns disabled-phase error describing process-local IDs and recommending `ingest-emit`. |
| `query` | Not Implemented | Returns Phase 2 not-implemented error for query. |
| `context-window` | Not Implemented | Returns Phase 2 not-implemented error for context window. |
| `html` | Not Implemented | Returns Phase 2 not-implemented error for html pipeline. |
| `pdf` | Not Implemented | Returns Phase 2 not-implemented error for pdf pipeline. |
