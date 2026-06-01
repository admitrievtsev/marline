use crate::chunkfs_sbc::{ClusterPoint, Clusters};
use crate::clusterer::{calculate_distance_to_other_vertices, Clusterer};
use crate::SBCHash;
use chunkfs::ClusteringMeasurements;
use std::collections::HashMap;

/// A vertex in the graph used for clustering.
///
/// Each vertex tracks its parent for union-find operations during MST construction.
/// In the union-find data structure, each vertex initially points to itself,
/// and vertices are merged by updating parent pointers.
struct Vertex {
    /// The parent vertex key in the union-find structure.
    /// If `parent == key`, this vertex is a root of its set.
    parent: u32,
}

impl Vertex {
    /// Creates a new vertex with itself as its own parent.
    ///
    /// # Arguments
    ///
    /// * `key` - The unique key identifying this vertex.
    ///
    /// # Returns
    ///
    /// A new `Vertex` instance.
    pub fn new(key: u32) -> Vertex {
        Vertex { parent: key }
    }
}

/// A clusterer that groups chunks using Kruskal's algorithm to build a minimum spanning tree (MST).
///
/// `GraphClusterer` uses a union-find data structure to cluster chunks based on their hash keys,
/// grouping chunks whose keys are close within a certain threshold (`max_weight_edge`).
///
/// # Algorithm Overview
///
/// The clustering process works as follows:
/// 1. Each chunk is converted to a u32 key via `get_key_for_graph_clusterer()`
/// 2. For each key, the algorithm searches for an existing parent vertex within `max_weight_edge` distance
/// 3. If found, the vertex is merged with that parent; otherwise, it becomes a new parent
/// 4. Union-find with path compression ensures efficient cluster management
///
/// # Type Parameters
///
/// * `Hash` - The hash type implementing `SBCHash`.
///
/// # Example
///
/// ```
/// # use sbc_algorithm::clusterer::GraphClusterer;
///
/// let mut clusterer = GraphClusterer::default();
/// // Use clusterer.clusterize(...) to cluster chunks.
/// ```
pub struct GraphClusterer {
    /// Map of vertex keys to their union-find vertex data.
    /// This structure maintains the parent relationships for all vertices
    /// in the clustering graph.
    vertices: HashMap<u32, Vertex>,

    /// Maximum allowed edge weight for clustering.
    /// Vertices with keys differing by more than this value will not be merged.
    max_weight_edge: u32,
}

impl Default for GraphClusterer {
    /// Creates a new, empty `GraphClusterer`.
    fn default() -> Self {
        Self::new(10)
    }
}

impl GraphClusterer {
    /// Constructs a new `GraphClusterer`.
    ///
    /// # Arguments
    ///
    /// * `max_weight_edge` - Maximum allowed edge weight for clustering.
    ///
    /// # Returns
    ///
    /// An empty `GraphClusterer`.
    pub fn new(max_weight_edge: u32) -> GraphClusterer {
        GraphClusterer {
            max_weight_edge,
            vertices: HashMap::new(),
        }
    }

    /// Finds the root parent of the given vertex key using path compression.
    ///
    /// This method implements the find operation of the union-find data structure
    /// with path compression optimization. Path compression flattens the structure
    /// of the tree, making future queries faster by pointing nodes directly to the root.
    ///
    /// # Arguments
    ///
    /// * `hash_set` - The vertex key to find the parent for.
    ///
    /// # Returns
    ///
    /// The root parent's key.
    ///
    /// # Panics
    ///
    /// Panics if the vertex key does not exist in the vertices map.
    #[allow(dead_code)]
    fn find_set(&mut self, hash_set: u32) -> u32 {
        let parent = self.vertices[&hash_set].parent;
        if hash_set != parent {
            let root = self.find_set(parent);
            self.vertices.get_mut(&hash_set).unwrap().parent = root;
            root
        } else {
            parent
        }
    }

    /// Attempts to find a nearby parent vertex within `max_weight_edge` distance to cluster with.
    /// If no suitable parent is found, the vertex becomes its own parent.
    ///
    /// This method is the core of the clustering algorithm. It searches for an existing
    /// cluster parent within the allowed distance threshold and either merges the new vertex
    /// with that cluster or creates a new cluster.
    ///
    /// # Arguments
    ///
    /// * `hash` - The vertex key to assign a parent for.
    ///
    /// # Returns
    ///
    /// The parent vertex key assigned.
    fn set_parent_vertex(&mut self, hash: u32) -> u32 {
        let parent_hash = self.find_nearest_parent(hash);
        self.vertices.insert(hash, Vertex::new(parent_hash));
        parent_hash
    }

    /// Finds the nearest parent vertex within the allowed distance range.
    ///
    /// This method searches the range `[hash - max_weight_edge, hash + max_weight_edge]`
    /// for existing vertices and selects the one with the minimum distance to the hash.
    /// The search considers the root parent of each vertex to ensure proper clustering.
    ///
    /// # Arguments
    ///
    /// * `hash` - The vertex key to find a parent for.
    ///
    /// # Returns
    ///
    /// The nearest parent vertex key, or the hash itself if no suitable parent is found.
    fn find_nearest_parent(&self, hash: u32) -> u32 {
        let mut min_dist = u32::MAX;
        let mut parent_hash = hash;

        let start = hash.saturating_sub(self.max_weight_edge);
        let end = hash.saturating_add(self.max_weight_edge);

        for other_hash in start..=end {
            if let Some(vertex) = self.vertices.get(&other_hash) {
                let other_parent_hash = self.find_root_parent(vertex.parent);
                let dist = other_parent_hash.abs_diff(hash);
                if dist < min_dist && dist <= self.max_weight_edge {
                    min_dist = dist;
                    parent_hash = other_parent_hash;
                }
            }
        }

        parent_hash
    }

    /// Finds the root parent of a vertex without path compression.
    ///
    /// This is a read-only version of `find_set` used during parent search.
    /// Unlike `find_set`, this method does not modify the parent pointers,
    /// making it safe to use during the search phase before a vertex is added.
    ///
    /// # Arguments
    ///
    /// * `parent_key` - The parent key to start searching from.
    ///
    /// # Returns
    ///
    /// The root parent's key.
    fn find_root_parent(&self, mut parent_key: u32) -> u32 {
        loop {
            if let Some(vertex) = self.vertices.get(&parent_key) {
                if parent_key == vertex.parent {
                    return parent_key;
                }
                parent_key = vertex.parent;
            } else {
                return parent_key;
            }
        }
    }
}

impl<Hash: SBCHash> Clusterer<Hash> for GraphClusterer {
    /// Clusters chunks by grouping them based on proximity of their hash keys using MST logic.
    ///
    /// # Arguments
    ///
    /// * `chunk_sbc_hash` - A vector of chunk points with their similarity hashes.
    ///
    /// # Returns
    ///
    /// A map of clusters keyed by the root hash, each containing grouped chunk points.
    fn clusterize<'a>(
        &mut self,
        chunk_sbc_hash: Vec<ClusterPoint<'a, Hash>>,
    ) -> (Clusters<'a, Hash>, ClusteringMeasurements) {
        let mut clusters: Clusters<Hash> = Clusters(HashMap::default());
        let mut cluster_stats = ClusterStats::new();
        let mut parent_vertices: Vec<u32> = Vec::new();

        for (sbc_hash, data_container) in chunk_sbc_hash {
            cluster_stats.total_cluster_size += 1;

            let key = sbc_hash.get_key_for_graph_clusterer();
            let parent_key = self.set_parent_vertex(key);

            self.update_cluster_stats(key, parent_key, &mut cluster_stats, &mut parent_vertices);

            self.add_to_cluster(&mut clusters, parent_key, sbc_hash, data_container);
        }

        let distance_to_other_clusters = calculate_distance_to_other_vertices(parent_vertices);
        let cluster_dedup_ratio = HashMap::new();

        let clusterization_report = ClusteringMeasurements {
            total_cluster_size: cluster_stats.total_cluster_size,
            number_of_clusters: cluster_stats.number_of_clusters,
            number_of_vertices_in_cluster: cluster_stats.number_of_vertices_in_cluster,
            distance_to_vertices_in_cluster: cluster_stats.distance_to_vertices_in_cluster,
            distance_to_other_clusters,
            cluster_dedup_ratio,
        };

        (clusters, clusterization_report)
    }
}

/// Internal statistics tracking for clustering operations.
struct ClusterStats {
    total_cluster_size: usize,
    number_of_clusters: usize,
    number_of_vertices_in_cluster: HashMap<u32, usize>,
    distance_to_vertices_in_cluster: HashMap<u32, Vec<usize>>,
}

impl ClusterStats {
    /// Creates a new empty `ClusterStats`.
    fn new() -> Self {
        ClusterStats {
            total_cluster_size: 0,
            number_of_clusters: 0,
            number_of_vertices_in_cluster: HashMap::new(),
            distance_to_vertices_in_cluster: HashMap::new(),
        }
    }
}

impl GraphClusterer {
    /// Updates cluster statistics for a newly processed vertex.
    ///
    /// # Arguments
    ///
    /// * `key` - The vertex key being processed.
    /// * `parent_key` - The parent vertex key assigned.
    /// * `cluster_stats` - Mutable reference to cluster statistics.
    /// * `parent_vertices` - Mutable reference to the list of parent vertices.
    fn update_cluster_stats(
        &self,
        key: u32,
        parent_key: u32,
        cluster_stats: &mut ClusterStats,
        parent_vertices: &mut Vec<u32>,
    ) {
        cluster_stats
            .number_of_vertices_in_cluster
            .entry(parent_key)
            .and_modify(|value| *value += 1)
            .or_insert(1);

        if key == parent_key {
            parent_vertices.push(key);
            cluster_stats
                .distance_to_vertices_in_cluster
                .insert(key, Vec::new());
            cluster_stats.number_of_clusters += 1;
        } else {
            cluster_stats
                .distance_to_vertices_in_cluster
                .entry(parent_key)
                .and_modify(|value| value.push(key.abs_diff(parent_key) as usize));
        }
    }

    /// Adds a chunk point to its assigned cluster.
    ///
    /// # Arguments
    ///
    /// * `clusters` - Mutable reference to the clusters map.
    /// * `parent_key` - The parent key identifying the cluster.
    /// * `sbc_hash` - The similarity hash of the chunk.
    /// * `data_container` - The data container associated with the chunk.
    fn add_to_cluster<'a, Hash: SBCHash>(
        &self,
        clusters: &mut Clusters<'a, Hash>,
        parent_key: u32,
        sbc_hash: Hash,
        data_container: &'a mut &'a mut chunkfs::DataContainer<crate::SBCKey<Hash>>,
    ) {
        let cluster = clusters
            .0
            .entry(Hash::new_with_u32(parent_key))
            .or_default();
        cluster.push((sbc_hash, data_container));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decoder, encoder, hasher, SBCMap, SBCScrubber};
    use chunkfs::chunkers::{SizeParams, SuperChunker};
    use chunkfs::hashers::Sha256Hasher;
    use chunkfs::{FileSystem, ScrubMeasurements};

    fn generate_test_data() -> Vec<u8> {
        const TEST_DATA_SIZE: usize = 32000;
        (0..TEST_DATA_SIZE).map(|_| rand::random::<u8>()).collect()
    }

    fn create_scrub_report(data: Vec<u8>) -> ScrubMeasurements {
        let chunk_size = SizeParams::new(2 * 1024, 8 * 1024, 16 * 1024);

        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(decoder::GdeltaDecoder::new(false)),
            Box::new(SBCScrubber::new(
                hasher::AronovichHasher,
                GraphClusterer::default(),
                encoder::GdeltaEncoder::new(false),
            )),
            Sha256Hasher::default(),
        );

        let mut handle = fs
            .create_file("file".to_string(), SuperChunker::new(chunk_size))
            .unwrap();
        fs.write_to_file(&mut handle, &data).unwrap();
        fs.close_file(handle).unwrap();
        fs.scrub().unwrap()
    }

    #[test]
    fn scrub_should_return_non_empty_scrub_measurements_for_graph_clusterer() {
        let test_data = generate_test_data();
        let scrub_report = create_scrub_report(test_data);

        let cluster_report = &scrub_report.clusterization_report;
        assert!(cluster_report.total_cluster_size > 0);
        assert!(cluster_report.number_of_clusters > 0);
        assert!(cluster_report
            .number_of_vertices_in_cluster
            .values()
            .all(|&v| v >= 1));
        assert!(!cluster_report.distance_to_vertices_in_cluster.is_empty());
        assert!(cluster_report
            .distance_to_other_clusters
            .values()
            .all(|v| !v.is_empty()));
    }

    #[test]
    fn scrub_should_return_scrub_measurements_with_correct_distance_to_vertices_in_cluster() {
        let test_data = generate_test_data();
        let scrub_report = create_scrub_report(test_data);

        let cluster_report = &scrub_report.clusterization_report;

        for (parent_key, &cluster_size) in &cluster_report.number_of_vertices_in_cluster {
            assert!(cluster_size > 0);

            let cluster_points = &scrub_report
                .clusterization_report
                .distance_to_vertices_in_cluster[parent_key];

            // The parent vertex is ignored.
            assert_eq!(cluster_points.len(), cluster_size - 1);
        }
    }

    #[test]
    fn total_cluster_size_matches_sum_of_cluster_vertices() {
        let test_data = generate_test_data();
        let scrub_report = create_scrub_report(test_data);
        let cluster_report = &scrub_report.clusterization_report;

        let sum_vertices = cluster_report.number_of_vertices_in_cluster.values().sum();

        assert_eq!(cluster_report.total_cluster_size, sum_vertices);
    }
}
