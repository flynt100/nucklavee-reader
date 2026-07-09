//! SQLite-backed [`DocumentStore`] (spec §7.2).
//!
//! The schema keeps the spec's queryable scalar columns (for dedupe, listing,
//! and chunk lookup) and additionally stores the **full serialized IR** as
//! JSON, so a document ingested in one process can be retrieved and re-emitted
//! in another — the in-memory store cannot do that.
//!
//! Concurrency: a single connection behind a `Mutex` (rusqlite `Connection`
//! is `Send` but not `Sync`). Fine for the CLI and tests; a connection pool
//! is a later concern.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::chunking::{Chunk, ChunkId};
use crate::ir::{Document, DocumentId, DocumentMeta};
use crate::storage::DocumentStore;
use crate::{Error, Result};

/// Current schema version, tracked in `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

#[derive(Clone)]
pub struct SqliteDocumentStore {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for SqliteDocumentStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteDocumentStore").finish_non_exhaustive()
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
    if version >= SCHEMA_VERSION {
        return Ok(());
    }

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

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(sqlite_err)?;
    Ok(())
}

impl DocumentStore for SqliteDocumentStore {
    fn upsert_document(&self, document: &Document) -> Result<()> {
        let conn = self.lock()?;
        let doc_json = serde_json::to_string(document).map_err(json_err)?;
        let block_type = format!("{:?}", document.meta.format);
        conn.execute(
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
                block_type,
                document.meta.title,
                document.meta.ingested_at.to_rfc3339(),
                document.meta.content_hash,
                doc_json,
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

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
        let conn = self.lock()?;
        // Explicit chunk delete as well, in case foreign_keys is ever off.
        conn.execute(
            "DELETE FROM chunks WHERE document_id = ?1",
            params![id.to_string()],
        )
        .map_err(sqlite_err)?;
        conn.execute("DELETE FROM documents WHERE id = ?1", params![id.to_string()])
            .map_err(sqlite_err)?;
        Ok(())
    }

    fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(sqlite_err)?;
        for chunk in chunks {
            let section_path = serde_json::to_string(&chunk.section_path).map_err(json_err)?;
            let chunk_json = serde_json::to_string(chunk).map_err(json_err)?;
            tx.execute(
                r#"INSERT INTO chunks
                     (id, document_id, section_path, content, block_type, sequence_index, token_count, chunk_json)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                   ON CONFLICT(id) DO UPDATE SET
                     document_id=excluded.document_id,
                     section_path=excluded.section_path,
                     content=excluded.content,
                     block_type=excluded.block_type,
                     sequence_index=excluded.sequence_index,
                     token_count=excluded.token_count,
                     chunk_json=excluded.chunk_json"#,
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
        }
        tx.commit().map_err(sqlite_err)?;
        Ok(())
    }

    fn get_chunks_by_document(&self, id: DocumentId) -> Result<Vec<Chunk>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT chunk_json FROM chunks WHERE document_id = ?1 ORDER BY sequence_index",
            )
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
        // Fetch all requested chunks, then reorder to match `ids` (preserving
        // order, skipping unknown) exactly like the in-memory store.
        let mut by_id: std::collections::HashMap<ChunkId, Chunk> =
            std::collections::HashMap::new();
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
