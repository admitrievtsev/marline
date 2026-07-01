use crate::sbc_scrubber::{ClusterPoint, Clusters, EqCluster};
use crate::clusterer::Clusterer;
use crate::SBCHash;
use chunkfs::ClusteringMeasurements;
use std::collections::HashMap;

/// A clusterer that groups chunks based on exact hash matches or partial matches within a tolerance range.
///
/// `EqClusterer` uses the `EqCluster` trait to find clusters for chunks. It searches for
/// existing clusters with matching hashes, or creates new clusters when no match is found.
/// The clustering is based on the similarity of hash values, allowing for a configurable
/// number of mismatches (`match_range`).
///
/// # Algorithm Overview
///
/// The clustering process works as follows:
/// 1. For each chunk, search for an existing cluster with a matching hash within `match_range` tolerance
/// 2. If a matching cluster is found, add the chunk to that cluster
/// 3. If no matching cluster is found, create a new cluster with the chunk's hash as the key
/// 4. Track statistics about cluster sizes and distances
///
/// # Type Parameters
///
/// * `Hash` - The hash type implementing `SBCHash`.
///
/// # Example
///
/// ```
/// # use sbc_algorithm::clusterer::EqClusterer;
///
/// let mut clusterer = EqClusterer::new(2);
/// // Use clusterer.clusterize(...) to cluster chunks.
/// ```
pub struct EqClusterer {
    /// Maximum number of mismatches allowed when searching for a matching cluster.
    /// A value of 0 means only exact hash matches are allowed.
    match_range: usize,
}

impl Default for EqClusterer {
    /// Creates a new `EqClusterer` with a default match range of 0 (exact matches only).
    fn default() -> Self {
        Self::new(0)
    }
}

impl EqClusterer {
    /// Constructs a new `EqClusterer` with the specified match range.
    ///
    /// # Arguments
    ///
    /// * `match_range` - Maximum number of mismatches allowed when searching for a matching cluster.
    ///
    /// # Returns
    ///
    /// A new `EqClusterer` instance.
    pub fn new(match_range: usize) -> Self {
        EqClusterer { match_range }
    }
}

impl<Hash: SBCHash + std::fmt::Debug> Clusterer<Hash> for EqClusterer {
    /// Groups chunks into clusters based on hash similarity.
    ///
    /// This method processes each chunk and assigns it to an existing cluster if a match
    /// is found within the `match_range` tolerance, or creates a new cluster otherwise.
    /// It also collects statistics about the clustering process.
    ///
    /// # Arguments
    ///
    /// * `chunk_sbc_hash` - A vector of `ClusterPoint` items representing chunks and their hashes.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The clusters map, where each key is a hash and each value is a vector of chunk points
    /// - A `ClusteringMeasurements` struct with statistics about the clustering process
    fn clusterize<'a>(
        &mut self,
        chunk_sbc_hash: Vec<ClusterPoint<'a, Hash>>,
    ) -> (Clusters<'a, Hash>, ClusteringMeasurements) {
        let mut clusters: Clusters<Hash> = Clusters(HashMap::default());
        let mut cluster_stats = ClusterStats::new();

        for (sbc_hash, data_container) in chunk_sbc_hash {
            let cluster = clusters.partial_search(&sbc_hash, self.match_range);
            cluster.push((sbc_hash, data_container));

            self.update_cluster_stats(&mut cluster_stats);
        }

        cluster_stats.number_of_clusters = clusters.0.len();

        let clusterization_report = cluster_stats.into_measurements();

        (clusters, clusterization_report)
    }
}

impl EqClusterer {
    /// Updates cluster statistics for a newly processed chunk.
    ///
    /// # Arguments
    ///
    /// * `cluster_stats` - Mutable reference to cluster statistics.
    fn update_cluster_stats(&self, cluster_stats: &mut ClusterStats) {
        // Each chunk processed increments the total count
        cluster_stats.total_cluster_size += 1;

        // Note: We don't track detailed cluster statistics like GraphClusterer does
        // because EqClusterer groups chunks based on exact/near-exact hash matches,
        // not graph-based clustering with distances.
    }
}

/// Internal statistics tracking for clustering operations.
///
/// This struct maintains statistics about the clustering process, including
/// the total number of chunks processed and the number of clusters created.
///
/// Note: `EqClusterer` does not compute detailed cluster metrics like distances
/// between clusters or within clusters, as it groups chunks based on exact or
/// near-exact hash matches rather than graph-based clustering.
struct ClusterStats {
    /// Total number of chunks processed.
    total_cluster_size: usize,
    /// Number of unique clusters created.
    number_of_clusters: usize,
}

impl ClusterStats {
    /// Creates a new empty `ClusterStats`.
    fn new() -> Self {
        ClusterStats {
            total_cluster_size: 0,
            number_of_clusters: 0,
        }
    }

    /// Converts the statistics into a `ClusteringMeasurements` struct.
    ///
    /// # Returns
    ///
    /// A `ClusteringMeasurements` struct with the collected statistics.
    /// Note that detailed metrics are empty as they are not computed by `EqClusterer`.
    fn into_measurements(self) -> ClusteringMeasurements {
        ClusteringMeasurements {
            total_cluster_size: self.total_cluster_size,
            number_of_clusters: self.number_of_clusters,
            number_of_vertices_in_cluster: HashMap::new(),
            distance_to_vertices_in_cluster: HashMap::new(),
            distance_to_other_clusters: HashMap::new(),
            cluster_dedup_ratio: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: correct metrics for SBC with features

    /*
    fn generate_test_data() -> Vec<u8> {
        const TEST_DATA_SIZE: usize = 16000;
        (0..TEST_DATA_SIZE).map(|_| rand::random::<u8>()).collect()
    }

    #[test]
    fn scrub_should_return_correct_scrub_measurements_for_eq_clusterer() {
        let test_data = generate_test_data();
        let chunk_size = SizeParams::new(2 * 1024, 8 * 1024, 16 * 1024);

        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(decoder::GdeltaDecoder::new(false)),
            Box::new(SBCScrubber::new(
                hasher::AronovichHasher,
                EqClusterer,
                encoder::GdeltaEncoder::new(false),
            )),
            Sha256Hasher::default(),
        );

        let mut handle = fs
            .create_file("file".to_string(), SuperChunker::new(chunk_size))
            .unwrap();
        fs.write_to_file(&mut handle, &test_data).unwrap();
        fs.close_file(handle).unwrap();

        let scrub_report = fs.scrub().unwrap();

        let cluster_report = &scrub_report.clusterization_report;
        assert!(cluster_report.total_cluster_size > 0);
        assert!(cluster_report.number_of_clusters > 0);
        assert!(cluster_report
            .number_of_vertices_in_cluster
            .values()
            .all(|&v| v == 1));
        assert!(cluster_report.distance_to_vertices_in_cluster.is_empty());
        assert!(cluster_report
            .distance_to_other_clusters
            .values()
            .all(|v| !v.is_empty()));
        assert!(cluster_report
            .cluster_dedup_ratio
            .values()
            .all(|&v| v == 0.0));
    }
     */
}
