mod eq_clusterer;
mod graph_clusterer;

use crate::chunkfs_sbc::{ClusterPoint, Clusters};
use crate::SBCHash;
use chunkfs::ClusteringMeasurements;
pub use eq_clusterer::EqClusterer;
pub use graph_clusterer::GraphClusterer;
use std::collections::HashMap;

/// A trait defining the clustering behavior for similarity-based chunking.
///
/// The `Clusterer` trait groups chunks, identified by their similarity hashes,
/// into clusters of related chunks. This is a key step in similarity-based chunking
/// workflows to identify chunks that can be efficiently encoded as deltas.
///
/// # Type Parameters
///
/// * `Hash` - The hash type implementing `SBCHash` that identifies chunks.
///
/// # Methods
///
/// * `clusterize` - Takes a vector of chunk similarity points and returns clusters grouping them.
pub trait Clusterer<Hash: SBCHash> {
    /// Groups chunks into clusters based on their similarity hashes.
    ///
    /// # Arguments
    ///
    /// * `chunk_sbc_hash` - A vector of `ClusterPoint` items representing chunks and their hashes.
    ///
    /// # Returns
    ///
    /// A collection of clusters, where each cluster is a grouping of related chunks.
    fn clusterize<'a>(
        &mut self,
        chunk_sbc_hash: Vec<ClusterPoint<'a, Hash>>,
    ) -> (Clusters<'a, Hash>, ClusteringMeasurements);
}

/// Accepts a vector consisting of vertices between which it is necessary to calculate the distances.
/// Returns a table with a list of distances corresponding to each vertex
fn calculate_distance_to_other_vertices(vertices: Vec<u32>) -> HashMap<u32, Vec<usize>> {
    let mut distance_to_other_vertices = HashMap::new();

    for i in 0..vertices.len() {
        let mut distances = Vec::new();

        for j in 0..vertices.len() {
            if i != j {
                let distance = vertices[i].abs_diff(vertices[j]) as usize;
                distances.push(distance);
            }
        }

        distance_to_other_vertices.insert(vertices[i], distances);
    }

    distance_to_other_vertices
}
