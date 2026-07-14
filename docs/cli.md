# Nucklavee CLI reference

The CLI is a persistent library: documents live in a SQLite database and a
usearch vector index whose paths come from a config file, so `ingest` in one
invocation is searchable by `search` in the next.

## Configuration

Read from `--config <path>`, or by default `~/.config/forge/config.toml`:

```toml
[storage]
database = "/home/you/.local/share/nucklavee/library.sqlite"
vector_index = "/home/you/.local/share/nucklavee/library.usearch"

[embedding]
endpoint = "https://api.openai.com/v1/embeddings"
model = "text-embedding-3-small"
dimension = 1536
api_key = "sk-..."     # optional; omit for local servers that need no auth
use_env_proxy = true   # optional; default true
```

The embedding `dimension` must match the vectors your endpoint returns and
stays fixed for the life of an index file. The endpoint is any
OpenAI-compatible `/v1/embeddings` service (OpenAI, `llama.cpp --embedding`,
etc.).

### One library, one embedding model

A library is permanently bound to the embedding space of its first ingest —
the combination of `endpoint`, `model`, and `dimension`. Every later command
must run under the same configuration; changing any of them (even to a
different model with the same dimension) makes every command fail with an
`embedding-space mismatch` error that names both spaces. Vectors from
different models are never comparable, so nucklavee refuses before anything
is searched or written.

To use a different embedding model, point `[storage]` at a **new** database
and index path and re-ingest your documents. `rebuild-index` cannot help
here: it rebuilds the index from the embeddings already stored in SQLite,
and no rebuild can convert stored embeddings from one model to another.

Databases created before embedding-space tracking (schema v2) that already
contain embeddings fail closed with a `legacy embeddings` error, because the
model that produced those vectors was never recorded. The remedy is the
same: create a new library and re-ingest.

## Global flags

- `--config <path>` — config file location.
- `--json` — machine-readable JSON output (applies to `ingest`, `list`,
  `info`, `search`, `rebuild-index`).

## Commands

| Command | Description |
|---|---|
| `ingest <path\|url>` | Ingest a local `.md`/`.html`/`.htm` file or an `http(s)://` URL: parse → store → chunk → embed → index. Prints the document ID. `--normalize-bare-callouts` rewrites `[!tip]`-style callouts (markdown). |
| `search <query> [--limit N]` | Semantic search (default limit 10). Prints ranked chunks as `[section path] snippet`. |
| `emit <id> <markdown\|html\|text>` | Emit a stored document in the given format. |
| `list` | List every document as `id⇥title`. |
| `info <id>` | Show a document's metadata and chunk count. |
| `context <query> [--budget N]` | Assemble a relevance-ranked, provenance-headed context window within a token budget (default 2048). |
| `remove <id>` | Remove a document and its chunks from the store and the vector index. |
| `rebuild-index` | Rebuild the vector index from embeddings stored in SQLite — no re-embedding, no network. Use after index corruption or loss; the store is authoritative and the index is a derived projection. This command never loads the existing index files (it starts empty and repopulates), so it works even when they are corrupt or missing; any other command whose index load fails prints an error pointing here. It only rebuilds under the embedding model the library is bound to — run with a different configured model it refuses, because rebuilding cannot convert stored embeddings between models. |

Unknown `emit` formats exit with code 2 and
`unsupported format '<value>'. supported: markdown, html, text`. A missing or
unreadable config exits with code 1 and a message pointing at `--config`.
A configured model/endpoint/dimension that does not match the library's
bound embedding space exits with code 1 and an `embedding-space mismatch`
message; a pre-space database with unidentified embeddings exits with code 1
and a `legacy embeddings` migration message (see above).

## Examples

```bash
# Ingest a file, then a web page
nucklavee ingest ./notes/valence.md
nucklavee ingest https://example.com/article

# Search and assemble LLM context
nucklavee search "electromagnetic valence in semiconductors" --limit 5
nucklavee context "how does valence affect conductivity" --budget 1500

# Convert and inspect
nucklavee emit 3f2a… html > page.html
nucklavee list --json
nucklavee info 3f2a…
nucklavee remove 3f2a…
```
