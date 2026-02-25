//! Embedding storage and similarity search for the Rust LLM CLI.
//!
//! This crate provides:
//!
//! - **Provider abstraction**: The [`EmbeddingProvider`] trait for generating
//!   vector embeddings, with built-in support for OpenAI's embedding models.
//!
//! - **Collection storage**: The [`Collection`] struct for storing embeddings
//!   in a SQLite database with content deduplication and metadata support.
//!
//! - **Similarity search**: Cosine similarity computation for finding similar
//!   embeddings in a collection.
//!
//! - **Schema compatibility**: Database migrations compatible with the upstream
//!   Python LLM project's embeddings.db format.
//!
//! # Quick Start
//!
//! ```ignore
//! use llm_embeddings::{Collection, OpenAIEmbeddingProvider, OpenAIEmbeddingConfig};
//!
//! // Create a provider
//! let config = OpenAIEmbeddingConfig {
//!     api_key: "your-api-key".to_string(),
//!     model: "text-embedding-3-small".to_string(),
//!     ..Default::default()
//! };
//! let provider = OpenAIEmbeddingProvider::new(config)?;
//!
//! // Open or create a collection
//! let collection = Collection::open("embeddings.db", "documents", Some("text-embedding-3-small"))?;
//!
//! // Embed and store content
//! collection.embed(&provider, "doc1", "Hello world", None, true)?;
//!
//! // Find similar documents
//! let results = collection.similar(&provider, "greeting", 5)?;
//! for entry in results {
//!     println!("{}: {:.3}", entry.id, entry.score.unwrap_or(0.0));
//! }
//! ```
//!
//! # Storage Format
//!
//! Embeddings are stored as little-endian binary sequences of 32-bit floats,
//! matching the upstream Python implementation. Use [`encode_embedding`] and
//! [`decode_embedding`] for conversion.

pub mod collection;
pub mod migrations;
pub mod provider;

// Re-export main types for convenience
pub use collection::{
    cosine_similarity, decode_embedding, delete_collection, encode_embedding, 
    list_collections, Collection, EmbedItem, Entry,
};
pub use migrations::{
    all_migrations as all_embeddings_migrations, list_applied_migrations,
    list_pending_migrations, run_embeddings_migrations, AppliedMigration, Migration,
};
pub use provider::{
    list_embedding_models, resolve_embedding_model, EmbeddingConfig, EmbeddingModelInfo,
    EmbeddingProvider, EmbeddingResult, OpenAIEmbeddingConfig, OpenAIEmbeddingProvider,
    BUILTIN_OPENAI_MODELS,
};

/// Returns the number of embeddings currently stored (always zero for now).
/// 
/// This is a compatibility shim for the old stub API.
#[deprecated(note = "Use Collection::count() instead")]
pub fn embedding_count() -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = vec![1.5f32, -2.0, 0.0, 3.14159];
        let encoded = encode_embedding(&original);
        let decoded = decode_embedding(&encoded);
        
        assert_eq!(original.len(), decoded.len());
        for (a, b) in original.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn test_cosine_similarity_normalized() {
        // Two normalized vectors
        let a = vec![0.6, 0.8]; // length 1
        let b = vec![0.8, 0.6]; // length 1
        
        let sim = cosine_similarity(&a, &b);
        // Expected: 0.6*0.8 + 0.8*0.6 = 0.96
        assert!((sim - 0.96).abs() < 1e-5);
    }

    #[test]
    fn test_collection_workflow() {
        // Create in-memory collection
        let collection = Collection::in_memory("test_collection", "test-model")
            .expect("create collection");

        // Initially empty
        assert_eq!(collection.count().unwrap(), 0);

        // Store some embeddings
        collection.store("a", &[1.0, 0.0, 0.0], Some("north"), None).unwrap();
        collection.store("b", &[0.0, 1.0, 0.0], Some("east"), None).unwrap();
        collection.store("c", &[0.0, 0.0, 1.0], Some("up"), None).unwrap();

        assert_eq!(collection.count().unwrap(), 3);

        // Find similar to [1, 0, 0]
        let results = collection.similar_by_vector(&[1.0, 0.0, 0.0], 2, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a"); // Exact match
        assert!(results[0].score.unwrap() > 0.99);
    }

    #[test]
    fn test_list_embedding_models() {
        let models = list_embedding_models();
        assert!(models.len() >= 3);
        
        // Check that all models have required fields
        for model in &models {
            assert!(!model.model_id.is_empty());
            assert_eq!(model.provider, "openai");
            assert!(model.dimensions.is_some());
        }
    }

    #[test]
    fn test_resolve_embedding_model_aliases() {
        assert_eq!(resolve_embedding_model("3-small"), Some("text-embedding-3-small"));
        assert_eq!(resolve_embedding_model("ada"), Some("text-embedding-ada-002"));
        assert_eq!(resolve_embedding_model("3-large"), Some("text-embedding-3-large"));
        
        // Case insensitive
        assert_eq!(resolve_embedding_model("3-SMALL"), Some("text-embedding-3-small"));
        
        // Unknown model
        assert_eq!(resolve_embedding_model("unknown-model"), None);
    }
}
