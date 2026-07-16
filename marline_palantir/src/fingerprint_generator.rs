use crate::types::{Chunk, Fingerprint, FingerprintGenerator, Sha256FingerprintGenerator};
use sha2::{Digest, Sha256};

impl FingerprintGenerator for Sha256FingerprintGenerator {
    fn generate(&self, chunk: &Chunk) -> Fingerprint {
        let mut hasher = Sha256::new();
        hasher.update(chunk.as_bytes());
        let result = hasher.finalize();
        let hash: [u8; 32] = result.into();
        Fingerprint::new(hash)
    }
}
