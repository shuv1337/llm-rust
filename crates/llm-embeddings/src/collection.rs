//! Embedding collections with SQLite storage.
//!
//! This module provides the `Collection` struct for storing and retrieving
//! embeddings in a SQLite database, with support for cosine similarity search.

use crate::migrations::run_embeddings_migrations;
use crate::provider::EmbeddingProvider;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, Mutex};

// ============================================================================
// Embedding Encoding/Decoding
// ============================================================================

/// Encode embedding vector as little-endian binary blob.
/// This matches the upstream Python implementation.
pub fn encode_embedding(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decode embedding from little-endian binary blob.
pub fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    let count = bytes.len() / 4;
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let offset = i * 4;
        let arr: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap_or([0; 4]);
        values.push(f32::from_le_bytes(arr));
    }
    values
}

/// Compute SHA256 hash of content for deduplication.
pub fn content_hash(content: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hasher.finalize().to_vec()
}

// ============================================================================
// Similarity Computation
// ============================================================================

/// Compute cosine similarity between two vectors.
/// Returns a value between -1 and 1, where 1 means identical direction.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot_product = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot_product += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denominator = norm_a.sqrt() * norm_b.sqrt();
    if denominator == 0.0 {
        return 0.0;
    }

    (dot_product / denominator) as f32
}

// ============================================================================
// Collection Entry
// ============================================================================

/// An entry in an embedding collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Unique identifier within the collection.
    pub id: String,
    /// Similarity score (only populated in search results).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Original text content (if stored).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional metadata JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Item to embed with metadata.
pub struct EmbedItem {
    /// Unique identifier.
    pub id: String,
    /// Text content to embed.
    pub content: String,
    /// Optional metadata.
    pub metadata: Option<serde_json::Value>,
}

impl EmbedItem {
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

// ============================================================================
// Collection
// ============================================================================

/// A collection of embeddings stored in SQLite.
pub struct Collection {
    /// Connection to the SQLite database.
    conn: Arc<Mutex<Connection>>,
    /// Collection name.
    name: String,
    /// Collection ID in the database.
    id: i64,
    /// Model ID used for this collection.
    model_id: String,
}

impl Collection {
    /// Open or create a collection in the specified database.
    ///
    /// If the collection exists, `model_id` is optional and will be read from the database.
    /// If creating a new collection, `model_id` is required.
    pub fn open<P: AsRef<Path>>(db_path: P, name: &str, model_id: Option<&str>) -> Result<Self> {
        let conn = open_embeddings_db(db_path.as_ref())?;
        Self::open_with_conn(Arc::new(Mutex::new(conn)), name, model_id)
    }

    /// Open or create a collection using an existing connection.
    pub fn open_with_conn(
        conn: Arc<Mutex<Connection>>,
        name: &str,
        model_id: Option<&str>,
    ) -> Result<Self> {
        // Run migrations
        {
            let c = conn.lock().unwrap();
            run_embeddings_migrations(&c)?;
        }

        // Try to find existing collection
        let existing: Option<(i64, String)> = {
            let c = conn.lock().unwrap();
            c.query_row(
                "SELECT id, model FROM collections WHERE name = ?1",
                params![name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("failed to query collection")?
        };

        match existing {
            Some((id, stored_model)) => {
                // Collection exists - use stored model_id
                let final_model_id = model_id.unwrap_or(&stored_model);
                if model_id.is_some() && model_id != Some(&stored_model) {
                    tracing::warn!(
                        "Collection '{}' was created with model '{}', ignoring provided model '{}'",
                        name,
                        stored_model,
                        final_model_id
                    );
                }
                Ok(Self {
                    conn,
                    name: name.to_string(),
                    id,
                    model_id: stored_model,
                })
            }
            None => {
                // Create new collection
                let model_id = model_id.ok_or_else(|| {
                    anyhow!("model_id is required when creating a new collection")
                })?;

                let id = {
                    let c = conn.lock().unwrap();
                    c.execute(
                        "INSERT INTO collections (name, model) VALUES (?1, ?2)",
                        params![name, model_id],
                    )
                    .context("failed to create collection")?;
                    c.last_insert_rowid()
                };

                Ok(Self {
                    conn,
                    name: name.to_string(),
                    id,
                    model_id: model_id.to_string(),
                })
            }
        }
    }

    /// Create an in-memory collection for testing.
    pub fn in_memory(name: &str, model_id: &str) -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to create in-memory database")?;
        Self::open_with_conn(Arc::new(Mutex::new(conn)), name, Some(model_id))
    }

    /// Returns the collection name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the collection ID.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// Returns the model ID used for this collection.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the number of embeddings in the collection.
    pub fn count(&self) -> Result<usize> {
        let c = self.conn.lock().unwrap();
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM embeddings WHERE collection_id = ?1",
                params![self.id],
                |row| row.get(0),
            )
            .context("failed to count embeddings")?;
        Ok(count as usize)
    }

    /// Check if a collection exists in the database.
    pub fn exists<P: AsRef<Path>>(db_path: P, name: &str) -> Result<bool> {
        let path = db_path.as_ref();
        if !path.exists() {
            return Ok(false);
        }
        let conn = open_embeddings_db(path)?;
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM collections WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(exists)
    }

    /// Embed and store content using the provided embedding provider.
    pub fn embed(
        &self,
        provider: &dyn EmbeddingProvider,
        id: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
        store_content: bool,
    ) -> Result<()> {
        // Check content hash to avoid re-embedding
        let hash = content_hash(content.as_bytes());

        if self.has_content_hash(&hash)? {
            // Content already embedded, just update the ID mapping if needed
            return self.upsert_entry(id, None, content, metadata, store_content, &hash);
        }

        // Generate embedding
        let result = provider.embed(content)?;
        self.upsert_entry(
            id,
            Some(&result.embedding),
            content,
            metadata,
            store_content,
            &hash,
        )
    }

    /// Store a pre-computed embedding.
    pub fn store(
        &self,
        id: &str,
        embedding: &[f32],
        content: Option<&str>,
        metadata: Option<serde_json::Value>,
    ) -> Result<()> {
        let hash = content
            .map(|c| content_hash(c.as_bytes()))
            .unwrap_or_default();
        let c = self.conn.lock().unwrap();

        let embedding_blob = encode_embedding(embedding);
        let metadata_json = metadata.map(|m| m.to_string());
        let now = chrono::Utc::now().timestamp();

        c.execute(
            r#"
            INSERT INTO embeddings (collection_id, id, embedding, content, content_hash, metadata, updated)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT (collection_id, id) DO UPDATE SET
                embedding = excluded.embedding,
                content = excluded.content,
                content_hash = excluded.content_hash,
                metadata = excluded.metadata,
                updated = excluded.updated
            "#,
            params![
                self.id,
                id,
                embedding_blob,
                content,
                hash,
                metadata_json,
                now,
            ],
        )
        .context("failed to store embedding")?;

        Ok(())
    }

    /// Embed and store multiple items efficiently.
    pub fn embed_multi(
        &self,
        provider: &dyn EmbeddingProvider,
        items: &[EmbedItem],
        store_content: bool,
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        // Separate items that need embedding from those already in DB
        let mut to_embed: Vec<(&EmbedItem, Vec<u8>)> = Vec::new();
        let mut already_embedded: Vec<(&EmbedItem, Vec<u8>)> = Vec::new();

        for item in items {
            let hash = content_hash(item.content.as_bytes());
            if self.has_content_hash(&hash)? {
                already_embedded.push((item, hash));
            } else {
                to_embed.push((item, hash));
            }
        }

        // Batch embed new content
        if !to_embed.is_empty() {
            let texts: Vec<&str> = to_embed
                .iter()
                .map(|(item, _)| item.content.as_str())
                .collect();
            let batch_size = provider.batch_size();

            for chunk_start in (0..texts.len()).step_by(batch_size) {
                let chunk_end = (chunk_start + batch_size).min(texts.len());
                let chunk_texts = &texts[chunk_start..chunk_end];
                let chunk_items = &to_embed[chunk_start..chunk_end];

                let results = provider.embed_multi(chunk_texts)?;

                for ((item, hash), result) in chunk_items.iter().zip(results.iter()) {
                    self.upsert_entry(
                        &item.id,
                        Some(&result.embedding),
                        &item.content,
                        item.metadata.clone(),
                        store_content,
                        hash,
                    )?;
                }
            }
        }

        // Update already-embedded items (just metadata/id mapping)
        for (item, hash) in already_embedded {
            self.upsert_entry(
                &item.id,
                None,
                &item.content,
                item.metadata.clone(),
                store_content,
                &hash,
            )?;
        }

        Ok(())
    }

    /// Find similar embeddings to the given query text.
    pub fn similar(
        &self,
        provider: &dyn EmbeddingProvider,
        query: &str,
        n: usize,
    ) -> Result<Vec<Entry>> {
        let result = provider.embed(query)?;
        self.similar_by_vector(&result.embedding, n, None)
    }

    /// Find similar embeddings to the given vector.
    pub fn similar_by_vector(
        &self,
        embedding: &[f32],
        n: usize,
        skip_id: Option<&str>,
    ) -> Result<Vec<Entry>> {
        let c = self.conn.lock().unwrap();

        let mut stmt = c.prepare(
            r#"
            SELECT id, embedding, content, metadata
            FROM embeddings
            WHERE collection_id = ?1
            "#,
        )?;

        let rows = stmt.query_map(params![self.id], |row| {
            let id: String = row.get(0)?;
            let emb_blob: Vec<u8> = row.get(1)?;
            let content: Option<String> = row.get(2)?;
            let metadata_json: Option<String> = row.get(3)?;
            Ok((id, emb_blob, content, metadata_json))
        })?;

        let mut scored: Vec<Entry> = Vec::new();

        for row in rows {
            let (id, emb_blob, content, metadata_json) = row?;

            if skip_id == Some(&id) {
                continue;
            }

            let stored_embedding = decode_embedding(&emb_blob);
            let score = cosine_similarity(embedding, &stored_embedding);

            let metadata = metadata_json.and_then(|s| serde_json::from_str(&s).ok());

            scored.push(Entry {
                id,
                score: Some(score),
                content,
                metadata,
            });
        }

        // Sort by score descending
        scored.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        scored.truncate(n);
        Ok(scored)
    }

    /// Find similar embeddings to an existing entry by ID.
    pub fn similar_by_id(&self, id: &str, n: usize) -> Result<Vec<Entry>> {
        let embedding = self
            .get_embedding(id)?
            .ok_or_else(|| anyhow!("Entry '{}' not found in collection", id))?;
        self.similar_by_vector(&embedding, n, Some(id))
    }

    /// Get the embedding vector for an entry.
    pub fn get_embedding(&self, id: &str) -> Result<Option<Vec<f32>>> {
        let c = self.conn.lock().unwrap();
        let emb_blob: Option<Vec<u8>> = c
            .query_row(
                "SELECT embedding FROM embeddings WHERE collection_id = ?1 AND id = ?2",
                params![self.id, id],
                |row| row.get(0),
            )
            .optional()
            .context("failed to get embedding")?;

        Ok(emb_blob.map(|blob| decode_embedding(&blob)))
    }

    /// Get an entry by ID.
    pub fn get(&self, id: &str) -> Result<Option<Entry>> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id, content, metadata FROM embeddings WHERE collection_id = ?1 AND id = ?2",
            params![self.id, id],
            |row| {
                let id: String = row.get(0)?;
                let content: Option<String> = row.get(1)?;
                let metadata_json: Option<String> = row.get(2)?;
                Ok(Entry {
                    id,
                    score: None,
                    content,
                    metadata: metadata_json.and_then(|s| serde_json::from_str(&s).ok()),
                })
            },
        )
        .optional()
        .context("failed to get entry")
    }

    /// Delete the collection and all its embeddings.
    pub fn delete(self) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "DELETE FROM embeddings WHERE collection_id = ?1",
            params![self.id],
        )?;
        c.execute("DELETE FROM collections WHERE id = ?1", params![self.id])?;
        Ok(())
    }

    // Internal helpers

    fn has_content_hash(&self, hash: &[u8]) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let exists: bool = c
            .query_row(
                "SELECT COUNT(*) > 0 FROM embeddings WHERE collection_id = ?1 AND content_hash = ?2",
                params![self.id, hash],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(exists)
    }

    fn upsert_entry(
        &self,
        id: &str,
        embedding: Option<&[f32]>,
        content: &str,
        metadata: Option<serde_json::Value>,
        store_content: bool,
        hash: &[u8],
    ) -> Result<()> {
        let c = self.conn.lock().unwrap();

        let stored_content = if store_content { Some(content) } else { None };
        let metadata_json = metadata.map(|m| m.to_string());
        let now = chrono::Utc::now().timestamp();

        if let Some(emb) = embedding {
            let embedding_blob = encode_embedding(emb);
            c.execute(
                r#"
                INSERT INTO embeddings (collection_id, id, embedding, content, content_hash, metadata, updated)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT (collection_id, id) DO UPDATE SET
                    embedding = excluded.embedding,
                    content = excluded.content,
                    content_hash = excluded.content_hash,
                    metadata = excluded.metadata,
                    updated = excluded.updated
                "#,
                params![
                    self.id,
                    id,
                    embedding_blob,
                    stored_content,
                    hash,
                    metadata_json,
                    now,
                ],
            )
            .context("failed to upsert embedding")?;
        } else {
            // Update without changing embedding
            c.execute(
                r#"
                INSERT INTO embeddings (collection_id, id, embedding, content, content_hash, metadata, updated)
                SELECT ?1, ?2, embedding, ?3, ?4, ?5, ?6
                FROM embeddings WHERE collection_id = ?1 AND content_hash = ?4
                ON CONFLICT (collection_id, id) DO UPDATE SET
                    content = excluded.content,
                    metadata = excluded.metadata,
                    updated = excluded.updated
                "#,
                params![
                    self.id,
                    id,
                    stored_content,
                    hash,
                    metadata_json,
                    now,
                ],
            )
            .context("failed to upsert entry")?;
        }

        Ok(())
    }
}

// ============================================================================
// Database Helpers
// ============================================================================

/// Open the embeddings database with proper settings.
fn open_embeddings_db(path: &Path) -> Result<Connection> {
    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    let conn = Connection::open(path)
        .with_context(|| format!("failed to open embeddings database: {}", path.display()))?;

    // Set pragmas
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        ",
    )
    .context("failed to set database pragmas")?;

    Ok(conn)
}

/// List all collections in a database.
pub fn list_collections<P: AsRef<Path>>(db_path: P) -> Result<Vec<(String, String)>> {
    let path = db_path.as_ref();
    if !path.exists() {
        return Ok(vec![]);
    }

    let conn = open_embeddings_db(path)?;
    run_embeddings_migrations(&conn)?;

    let mut stmt = conn.prepare("SELECT name, model FROM collections ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to list collections")
}

/// Delete a collection by name.
pub fn delete_collection<P: AsRef<Path>>(db_path: P, name: &str) -> Result<bool> {
    let path = db_path.as_ref();
    if !path.exists() {
        return Ok(false);
    }

    let conn = open_embeddings_db(path)?;
    run_embeddings_migrations(&conn)?;

    // Get collection ID
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM collections WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .optional()?;

    let Some(id) = id else {
        return Ok(false);
    };

    // Delete embeddings and collection
    conn.execute(
        "DELETE FROM embeddings WHERE collection_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;

    Ok(true)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_embedding() {
        let original = vec![1.0f32, 2.5, -3.0, 0.0, 1e-10];
        let encoded = encode_embedding(&original);
        let decoded = decode_embedding(&encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_content_hash() {
        let hash1 = content_hash(b"hello world");
        let hash2 = content_hash(b"hello world");
        let hash3 = content_hash(b"different");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 32); // SHA256 = 32 bytes
    }

    #[test]
    fn test_collection_in_memory() {
        let collection =
            Collection::in_memory("test", "text-embedding-3-small").expect("create collection");

        assert_eq!(collection.name(), "test");
        assert_eq!(collection.model_id(), "text-embedding-3-small");
        assert_eq!(collection.count().unwrap(), 0);
    }

    #[test]
    fn test_collection_store_and_retrieve() {
        let collection = Collection::in_memory("test", "model").expect("create collection");

        let embedding = vec![0.1, 0.2, 0.3];
        collection
            .store("item1", &embedding, Some("hello"), None)
            .expect("store");

        assert_eq!(collection.count().unwrap(), 1);

        let retrieved = collection.get_embedding("item1").expect("get");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.len(), 3);
        assert!((retrieved[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn test_collection_similar_by_vector() {
        let collection = Collection::in_memory("test", "model").expect("create collection");

        // Store some embeddings
        collection
            .store("a", &[1.0, 0.0, 0.0], Some("north"), None)
            .unwrap();
        collection
            .store("b", &[0.9, 0.1, 0.0], Some("north-ish"), None)
            .unwrap();
        collection
            .store("c", &[0.0, 1.0, 0.0], Some("east"), None)
            .unwrap();
        collection
            .store("d", &[-1.0, 0.0, 0.0], Some("south"), None)
            .unwrap();

        // Find similar to north
        let query = vec![1.0, 0.0, 0.0];
        let results = collection.similar_by_vector(&query, 2, None).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a"); // Most similar
        assert_eq!(results[1].id, "b"); // Second most similar
        assert!(results[0].score.unwrap() > results[1].score.unwrap());
    }

    #[test]
    fn test_collection_similar_by_id() {
        let collection = Collection::in_memory("test", "model").expect("create collection");

        collection.store("a", &[1.0, 0.0], None, None).unwrap();
        collection.store("b", &[0.9, 0.1], None, None).unwrap();
        collection.store("c", &[0.0, 1.0], None, None).unwrap();

        let results = collection.similar_by_id("a", 2).unwrap();

        assert_eq!(results.len(), 2);
        // Should not include "a" itself
        assert!(results.iter().all(|e| e.id != "a"));
        assert_eq!(results[0].id, "b"); // Most similar to "a"
    }

    #[test]
    fn test_collection_with_metadata() {
        let collection = Collection::in_memory("test", "model").expect("create collection");

        let metadata = serde_json::json!({"name": "Test", "count": 42});
        collection
            .store("item", &[1.0, 2.0], Some("content"), Some(metadata.clone()))
            .unwrap();

        let entry = collection.get("item").unwrap().unwrap();
        assert_eq!(entry.content, Some("content".to_string()));
        assert_eq!(entry.metadata, Some(metadata));
    }

    #[test]
    fn test_collection_upsert() {
        let collection = Collection::in_memory("test", "model").expect("create collection");

        collection
            .store("item", &[1.0, 2.0], Some("v1"), None)
            .unwrap();
        collection
            .store("item", &[3.0, 4.0], Some("v2"), None)
            .unwrap();

        assert_eq!(collection.count().unwrap(), 1);

        let entry = collection.get("item").unwrap().unwrap();
        assert_eq!(entry.content, Some("v2".to_string()));

        let emb = collection.get_embedding("item").unwrap().unwrap();
        assert_eq!(emb, vec![3.0, 4.0]);
    }

    #[test]
    fn test_collection_delete() {
        let collection = Collection::in_memory("test", "model").expect("create collection");

        collection.store("a", &[1.0], None, None).unwrap();
        collection.store("b", &[2.0], None, None).unwrap();
        assert_eq!(collection.count().unwrap(), 2);

        collection.delete().expect("delete");
        // Collection is consumed, can't access it anymore
    }

    #[test]
    fn test_embed_item() {
        let item = EmbedItem::new("id1", "hello world")
            .with_metadata(serde_json::json!({"tag": "greeting"}));

        assert_eq!(item.id, "id1");
        assert_eq!(item.content, "hello world");
        assert!(item.metadata.is_some());
    }

    #[test]
    fn test_entry_serialization() {
        let entry = Entry {
            id: "test".to_string(),
            score: Some(0.95),
            content: Some("hello".to_string()),
            metadata: Some(serde_json::json!({"key": "value"})),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"id\":\"test\""));
        assert!(json.contains("\"score\":0.95"));
        assert!(json.contains("\"content\":\"hello\""));

        // Entry without optional fields
        let entry2 = Entry {
            id: "minimal".to_string(),
            score: None,
            content: None,
            metadata: None,
        };

        let json2 = serde_json::to_string(&entry2).unwrap();
        assert!(!json2.contains("score"));
        assert!(!json2.contains("content"));
        assert!(!json2.contains("metadata"));
    }
}
