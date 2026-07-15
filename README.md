<div align="center">

  # 🧹 Marline

  **Advanced Similarity-Based Chunking for Efficient Data Deduplication**

  [![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
  [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
  [![Build Status](https://github.com/yourusername/marline/workflows/Rust/badge.svg)](.github/workflows/rust.yml)

  [Documentation](https://docs.rs/marline) | [Examples](#examples) | [Contributing](#contributing)

</div>

---

## ✨ Features

Marline is a high-performance Rust library implementing **Similarity-Based Chunking (SBC)** algorithms for advanced data deduplication. Built on top of [ChunkFS](https://github.com/Piletskii-Oleg/chunkfs), it provides:

- 🎯 **Multiple Delta Encoders**: Gdelta, Xdelta, Zdelta, and Levenshtein encoders for optimal compression
- 📊 **Advanced Clustering**: Graph-based and equality clustering algorithms
- 🔐 **Robust Hashing**: Aronovich and Odess hashers for efficient similarity detection
- ⚡ **High Performance**: Parallel processing with Rayon for maximum throughput
- 🧩 **Modular Design**: Pluggable components for custom implementations
- 📈 **Deduplication Metrics**: Built-in CDC and SBC deduplication ratio tracking

## 📦 Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
marline_scrub = { git = "https://github.com/yourusername/marline.git" }
marline_sketcher = { git = "https://github.com/yourusername/marline.git" }
chunkfs = { git = "https://github.com/Piletskii-Oleg/chunkfs.git", features = ["chunkers", "hashers"] }
```

## 🚀 Quick Start

```rust
use chunkfs::chunkers::{SizeParams, SuperChunker};
use chunkfs::hashers::Sha256Hasher;
use chunkfs::FileSystem;
use marline_scrub::{clusterer, decoder, encoder, hasher};
use marline_scrub::{SBCMap, SBCScrubber};
use marline_sketcher::SBCHash;
use std::collections::HashMap;

fn main() -> std::io::Result<()> {
    // Create sample data
    let data = vec![10; 1024 * 1024];
    
    // Configure chunking parameters
    let chunk_size = SizeParams::new(2 * 1024, 8 * 1024, 16 * 1024);
    
    // Initialize filesystem with SBC scrubber
    let mut fs = FileSystem::new_with_scrubber(
        HashMap::default(),
        SBCMap::new(decoder::GdeltaDecoder::new(false)),
        Box::new(SBCScrubber::new(
            hasher::AronovichHasher,
            clusterer::GraphClusterer::default(),
            encoder::GdeltaEncoder::new(false),
        )),
        Sha256Hasher::default(),
    );
    
    // Write data to file
    let mut handle = fs.create_file("file".to_string(), SuperChunker::new(chunk_size))?;
    fs.write_to_file(&mut handle, &data)?;
    fs.close_file(handle)?;
    
    // Read back and verify
    let read_handle = fs.open_file_readonly("file")?;
    let read = fs.read_file_complete(&read_handle)?;
    
    // Perform scrubbing and get metrics
    let cdc_dedup_ratio = fs.cdc_dedup_ratio();
    let res = fs.scrub().unwrap();
    let sbc_dedup_ratio = fs.total_dedup_ratio();
    
    println!("CDC dedup ratio: {}", cdc_dedup_ratio);
    println!("SBC dedup ratio: {}", sbc_dedup_ratio);
    println!("Scrub results: {:?}", res);
    
    assert_eq!(read.len(), data.len());
    Ok(())
}
```

## 🏗️ Architecture

Marline is organized into several modular crates:

### [`marline_scrub`](marline_scrub/)
Core SBC implementation with:
- **Encoders**: Delta encoding algorithms (Gdelta, Xdelta, Zdelta, Levenshtein)
- **Decoders**: Corresponding delta decoders
- **Clusterers**: Graph-based and equality clustering
- **Scrubber**: Main SBC orchestration logic

### [`marline_sketcher`](marline_sketcher/)
Similarity detection and hashing:
- **Aronovich Hasher**: Fast similarity-based hashing
- **ODESS Hasher**: Advanced sketching algorithm
- **Broder's Method**: MinHash-based similarity estimation

### [`marline_delta`](marline_delta/)
Delta encoding utilities and shared components.

### [`marline`](marline/)
Example applications and benchmarks.

## 🔧 Configuration

### Encoders

| Encoder | Description | Use Case |
|---------|-------------|----------|
| `GdeltaEncoder` | General delta encoding | Most scenarios |
| `XdeltaEncoder` | Xdelta algorithm | Binary data |
| `ZdeltaEncoder` | Compressed delta | High compression needs |
| `LevenshteinEncoder` | Edit distance based | Text data |

### Clusterers

| Clusterer | Description | Performance |
|-----------|-------------|-------------|
| `GraphClusterer` | Graph-based clustering | High accuracy |
| `EqClusterer` | Equality-based clustering | Fast, simple |

### Hashers

| Hasher            | Description | Speed |
|-------------------|-------------|------|
| `AronovichHasher` | Similarity-based hash | Medium |
| `OdessHasher`     | Advanced sketching | Fast |

## 📊 Performance

Marline is designed for high-performance scenarios:

- **Parallel Processing**: Utilizes Rayon for multi-threaded operations
- **Memory Efficient**: Optimized data structures for minimal overhead
- **Zero-Copy**: Where possible, avoids unnecessary data copying

Benchmarks show significant improvements in deduplication ratios compared to traditional CDC (Content-Defined Chunking) approaches.

## 🧪 Testing

Run the test suite:

```bash
cargo test --workspace
```

Run with output:

```bash
cargo test --workspace -- --nocapture
```

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built on top of [ChunkFS](https://github.com/Piletskii-Oleg/chunkfs)
- Inspired by research in similarity-based deduplication
- Thanks to all contributors

## 📚 Resources

- [ChunkFS Documentation](https://github.com/Piletskii-Oleg/chunkfs)

---

<div align="center">

  **Built with ❤️ in Rust**

  [⬆ Back to top](#-marline)

</div>
