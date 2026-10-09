use std::collections::HashMap;

pub trait ChunkHash: Clone + Eq + std::hash::Hash + Send + Sync + Default + 'static {}
impl ChunkHash  for [u8; 32] {}

pub trait Database<K, V> {
    fn insert(&mut self, key: K, value: V);
    fn get(&self, key: &K) -> Option<V>;
    fn conteins(&self, key: &K) -> bool {
        self.get(key).is_some()
    }
    fn remove(&mut self, key: &K) -> Option<V>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait IterableDatabase<K, V>: Database<K, V> {
    fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = (&'a K, &'a V)> + 'a>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryDatabase<K: std::hash::Hash + Eq, V> {
    inner: HashMap<K, V>,
    pub total_bytes: usize,
}

impl<K: std::hash::Hash + Eq, V> InMemoryDatabase<K, V> {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            total_bytes: 0,
        }
    }

    pub fn get_ref(&self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }
}

impl<K: ChunkHash, V: Clone> Database<K, V> for InMemoryDatabase<K, V> {
    fn insert(&mut self, key: K, value: V) {
        self.inner.insert(key, value);
    }

    fn get(&self, key: &K) -> Option<V> {
        self.inner.get(key).cloned()
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        self.inner.remove(key)
    }

    fn len(&self) -> usize {
        self.inner.len()
    }
}
    
    impl<K: ChunkHash, V: Clone + 'static> IterableDatabase<K, V> for InMemoryDatabase<K, V> {
        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = (&'a K, &'a V)> + 'a> {
            Box::new(self.inner.iter())
        }
    }


#[derive(Debug, Clone)]
pub struct MetricsDatabase<H: ChunkHash = [u8; 32]> {
    pub total_saved_bytes: usize,
    pub unique_chunks_count: usize,
    pub delta_chunks_count: usize,
    /// Храним базовые чанки, чтобы энкодер мог сжимать последующие чанки относительно них
    base_chunks: HashMap<H, Vec<u8>>,
}

impl<H: ChunkHash> Default for MetricsDatabase<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H: ChunkHash> MetricsDatabase<H> {
    pub fn new() -> Self {
        Self {
            total_saved_bytes: 0,
            unique_chunks_count: 0,
            delta_chunks_count: 0,
            base_chunks: HashMap::new(),
        }
    }

    /// Сохранение уникального базового чанка (сохраняем тело + учитываем размер)
    pub fn insert(&mut self, hash: H, data: &[u8]) {
        self.total_saved_bytes += data.len();
        self.unique_chunks_count += 1;
        self.base_chunks.insert(hash, data.to_vec());
    }

    /// Сохранение дельты (учитываем только размер сжатой дельты, экономим RAM)
    pub fn insert_delta(&mut self, _hash: H, delta: &[u8]) {
        self.total_saved_bytes += delta.len();
        self.delta_chunks_count += 1;
    }

    /// Получение базового чанка для дельта-сжатия
    pub fn get(&self, hash: &H) -> Option<Vec<u8>> {
        self.base_chunks.get(hash).cloned()
    }

    /// Получение ссылки на базовый чанк без лишнего клонирования
    pub fn get_ref(&self, hash: &H) -> Option<&[u8]> {
        self.base_chunks.get(hash).map(|v| v.as_slice())
    }

    pub fn contains(&self, hash: &H) -> bool {
        self.base_chunks.contains_key(hash)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create deterministic 32-byte chunk hashes
    fn make_hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    // =========================================================================
    // Tests for InMemoryDatabase
    // =========================================================================

    #[test]
    fn test_in_memory_db_basic_crud_and_trait_methods() {
        let mut db: InMemoryDatabase<[u8; 32], Vec<u8>> = InMemoryDatabase::new();

        // Check empty state
        assert!(db.is_empty(), "Newly created database should be empty");
        assert_eq!(db.len(), 0);

        let key1 = make_hash(1);
        let val1 = vec![10, 20, 30];

        // Insert first entry
        db.insert(key1, val1.clone());

        assert!(!db.is_empty(), "Database should not be empty after insert");
        assert_eq!(db.len(), 1);
        assert!(db.conteins(&key1), "Database should contain key1");

        // Verify retrieval via get() and get_ref()
        assert_eq!(db.get(&key1), Some(val1.clone()));
        assert_eq!(db.get_ref(&key1), Some(&val1));

        // Non-existent key check
        let missing_key = make_hash(99);
        assert!(!db.conteins(&missing_key));
        assert_eq!(db.get(&missing_key), None);
        assert_eq!(db.get_ref(&missing_key), None);

        // Remove entry
        let removed = db.remove(&key1);
        assert_eq!(removed, Some(val1), "Removed value should match the original");
        assert!(db.is_empty(), "Database should be empty after removing the only entry");
        assert!(!db.conteins(&key1));

        // Removing non-existent key returns None
        assert_eq!(db.remove(&key1), None);
    }

    #[test]
    fn test_in_memory_db_iteration() {
        let mut db: InMemoryDatabase<[u8; 32], u32> = InMemoryDatabase::default();

        let entries = [
            (make_hash(1), 100),
            (make_hash(2), 200),
            (make_hash(3), 300),
        ];

        for (k, v) in entries {
            db.insert(k, v);
        }

        assert_eq!(db.len(), 3);

        // Collect elements using IterableDatabase::iter
        let collected: HashMap<[u8; 32], u32> = db.iter().map(|(&k, &v)| (k, v)).collect();

        assert_eq!(collected.len(), 3, "Iterator should yield all items");
        for (k, v) in entries {
            assert_eq!(collected.get(&k), Some(&v));
        }
    }

    // =========================================================================
    // Tests for MetricsDatabase
    // =========================================================================

    #[test]
    fn test_metrics_db_initial_state() {
        let db: MetricsDatabase<[u8; 32]> = MetricsDatabase::default();

        assert_eq!(db.total_saved_bytes, 0);
        assert_eq!(db.unique_chunks_count, 0);
        assert_eq!(db.delta_chunks_count, 0);
    }

    #[test]
    fn test_metrics_db_insert_base_chunk() {
        let mut db: MetricsDatabase<[u8; 32]> = MetricsDatabase::new();

        let hash = make_hash(42);
        let payload = b"base chunk data payload";

        db.insert(hash, payload);

        // Verify metrics
        assert_eq!(db.total_saved_bytes, payload.len());
        assert_eq!(db.unique_chunks_count, 1);
        assert_eq!(db.delta_chunks_count, 0);

        // Verify chunk retrieval
        assert!(db.contains(&hash));
        assert_eq!(db.get(&hash), Some(payload.to_vec()));
        assert_eq!(db.get_ref(&hash), Some(payload.as_slice()));
    }

    #[test]
    fn test_metrics_db_insert_delta_chunk() {
        let mut db: MetricsDatabase<[u8; 32]> = MetricsDatabase::new();

        let delta_hash = make_hash(7);
        let delta_data = b"compressed delta diff";

        db.insert_delta(delta_hash, delta_data);

        // Verify metrics
        assert_eq!(db.total_saved_bytes, delta_data.len());
        assert_eq!(db.unique_chunks_count, 0);
        assert_eq!(db.delta_chunks_count, 1);

        // Delta chunks are NOT stored in base_chunks (RAM saving)
        assert!(!db.contains(&delta_hash), "Delta chunks should not be stored in base_chunks");
        assert_eq!(db.get(&delta_hash), None);
        assert_eq!(db.get_ref(&delta_hash), None);
    }

    #[test]
    fn test_metrics_db_combined_flow() {
        let mut db: MetricsDatabase<[u8; 32]> = MetricsDatabase::new();

        let base1 = make_hash(1);
        let base1_data = vec![0xAA; 100];

        let base2 = make_hash(2);
        let base2_data = vec![0xBB; 200];

        let delta1 = make_hash(3);
        let delta1_data = vec![0xCC; 30];

        db.insert(base1, &base1_data);
        db.insert(base2, &base2_data);
        db.insert_delta(delta1, &delta1_data);

        // Verify cumulative metrics
        assert_eq!(db.unique_chunks_count, 2);
        assert_eq!(db.delta_chunks_count, 1);
        assert_eq!(
            db.total_saved_bytes,
            base1_data.len() + base2_data.len() + delta1_data.len()
        );

        // Verify that base chunks exist and delta chunk is not kept in map
        assert!(db.contains(&base1));
        assert!(db.contains(&base2));
        assert!(!db.contains(&delta1));

        assert_eq!(db.get_ref(&base1), Some(base1_data.as_slice()));
        assert_eq!(db.get_ref(&base2), Some(base2_data.as_slice()));
    }
}