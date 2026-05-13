use crate::chunkfs_sbc::{ClusterPoint, Clusters};
use crate::clusterer::{calculate_distance_to_other_vertices, Clusterer};
use crate::SBCHash;
use chunkfs::ClusteringMeasurements;
use std::collections::HashMap;

/// A vertex in the graph used for clustering.
///
/// Each vertex tracks its parent for union-find operations during MST construction.
struct Vertex {
    /// The parent vertex key in the union-find structure.
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
/// # Details
///
/// The clustering is performed by assigning each chunk to a cluster represented by the root parent
/// found via union-find. The `set_parent_vertex` method attempts to find a nearby parent vertex
/// within the allowed edge weight to merge clusters.
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
    vertices: HashMap<u32, Vertex>,
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
    /// # Returns
    ///
    /// An empty `GraphClusterer`.
    pub fn new(_max_weight_edge: u32) -> GraphClusterer {
        GraphClusterer {
            max_weight_edge: _max_weight_edge,
            vertices: HashMap::new(),
        }
    }

    /// Finds the root parent of the given vertex key using path compression.
    ///
    /// # Arguments
    ///
    /// * `hash_set` - The vertex key to find the parent for.
    ///
    /// # Returns
    ///
    /// The root parent's key.
    fn find_set(&mut self, hash_set: u32) -> u32 {
        let parent = self.vertices.get(&hash_set).unwrap().parent;
        if hash_set != parent {
            let parent = self.find_set(parent);
            self.vertices.get_mut(&hash_set).unwrap().parent = parent;
            parent
        } else {
            parent
        }
    }

    /// Attempts to find a nearby parent vertex within `max_weight_edge` distance to cluster with.
    /// If no suitable parent is found, the vertex becomes its own parent.
    ///
    /// # Arguments
    ///
    /// * `hash` - The vertex key to assign a parent for.
    ///
    /// # Returns
    ///
    /// The parent vertex key assigned.
    fn set_parent_vertex(&mut self, hash: u32) -> u32 {
        let mut min_dist = u32::MAX;
        let mut parent_hash = hash;

        // Search in the range [hash - MAX_WEIGHT_EDGE, hash + MAX_WEIGHT_EDGE]
        let start = hash.saturating_sub(self.max_weight_edge);
        let end = hash.saturating_add(self.max_weight_edge);

        for other_hash in start..=end {
            if self.vertices.contains_key(&other_hash) {
                let other_parent_hash = self.find_set(other_hash);
                let dist = other_parent_hash.abs_diff(hash);
                if dist < min_dist && dist <= self.max_weight_edge {
                    min_dist = dist;
                    parent_hash = other_parent_hash;
                }
            }
        }

        self.vertices.insert(hash, Vertex::new(parent_hash));
        parent_hash
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
        let mut total_cluster_size = 0;
        let mut number_of_clusters = 0;
        let mut number_of_vertices_in_cluster = HashMap::new();
        let mut distance_to_vertices_in_cluster: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut parent_vertices: Vec<u32> = Vec::new();

        for (sbc_hash, data_container) in chunk_sbc_hash {
            total_cluster_size += 1;

            // Obtain u32 key for graph clustering from the hash
            let key = sbc_hash.get_key_for_graph_clusterer();

            // Find or assign the parent vertex for this key
            let parent_key = self.set_parent_vertex(key);

            number_of_vertices_in_cluster
                .entry(parent_key)
                .and_modify(|value| *value += 1)
                .or_insert(1);
            if key == parent_key {
                parent_vertices.push(key);
                distance_to_vertices_in_cluster.insert(key, vec![]);
                number_of_clusters += 1;
            } else {
                distance_to_vertices_in_cluster
                    .entry(parent_key)
                    .and_modify(|value| value.push(key.abs_diff(parent_key) as usize));
            }

            // Group the chunk into the cluster identified by the parent's hash
            let cluster = clusters
                .0
                .entry(Hash::new_with_u32(parent_key))
                .or_default();
            cluster.push((sbc_hash, data_container));
        }

        let distance_to_other_clusters = calculate_distance_to_other_vertices(parent_vertices);

        // Stub. The calculation cannot be performed at this stage.
        let cluster_dedup_ratio = HashMap::new();

        let clusterization_report = ClusteringMeasurements {
            total_cluster_size,
            number_of_clusters,
            number_of_vertices_in_cluster,
            distance_to_vertices_in_cluster,
            distance_to_other_clusters,
            cluster_dedup_ratio,
        };

        (clusters, clusterization_report)
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
