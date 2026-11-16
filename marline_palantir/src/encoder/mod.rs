mod ddelta;
mod gdelta;
mod xdelta;
mod zdelta;

pub use ddelta::DdeltaEncoder;
pub use gdelta::GdeltaEncoder;
pub use xdelta::XdeltaEncoder;
pub use zdelta::ZdeltaEncoder;

pub trait Encoder {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8>;
    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8>;

    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        manifest: &std::collections::HashMap<u64, u32>,
    ) -> Vec<u8> {
        let _ = manifest;
        self.encode(new_chunk, base_chunk)
    }

    fn generate_fp_table(&self, _chunk: &[u8]) -> Option<std::collections::HashMap<u64, u32>> {
        None
    }

    fn name(&self) -> &'static str;
}

impl<T: Encoder + ?Sized> Encoder for Box<T> {
    fn encode(&self, new_chunk: &[u8], base_chunk: &[u8]) -> Vec<u8> {
        (**self).encode(new_chunk, base_chunk)
    }

    fn decode(&self, base_chunk: &[u8], delta: &[u8]) -> Vec<u8> {
        (**self).decode(base_chunk, delta)
    }

    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        manifest: &std::collections::HashMap<u64, u32>,
    ) -> Vec<u8> {
        (**self).encode_with_manifest(new_chunk, base_chunk, manifest)
    }

    fn generate_fp_table(&self, chunk: &[u8]) -> Option<std::collections::HashMap<u64, u32>> {
        (**self).generate_fp_table(chunk)
    }

    fn name(&self) -> &'static str {
        (**self).name()
    }
}
