//! SQLite-backed [`DocumentStore`] (spec §7.2).
//!
//! The schema keeps the spec's queryable scalar columns (for dedupe, listing,
//! and chunk lookup) and additionally stores the **full serialized IR** as
//! JSON, so a document ingested in one process can be retrieved and re-emitted
//! in another. Chunk embeddings are stored durably (`chunk_embeddings`) so the
//! vector index can be rebuilt without re-calling the embedding provider —
//! SQLite is the system of record; the index is a derived projection (see
//! `docs/adr/0001-persistence-and-index-consistency.md`).
//!
//! Concurrency: a single connection behind a `Mutex` (rusqlite `Connection`
//! is `Send` but not `Sync`). Fine for the CLI and tests; a connection pool
//! is a later concern.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::chunking::{Chunk, ChunkId};
use crate::embedder::EmbeddingSpace;
use crate::ir::{Document, DocumentId, DocumentMeta};
use crate::storage::{DocumentStore, check_space_binding, validate_projection};
use crate::{Error, Result};

/// Current schema version, tracked in `PRAGMA user_version`.
/// v3 adds the `library_metadata` singleton binding the library to one
/// embedding space.
const SCHEMA_VERSION: i64 = 3;

#[derive(Clone)]
pub struct SqliteDocumentStore {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for SqliteDocumentStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteDocumentStore")
            .finish_non_exhaustive()
    }
}

impl SqliteDocumentStore {
    /// Open (creating if needed) a database at `path` and run migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref()).map_err(sqlite_err)?;
        Self::from_connection(conn)
    }

    /// Open a private in-memory database (each instance is independent).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(sqlite_err)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(sqlite_err)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Storage("sqlite connection mutex poisoned".to_string()))
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sqlite_err)?;

    if version < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS documents (
                id            TEXT PRIMARY KEY,
                source        TEXT NOT NULL,
                source_format TEXT NOT NULL,
                title         TEXT,
                ingested_at   TEXT NOT NULL,
                content_hash  TEXT NOT NULL UNIQUE,
                doc_json      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id             TEXT PRIMARY KEY,
                document_id    TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                section_path   TEXT NOT NULL,
                content        TEXT NOT NULL,
                block_type     TEXT NOT NULL,
                sequence_index INTEGER NOT NULL,
                token_count    INTEGER NOT NULL,
                chunk_json     TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_document ON chunks(document_id);
            CREATE INDEX IF NOT EXISTS idx_chunks_block_type ON chunks(block_type);
            "#,
        )
        .map_err(sqlite_err)?;
    }

    if version < 2 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chunk_embeddings (
                chunk_id    TEXT PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
                dimension   INTEGER NOT NULL,
                vector_json TEXT NOT NULL
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_chunks_doc_seq
                ON chunks(document_id, sequence_index);
            "#,
        )
        .map_err(sqlite_err)?;
    }

    if version < 3 {
        // The singleton row starts unbound (NULLs). A pre-v3 store that
        // already holds embeddings therefore migrates to "unbound with
        // embeddings" — the fail-closed legacy state: those vectors' model
        // identity cannot be inferred, so `Library` construction refuses it
        // rather than silently labeling them with the configured model.
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS library_metadata (
                singleton                    INTEGER PRIMARY KEY CHECK (singleton = 1),
                embedding_space_fingerprint  TEXT,
                embedding_dimension          INTEGER
            );

            INSERT OR IGNORE INTO library_metadata (
                singleton,
                embedding_space_fingerprint,
                embedding_dimension
            ) VALUES (1, NULL, NULL);
            "#,
        )
        .map_err(sqlite_err)?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(sqlite_err)?;
    Ok(())
}

/// Read the bound embedding space within an existing connection/transaction.
fn read_embedding_space(conn: &Connection) -> Result<Option<EmbeddingSpace>> {
    let row: Option<(Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT embedding_space_fingerprint, embedding_dimension
             FROM library_metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sqlite_err)?;
    match row {
        None | Some((None, None)) => Ok(None),
        Some((Some(fingerprint), Some(dimension))) => {
            let dimension = usize::try_from(dimension).map_err(|_| {
                Error::Storage(format!(
                    "library_metadata records an invalid embedding dimension ({dimension})"
                ))
            })?;
            Ok(Some(EmbeddingSpace {
                fingerprint,
                dimension,
            }))
        }
        Some(_) => Err(Error::Consistency(
            "library_metadata is half-bound (one of fingerprint/dimension is NULL); \
             the database is damaged"
                .to_string(),
        )),
    }
}

impl DocumentStore for SqliteDocumentStore {
    fn get_document(&self, id: DocumentId) -> Result<Document> {
        let conn = self.lock()?;
        let json: Option<String> = conn
            .query_row(
                "SELECT doc_json FROM documents WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_err)?;
        match json {
            Some(json) => serde_json::from_str(&json).map_err(json_err),
            None => Err(Error::Storage(format!("document not found: {id}"))),
        }
    }

    fn find_by_content_hash(&self, content_hash: &str) -> Result<Option<DocumentId>> {
        let conn = self.lock()?;
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM documents WHERE content_hash = ?1",
                params![content_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_err)?;
        match id {
            Some(id) => Ok(Some(parse_uuid(&id)?)),
            None => Ok(None),
        }
    }

    fn list_documents(&self) -> Result<Vec<DocumentMeta>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT doc_json FROM documents ORDER BY ingested_at")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sqlite_err)?;
        let mut out = Vec::new();
        for row in rows {
            let json = row.map_err(sqlite_err)?;
            let doc: Document = serde_json::from_str(&json).map_err(json_err)?;
            out.push(doc.meta);
        }
        Ok(out)
    }

    fn remove_document(&self, id: DocumentId) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(sqlite_err)?;
        // Explicit deletes (not just FK cascade) so behavior holds even if
        // foreign_keys is ever off.
        tx.execute(
            "DELETE FROM chunk_embeddings WHERE chunk_id IN
               (SELECT id FROM chunks WHERE document_id = ?1)",
            params![id.to_string()],
        )
        .map_err(sqlite_err)?;
        tx.execute(
            "DELETE FROM chunks WHERE document_id = ?1",
            params![id.to_string()],
        )
        .map_err(sqlite_err)?;
        tx.execute(
            "DELETE FROM documents WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(sqlite_err)?;
        tx.commit().map_err(sqlite_err)?;
        Ok(())
    }

    fn embedding_space(&self) -> Result<Option<EmbeddingSpace>> {
        let conn = self.lock()?;
        read_embedding_space(&conn)
    }

    fn has_embeddings(&self) -> Result<bool> {
        let conn = self.lock()?;
        conn.query_row("SELECT EXISTS(SELECT 1 FROM chunk_embeddings)", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_err)
    }

    fn replace_document_projection(
        &self,
        document: &Document,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
        embedding_space: &EmbeddingSpace,
    ) -> Result<()> {
        validate_projection(document, chunks, embeddings, embedding_space)?;

        let doc_json = serde_json::to_string(document).map_err(json_err)?;
        let format = format!("{:?}", document.meta.format);

        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(sqlite_err)?;

        // Bind-or-verify the library's space inside the same transaction as
        // the projection write, so no concurrent writer can slip a different
        // space in between the check and the mutation.
        let bound = read_embedding_space(&tx)?;
        if check_space_binding(bound.as_ref(), embedding_space)? {
            tx.execute(
                "UPDATE library_metadata
                 SET embedding_space_fingerprint = ?1, embedding_dimension = ?2
                 WHERE singleton = 1",
                params![
                    embedding_space.fingerprint,
                    embedding_space.dimension as i64
                ],
            )
            .map_err(sqlite_err)?;
        }

        tx.execute(
            r#"INSERT INTO documents
                 (id, source, source_format, title, ingested_at, content_hash, doc_json)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
               ON CONFLICT(id) DO UPDATE SET
                 source=excluded.source,
                 source_format=excluded.source_format,
                 title=excluded.title,
                 ingested_at=excluded.ingested_at,
                 content_hash=excluded.content_hash,
                 doc_json=excluded.doc_json"#,
            params![
                document.meta.id.to_string(),
                document.meta.source.raw_source,
                format,
                document.meta.title,
                document.meta.ingested_at.to_rfc3339(),
                document.meta.content_hash,
                doc_json,
            ],
        )
        .map_err(sqlite_err)?;

        // Retire the entire previous chunk generation before inserting the
        // new one — the table never holds a partial mix.
        tx.execute(
            "DELETE FROM chunk_embeddings WHERE chunk_id IN
               (SELECT id FROM chunks WHERE document_id = ?1)",
            params![document.meta.id.to_string()],
        )
        .map_err(sqlite_err)?;
        tx.execute(
            "DELETE FROM chunks WHERE document_id = ?1",
            params![document.meta.id.to_string()],
        )
        .map_err(sqlite_err)?;

        for (chunk, embedding) in chunks.iter().zip(embeddings) {
            let section_path = serde_json::to_string(&chunk.section_path).map_err(json_err)?;
            let chunk_json = serde_json::to_string(chunk).map_err(json_err)?;
            tx.execute(
                r#"INSERT INTO chunks
                     (id, document_id, section_path, content, block_type, sequence_index, token_count, chunk_json)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
                params![
                    chunk.id.to_string(),
                    chunk.document_id.to_string(),
                    section_path,
                    chunk.content,
                    format!("{:?}", chunk.block_type),
                    chunk.sequence_index as i64,
                    chunk.token_count as i64,
                    chunk_json,
                ],
            )
            .map_err(sqlite_err)?;

            let vector_json = serde_json::to_string(embedding).map_err(json_err)?;
            tx.execute(
                "INSERT INTO chunk_embeddings (chunk_id, dimension, vector_json)
                 VALUES (?1, ?2, ?3)",
                params![chunk.id.to_string(), embedding.len() as i64, vector_json],
            )
            .map_err(sqlite_err)?;
        }

        tx.commit().map_err(sqlite_err)?;
        Ok(())
    }

    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT chunk_json FROM chunks WHERE document_id = ?1 ORDER BY sequence_index")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![id.to_string()], |row| row.get::<_, String>(0))
            .map_err(sqlite_err)?;
        let mut out = Vec::new();
        for row in rows {
            let json = row.map_err(sqlite_err)?;
            out.push(serde_json::from_str(&json).map_err(json_err)?);
        }
        Ok(out)
    }

    fn get_chunks_by_ids(&self, ids: &[ChunkId]) -> Result<Vec<Chunk>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.lock()?;
        let mut by_id: std::collections::HashMap<ChunkId, Chunk> = std::collections::HashMap::new();
        let mut stmt = conn
            .prepare("SELECT chunk_json FROM chunks WHERE id = ?1")
            .map_err(sqlite_err)?;
        for id in ids {
            let json: Option<String> = stmt
                .query_row(params![id.to_string()], |row| row.get(0))
                .optional()
                .map_err(sqlite_err)?;
            if let Some(json) = json {
                let chunk: Chunk = serde_json::from_str(&json).map_err(json_err)?;
                by_id.insert(chunk.id, chunk);
            }
        }
        Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect())
    }

    fn get_all_embeddings(&self) -> Result<Vec<(ChunkId, Vec<f32>)>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT chunk_id, vector_json FROM chunk_embeddings")
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sqlite_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, vector_json) = row.map_err(sqlite_err)?;
            let vector: Vec<f32> = serde_json::from_str(&vector_json).map_err(json_err)?;
            out.push((parse_uuid(&id)?, vector));
        }
        Ok(out)
    }
}

fn parse_uuid(s: &str) -> Result<DocumentId> {
    DocumentId::parse_str(s).map_err(|e| Error::Storage(format!("invalid UUID in database: {e}")))
}

fn sqlite_err(e: rusqlite::Error) -> Error {
    Error::Storage(format!("sqlite: {e}"))
}

fn json_err(e: serde_json::Error) -> Error {
    Error::Storage(format!("serialization: {e}"))
}
