//! HNSW vector index for nearest-neighbor search.
//!
//! Wraps usearch for in-memory HNSW index. Built from embeddings
//! stored in SQLite; rebuilt on startup and after significant changes.

use crate::error::SearchError;
use usearch::ffi::{IndexOptions, MetricKind, ScalarKind};

/// In-memory HNSW vector index.
pub struct VectorIndex {
    index: usearch::Index,
    dim: usize,
    /// Number of live vectors (excludes tombstones).
    count: usize,
    /// Number of removed vectors (tombstones).
    tombstones: usize,
}

/// A search result with chunk id and distance.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Chunk id (matches the SQLite chunks.id).
    pub chunk_id: i64,
    /// Cosine similarity score (higher is better, 0..1 range).
    pub score: f32,
}

impl VectorIndex {
    /// Create an empty index with the given embedding dimension.
    pub fn new(dim: usize) -> Result<Self, SearchError> {
        let options = IndexOptions {
            dimensions: dim,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };
        let index = usearch::Index::new(&options)
            .map_err(|e| SearchError::Index(format!("failed to create index: {e}")))?;

        Ok(Self { index, dim, count: 0, tombstones: 0 })
    }

    /// Build index from a set of (chunk_id, embedding) pairs.
    pub fn build(dim: usize, data: &[(i64, Vec<f32>)]) -> Result<Self, SearchError> {
        let mut idx = Self::new(dim)?;
        if data.is_empty() {
            return Ok(idx);
        }

        idx.index
            .reserve(data.len())
            .map_err(|e| SearchError::Index(format!("failed to reserve: {e}")))?;

        for (chunk_id, embedding) in data {
            idx.index
                .add(*chunk_id as u64, embedding)
                .map_err(|e| SearchError::Index(format!("failed to add vector: {e}")))?;
        }
        idx.count = data.len();

        Ok(idx)
    }

    /// Add a single vector to the index.
    pub fn add(&mut self, chunk_id: i64, embedding: &[f32]) -> Result<(), SearchError> {
        if self.count + 1 > self.index.capacity() {
            let new_cap = (self.count + 1).next_power_of_two().max(1024);
            self.index
                .reserve(new_cap)
                .map_err(|e| SearchError::Index(format!("failed to reserve: {e}")))?;
        }
        self.index
            .add(chunk_id as u64, embedding)
            .map_err(|e| SearchError::Index(format!("failed to add vector: {e}")))?;
        self.count += 1;
        Ok(())
    }

    /// Mark a vector as removed (tombstone).
    ///
    /// usearch doesn't support true deletion, so we track tombstones
    /// and rebuild when the ratio exceeds a threshold.
    pub fn remove(&mut self, chunk_id: i64) -> Result<(), SearchError> {
        self.index
            .remove(chunk_id as u64)
            .map_err(|e| SearchError::Index(format!("failed to remove vector: {e}")))?;
        self.tombstones += 1;
        Ok(())
    }

    /// Search for nearest neighbors of a query embedding.
    ///
    /// Returns up to `limit` results sorted by descending similarity.
    pub fn search(&self, query: &[f32], limit: usize) -> Result<Vec<SearchResult>, SearchError> {
        if self.count == 0 {
            return Ok(Vec::new());
        }

        let results = self
            .index
            .search(query, limit)
            .map_err(|e| SearchError::Index(format!("search failed: {e}")))?;

        Ok(results
            .keys
            .iter()
            .zip(results.distances.iter())
            .map(|(&key, &distance)| SearchResult {
                chunk_id: key as i64,
                // usearch cosine metric returns distance (0 = identical),
                // convert to similarity score.
                score: 1.0 - distance,
            })
            .collect())
    }

    /// Number of vectors in the index.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether the index needs rebuilding (too many tombstones).
    pub fn needs_rebuild(&self) -> bool {
        self.count > 0 && self.tombstones * 20 > self.count
    }

    /// Reset tombstone counter (after a rebuild).
    pub fn reset_tombstones(&mut self) {
        self.tombstones = 0;
    }

    /// Embedding dimension.
    pub fn dim(&self) -> usize {
        self.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_vec(dim: usize, seed: u64) -> Vec<f32> {
        // Simple deterministic pseudo-random for tests.
        let mut v = Vec::with_capacity(dim);
        let mut x = seed;
        for _ in 0..dim {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            v.push((x as f32) / (u64::MAX as f32));
        }
        // Normalize.
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut v {
            *x /= norm;
        }
        v
    }

    #[test]
    fn build_and_search() {
        let dim = 64;
        let data: Vec<(i64, Vec<f32>)> =
            (1..=100).map(|i| (i, random_vec(dim, i as u64))).collect();

        let index = VectorIndex::build(dim, &data).unwrap();
        assert_eq!(index.len(), 100);

        // Search for the first vector — it should find itself.
        let results = index.search(&data[0].1, 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk_id, 1);
        assert!(results[0].score > 0.99);
    }

    #[test]
    fn empty_index_search() {
        let index = VectorIndex::new(64).unwrap();
        let results = index.search(&random_vec(64, 42), 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn incremental_add() {
        let dim = 32;
        let mut index = VectorIndex::new(dim).unwrap();

        for i in 1..=50 {
            index.add(i, &random_vec(dim, i as u64)).unwrap();
        }
        assert_eq!(index.len(), 50);

        let results = index.search(&random_vec(dim, 1), 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn tombstone_tracking() {
        let dim = 16;
        let mut index = VectorIndex::build(
            dim,
            &(1..=10).map(|i| (i, random_vec(dim, i as u64))).collect::<Vec<_>>(),
        )
        .unwrap();

        assert!(!index.needs_rebuild());

        // Remove >5% of vectors.
        index.remove(1).unwrap();
        assert!(index.needs_rebuild());

        index.reset_tombstones();
        assert!(!index.needs_rebuild());
    }
}
