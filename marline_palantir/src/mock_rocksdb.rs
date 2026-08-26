use chunkfs::{Database, IterableDatabase};
use std::cell::Cell;
use std::collections::HashMap;
use std::io;

#[derive(Clone)]
pub struct Entry {
    manifest: Option<HashMap<u32, u32>>,
    data: Vec<u8>,
}

impl Entry {
    pub fn new(manifest: Option<HashMap<u32, u32>>, data: Vec<u8>) -> Self {
        Entry { manifest, data }
    }
    pub fn get_manifest(&self) -> &Option<HashMap<u32, u32>> {
        &self.manifest
    }
    pub fn get_data(&self) -> &Vec<u8> {
        &self.data
    }
}

impl From<Vec<u8>> for Entry {
    fn from(data: Vec<u8>) -> Self {
        Entry::new(None, data)
    }
}

pub struct MockRocksDBMap {
    inner: HashMap<[u8; 32], Entry>,
    pub get_count: Cell<usize>,
    pub insert_count: Cell<usize>,
    pub clear_count: Cell<usize>,
}

impl MockRocksDBMap {
    pub fn get_2(&self, key: &[u8; 32]) -> Option<&Entry> {
        self.inner.get(key)
    }
}

impl Database<[u8; 32], Entry> for MockRocksDBMap {
    fn insert(&mut self, key: [u8; 32], value: Entry) -> io::Result<()> {
        self.insert_count.set(self.insert_count.get() + 1);
        self.inner.insert(key, value);
        Ok(())
    }

    fn get(&self, key: &[u8; 32]) -> io::Result<Entry> {
        self.get_count.set(self.get_count.get() + 1);
        self.inner
            .get(key)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "key not found"))
    }

    fn contains(&self, key: &[u8; 32]) -> bool {
        self.inner.contains_key(key)
    }
}

impl IterableDatabase<[u8; 32], Entry> for MockRocksDBMap {
    fn iterator(&self) -> Box<dyn Iterator<Item = (&[u8; 32], &Entry)> + '_> {
        Box::new(self.inner.iter())
    }

    fn iterator_mut(&mut self) -> Box<dyn Iterator<Item = (&[u8; 32], &mut Entry)> + '_> {
        Box::new(self.inner.iter_mut())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.clear_count.set(self.clear_count.get() + 1);
        self.inner.clear();
        Ok(())
    }
}

impl Default for MockRocksDBMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRocksDBMap {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            get_count: Cell::new(0),
            insert_count: Cell::new(0),
            clear_count: Cell::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn total_bytes(&self) -> usize {
        self.inner.values().map(|v| v.data.len()).sum()
    }
}
