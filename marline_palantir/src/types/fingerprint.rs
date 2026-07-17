use super::chunk::Chunk;

#[derive(Copy, Clone, Default, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    hash: [u8; 32],
}

impl Fingerprint {
    pub fn new(hash: [u8; 32]) -> Self {
        Self { hash }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.hash
    }
}

pub trait FingerprintGenerator {
    fn generate(&self, chunk: &Chunk) -> Fingerprint;
}

pub struct Sha256FingerprintGenerator;
