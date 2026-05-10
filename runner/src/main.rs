extern crate chunkfs;
extern crate sbc_algorithm;

use chunkfs::chunkers::{FastChunker, SizeParams, SuperChunker};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::FileSystem;
use sbc_algorithm::{clusterer, decoder, encoder, hasher};
use sbc_algorithm::{SBCMap, SBCScrubber};
use std::collections::HashMap;
use std::{fs, io};
use sbc_algorithm::encoder::{DdeltaEncoder, GdeltaEncoder, XdeltaEncoder};

fn main() -> io::Result<()> {
    let data = fs::read("/home/alexei/Work/chunkfs_eunner/src/files/kernels.tar")?;

    // According to Odess sizes
    let chunk_size = SizeParams::new(2 * 1024, 8 * 1024, 16 * 1024);

    let mut fs = FileSystem::new_with_scrubber(
        HashMap::default(),
        SBCMap::new(decoder::GdeltaDecoder::new(false)),
        Box::new(SBCScrubber::new(
            hasher::OdessHasher::default(),
            clusterer::EqClusterer,
            GdeltaEncoder::new(false),
        )),
        Sha256Hasher::default(),
    );
    let mut handle = fs.create_file("file".to_string(), FastChunker::new(chunk_size))?;
    fs.write_to_file(&mut handle, &data)?;
    fs.close_file(handle)?;

    let read_handle = fs.open_file_readonly("file")?;
    let read = fs.read_file_complete(&read_handle)?;

    let cdc_dedup_ratio = fs.cdc_dedup_ratio();
    println!("CDC finished");
    let res = fs.scrub()?;
    let sbc_dedup_ratio = fs.total_dedup_ratio();
    println!("CDC dedup ratio: {}", cdc_dedup_ratio);
    println!("SBC dedup ratio: {}", sbc_dedup_ratio);
    println!("ScrubMeasure: {:?}", res);
    assert_eq!(read.len(), data.len());
    Ok(())
}
