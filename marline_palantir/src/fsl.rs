//! Replay of FSL (File systems and Storage Lab) filesystem snapshots.
//!
//! The snapshots published at <https://tracer.filesystems.org/> contain **no file
//! content** — only a per-file record of the path, the size, the timestamps and the
//! list of content-defined chunk digests that `fs-hasher` computed at collection
//! time. To benchmark a scrubber on such a snapshot we therefore *replay* it: we
//! rebuild a byte corpus that reproduces the snapshot's observable structure.
//!
//! # What the replay reproduces exactly
//!
//! * the number of files and every file's logical size;
//! * the total logical byte count;
//! * the byte-identical file classes implied by the trace (two files whose chunk
//!   digest sequences are equal were byte-identical on disk);
//! * the aggregate density of repeated regions, because a chunk digest that occurs
//!   many times in the trace is materialised as the *same* byte string every time.
//!
//! # What the replay cannot reproduce
//!
//! * **Chunk-boundary alignment.** The trace stores the boundaries produced by the
//!   collection-time chunker (e.g. Rabin, min 32 KiB / avg 128 KiB / max 256 KiB),
//!   whereas the benchmark re-chunks with its own [`SizeParams`]. Repeated regions
//!   are still found, because content-defined chunking resynchronises, but not with
//!   the precision a native CDC run over the original bytes would achieve.
//! * **Similarity between distinct chunks.** The trace stores digests, not content,
//!   so it carries no information about how alike two *different* chunks are. This
//!   is precisely the signal Palantir consumes, so near-duplicates are injected
//!   explicitly via [`ReplayConfig::near_dup_rate`] and every reported gain is a
//!   function of that assumed rate rather than a measurement of the FSL data.
//!
//! # Trace format
//!
//! `fs-hasher` v7 (`libhashfile.h`), little-endian, all structs `__attribute__((packed))`:
//!
//! ```text
//! header_v4   : magic u32 | version u32 | files u64 | path_root [4096]
//!               chunks u64 | chnk_method u32 | params [44] | hsh_method u32
//!               hash_size u32 | sysid [4096] | start u64 | end u64 | bytes u64
//! file_header_v4 * files:
//!               size u64 | blocks u64 | uid u32 | gid u32 | perm u64
//!               atime u64 | mtime u64 | ctime u64 | nlink u64 | dev u64
//!               ino u64 | chunks u64 | pathlen u32 | tpathlen u32
//!               path [pathlen] | target [tpathlen]
//!               chunks * { size u32 | hash [hash_size/8] | cratio u8 }
//! ```
//!
//! Note that the `params` union occupies 44 bytes even though only the first 4
//! (fixed chunking) or 44 (variable chunking) are meaningful.
//!
//! Parsing is defensive in case a file is truncated by an interrupted download: the
//! header announces a file count, and if the stream ends early the parser stops
//! instead of failing, recording the loss in [`FslSnapshot::truncated_files`]. A
//! complete published trace parses to exactly the declared counts, so
//! `truncated_files == 0` is the signal that a download was not partial.
//!
//! Verified against `fslhomes-user002-2014-02-18.128kb.hash.anon`: 5211/5211 files,
//! 7325/7325 chunks, matching its `SHA1SUM` entry.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io;
use std::path::Path;

/// `HASH_FILE_MAGIC` from `libhashfile.h`.
const MAGIC: u32 = 0xDEADDEAD;
/// Highest `HASH_FILE_VERSION` this parser understands.
const VERSION: u32 = 7;
/// `sizeof(struct file_header_v4)`, excluding the two variable-length path fields.
const FILE_HEADER_LEN: usize = 96;

// Fixed offsets inside `header_v4`. Derived where possible so the 44-byte params union
// is stated once.
const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_FILES: usize = 8;
const OFF_PATH_ROOT: usize = 16;
const OFF_CHUNKS: usize = OFF_PATH_ROOT + 4096;
const OFF_CHNK_METHOD: usize = 4120;
const OFF_PARAMS: usize = 4124;
/// `sizeof(union cnk_method_params)` — the larger of the two chunking param structs.
const PARAMS_LEN: usize = 44;
const OFF_HSH_METHOD: usize = OFF_PARAMS + PARAMS_LEN;
const OFF_HASH_SIZE: usize = OFF_HSH_METHOD + 4;
const OFF_SYSID: usize = OFF_HASH_SIZE + 4;
const OFF_START_TIME: usize = OFF_SYSID + 4096;
const OFF_END_TIME: usize = OFF_START_TIME + 8;
const OFF_BYTES: usize = OFF_END_TIME + 8;
/// `sizeof(struct header_v4)`.
const HEADER_LEN: usize = OFF_BYTES + 8;

/// `sizeof(struct var_chnking_params)`: algo u32 | union[32] | min_csize u32 | max_csize u32.
const VAR_PARAMS_MIN_OFF: usize = OFF_PARAMS + 4 + 32;
const VAR_PARAMS_MAX_OFF: usize = VAR_PARAMS_MIN_OFF + 4;

/// A single chunk reference as recorded in a snapshot: the 48-bit digest plus the
/// exact byte length of the chunk in the original file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChunkRef {
    /// Anonymised chunk digest, widened from the on-disk 48 bits.
    pub digest: u64,
    /// Chunk length in bytes.
    pub size: u32,
}

impl ChunkRef {
    fn key(&self) -> (u64, u32) {
        (self.digest, self.size)
    }
}

/// One file record from a snapshot.
#[derive(Clone, Debug)]
pub struct FslFile {
    /// Anonymised path, kept only for extension statistics.
    pub path: Vec<u8>,
    /// Logical file size in bytes.
    pub size: u64,
    /// Chunks in file order. Empty when the collector recorded no chunks.
    pub chunks: Vec<ChunkRef>,
    /// True when the collector stored a non-zero size but no chunk records.
    pub size_without_chunks: bool,
}

impl FslFile {
    /// Lowercased extension of the anonymised path, or `None` if it has none.
    pub fn extension(&self) -> Option<String> {
        let name = self.path.rsplit(|&b| b == b'/').next()?;
        let dot = name.iter().rposition(|&b| b == b'.')?;
        if dot == 0 || dot + 1 == name.len() {
            return None;
        }
        let ext: Vec<u8> = name[dot + 1..].iter().map(|b| b.to_ascii_lowercase()).collect();
        Some(String::from_utf8_lossy(&ext).into_owned())
    }
}

/// A parsed FSL snapshot plus the statistics needed to judge a replay.
#[derive(Clone, Debug)]
pub struct FslSnapshot {
    pub files: Vec<FslFile>,
    /// `bytes` field of the header: total logical size claimed by the trace.
    pub declared_bytes: u64,
    /// `files` field of the header.
    pub declared_files: u64,
    /// `chunks` field of the header.
    pub declared_chunks: u64,
    /// Sum of the sizes of all chunk records actually read.
    pub sum_chunk_bytes: u64,
    /// Sum of the sizes of chunk records that occur exactly once.
    pub unique_chunk_bytes: u64,
    /// Number of distinct `(digest, size)` pairs.
    pub unique_chunks: u64,
    /// Chunk records read, including repeats.
    pub chunks_read: u64,
    /// Files that could not be parsed because the stream ended early.
    pub truncated_files: u64,
    /// Collector-side chunker bounds, when the trace used variable-size chunking.
    pub min_chunk: Option<u32>,
    pub max_chunk: Option<u32>,
    /// Digest width in bits (48 for the published `MD5_48BIT_HASH` traces).
    pub digest_bits: u32,
}

impl FslSnapshot {
    /// Reads and parses a `.hash.anon` file.
    pub fn parse(path: &Path) -> io::Result<Self> {
        let raw = std::fs::read(path)?;
        Self::parse_bytes(&raw)
    }

    /// Alias for [`FslSnapshot::parse`].
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::parse(path)
    }

    /// Multi-line human-readable summary, used by `--inspect`.
    pub fn describe(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "source            : fs-hasher v7 trace");
        let _ = writeln!(s, "digest_bits       : {}", self.digest_bits);
        match (self.min_chunk, self.max_chunk) {
            (Some(lo), Some(hi)) => {
                let _ = writeln!(s, "collector_chunking: variable, min {lo} max {hi} B");
            }
            _ => {
                let _ = writeln!(s, "collector_chunking: fixed/unknown");
            }
        }
        let _ = writeln!(
            s,
            "files             : {} parsed / {} declared",
            self.files.len(),
            self.declared_files
        );
        let _ = writeln!(
            s,
            "chunks            : {} read / {} declared ({} distinct (digest,size))",
            self.chunks_read, self.declared_chunks, self.unique_chunks
        );
        let _ = writeln!(
            s,
            "bytes             : {} parsed / {} declared ({:.2} MiB)",
            self.parsed_bytes(),
            self.declared_bytes,
            self.parsed_bytes() as f64 / (1 << 20) as f64
        );
        let _ = writeln!(
            s,
            "chunk_bytes       : total {} unique {} ({:.4} intrinsic ratio)",
            self.sum_chunk_bytes,
            self.unique_chunk_bytes,
            self.intrinsic_dedup_ratio()
        );
        let _ = writeln!(s, "truncated_files   : {}", self.truncated_files);
        let zero_chunk = self.files.iter().filter(|f| f.chunks.is_empty()).count();
        let _ = writeln!(s, "files_without_chunks: {zero_chunk}");
        let exts: BTreeMap<String, u64> =
            self.files.iter().filter_map(|f| f.extension()).fold(BTreeMap::new(), |mut m, e| {
                *m.entry(e).or_insert(0) += 1;
                m
            });
        let mut by_count: Vec<(&String, &u64)> = exts.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let top: Vec<String> = by_count.iter().take(10).map(|(k, v)| format!("{k}={v}")).collect();
        let _ = writeln!(s, "top_extensions    : {}", top.join(" "));
        s
    }

    /// Parses an in-memory `.hash.anon` image.
    pub fn parse_bytes(raw: &[u8]) -> io::Result<Self> {
        let bad = |what: &str| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("not a valid fs-hasher hash file: {what}"),
            )
        };

        if raw.len() < HEADER_LEN {
            return Err(bad("shorter than the v7 header"));
        }
        let magic = rd_u32(raw, OFF_MAGIC);
        if magic != MAGIC {
            return Err(bad(&format!("bad magic {magic:#010x}")));
        }
        let version = rd_u32(raw, OFF_VERSION);
        if version != VERSION {
            return Err(bad(&format!(
                "unsupported version {version}, this parser handles v{VERSION} only"
            )));
        }

        let declared_files = rd_u64(raw, OFF_FILES);
        let declared_chunks = rd_u64(raw, OFF_CHUNKS);
        let declared_bytes = rd_u64(raw, OFF_BYTES);
        let digest_bits = rd_u32(raw, OFF_HASH_SIZE);
        if digest_bits == 0 || digest_bits % 8 != 0 || digest_bits > 64 {
            return Err(bad(&format!("implausible digest width {digest_bits}")));
        }
        let digest_len = (digest_bits / 8) as usize;
        // v7: every chunk record is `size u32 | digest | cratio u8`.
        let chunk_rec_len = 4 + digest_len + 1;

        let variable_chunking = rd_u32(raw, OFF_CHNK_METHOD) == 2;
        let (min_chunk, max_chunk) = if variable_chunking {
            (Some(rd_u32(raw, VAR_PARAMS_MIN_OFF)), Some(rd_u32(raw, VAR_PARAMS_MAX_OFF)))
        } else {
            (None, None)
        };

        let mut cursor = Cursor::new(raw, HEADER_LEN);
        let mut files = Vec::with_capacity(declared_files.min(1 << 20) as usize);
        let mut uniq: HashMap<(u64, u32), u32> = HashMap::new();
        let mut sum_chunk_bytes = 0u64;
        let mut chunks_read = 0u64;
        let mut truncated_files = 0u64;

        for _ in 0..declared_files {
            match read_file(cursor.raw, cursor.off, digest_len, chunk_rec_len, &mut uniq) {
                Some((file, next)) => {
                    sum_chunk_bytes += file.chunks.iter().map(|c| c.size as u64).sum::<u64>();
                    chunks_read += file.chunks.len() as u64;
                    files.push(file);
                    cursor.off = next;
                }
                None => {
                    truncated_files = declared_files - files.len() as u64;
                    break;
                }
            }
        }

        let unique_chunk_bytes = uniq.values().map(|&s| s as u64).sum();
        let unique_chunks = uniq.len() as u64;
        drop(uniq);

        Ok(Self {
            files,
            declared_bytes,
            declared_files,
            declared_chunks,
            sum_chunk_bytes,
            unique_chunk_bytes,
            unique_chunks,
            chunks_read,
            truncated_files,
            min_chunk,
            max_chunk,
            digest_bits,
        })
    }

    /// Total logical size of the parsed files, from their `file_size` fields.
    pub fn parsed_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Dedup ratio the trace's own chunk graph would yield, computed independently of
    /// any benchmark: `sum of every chunk occurrence / sum of distinct chunk bytes`.
    ///
    /// This is an upper bound on what a CDC pass can achieve on this workload, since it
    /// assumes the collector's boundaries are reproduced exactly. Comparing it against
    /// the measured `cdc` column of a replay run shows how much of the trace's
    /// duplication potential the replay recovered.
    pub fn intrinsic_dedup_ratio(&self) -> f64 {
        if self.unique_chunk_bytes == 0 {
            return 0.0;
        }
        self.sum_chunk_bytes as f64 / self.unique_chunk_bytes as f64
    }

    /// Histogram of file sizes, as `(size, count)` pairs sorted by size.
    pub fn size_histogram(&self, buckets: usize) -> Vec<(u64, usize)> {
        let mut edges: Vec<u64> = vec![0; buckets + 1];
        let max = self.files.iter().map(|f| f.size).max().unwrap_or(0);
        for i in 1..=buckets {
            edges[i] = max * i as u64 / buckets as u64;
        }
        let mut counts = vec![0usize; buckets];
        for f in &self.files {
            let mut idx = buckets - 1;
            for i in 0..buckets {
                if f.size < edges[i + 1] {
                    idx = i;
                    break;
                }
            }
            counts[idx] += 1;
        }
        (0..buckets).map(|i| (edges[i], counts[i])).collect()
    }
}

/// Reads one `file_header_v4` plus its chunk records, or `None` if the stream ends
/// before the record is complete.
fn read_file(
    raw: &[u8],
    start: usize,
    digest_len: usize,
    chunk_rec_len: usize,
    uniq: &mut HashMap<(u64, u32), u32>,
) -> Option<(FslFile, usize)> {
    let mut o = start;
    if o + FILE_HEADER_LEN > raw.len() {
        return None;
    }
    let size = rd_u64(raw, o);
    o += 8; // file_size
    o += 8; // blocks
    o += 4; // uid
    o += 4; // gid
    o += 8; // perm
    o += 8; // atime
    o += 8; // mtime
    o += 8; // ctime
    o += 8; // hardlinks
    o += 8; // deviceid
    o += 8; // inodenum
    let n_chunks = rd_u64(raw, o);
    o += 8;
    let path_len = rd_u32(raw, o) as usize;
    o += 4;
    let target_len = rd_u32(raw, o) as usize;
    o += 4;
    if o + path_len + target_len > raw.len() {
        return None;
    }
    let path = raw[o..o + path_len].to_vec();
    o += path_len + target_len;

    // Guard against a corrupt `chunks` count asking for an absurd allocation.
    if n_chunks > (raw.len() / chunk_rec_len) as u64 {
        return None;
    }
    let rec_end = match o.checked_add((n_chunks as usize).checked_mul(chunk_rec_len)?) {
        Some(e) => e,
        None => return None,
    };
    if rec_end > raw.len() {
        return None;
    }

    let mut chunks = Vec::with_capacity(n_chunks as usize);
    for _ in 0..n_chunks {
        let csize = rd_u32(raw, o);
        o += 4;
        let mut digest = 0u64;
        for k in (0..digest_len).rev() {
            digest = (digest << 8) | raw[o + k] as u64;
        }
        o += digest_len;
        o += 1; // cratio

        let r = ChunkRef { digest, size: csize };
        // A digest repeated at a different length cannot be the same byte string;
        // counting on the pair keeps the intrinsic ratio honest.
        let e = uniq.entry(r.key()).or_insert(0);
        *e = csize;
        chunks.push(r);
    }

    let size_without_chunks = size > 0 && n_chunks == 0;
    Some((FslFile { path, size, chunks, size_without_chunks }, rec_end))
}

struct Cursor<'a> {
    raw: &'a [u8],
    off: usize,
}

impl<'a> Cursor<'a> {
    fn new(raw: &'a [u8], off: usize) -> Self {
        Self { raw, off }
    }
}

fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn rd_u64(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

/// Knobs for [`ReplayCorpus`].
#[derive(Clone, Debug)]
pub struct ReplayConfig {
    /// Fraction of files that are emitted as a mutated copy of an earlier file,
    /// i.e. as a near-duplicate. `0.0` reproduces exact duplicates only, which is
    /// what the trace alone justifies; Palantir can find nothing above CDC in that
    /// case because no two distinct chunks are alike.
    pub near_dup_rate: f64,
    /// Bytes overwritten in each near-duplicate. They are spread evenly across the
    /// file so that several chunks differ from their parent.
    pub mutation_bytes: usize,
    /// Seed for the deterministic content generator and for the injection draw.
    pub seed: u64,
    /// Stop after this many logical bytes.
    pub max_bytes: Option<u64>,
    /// Stop after this many files.
    pub max_files: Option<usize>,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            near_dup_rate: 0.0,
            mutation_bytes: 64,
            seed: 0x5EED_F512_10AD_BE,
            max_bytes: None,
            max_files: None,
        }
    }
}

/// What the replay actually produced, for the benchmark report.
#[derive(Clone, Debug, Default)]
pub struct ReplayStats {
    pub files: usize,
    pub near_dup_files: usize,
    pub logical_bytes: u64,
    /// Files dropped because they had no content in the trace and no size either.
    pub empty_files_skipped: usize,
    /// Files dropped by `max_bytes` / `max_files`.
    pub files_truncated_by_cap: usize,
    /// Digest-width note carried into the report.
    pub digest_bits: u32,
}

/// A replayed corpus: file contents reconstructed from a snapshot.
#[derive(Clone, Debug, Default)]
pub struct ReplayCorpus {
    pub files: Vec<Vec<u8>>,
    pub stats: ReplayStats,
}

impl ReplayCorpus {
    /// Materialises the snapshot as a byte corpus.
    pub fn build(snap: &FslSnapshot, cfg: &ReplayConfig) -> Self {
        let mut rng = Rng::new(cfg.seed);
        let mut corpus = ReplayCorpus {
            files: Vec::new(),
            stats: ReplayStats { digest_bits: snap.digest_bits, ..Default::default() },
        };

        let mut budget = cfg.max_bytes.unwrap_or(u64::MAX);
        // Indices of already-emitted files, reused as near-duplicate parents.
        let mut parents: Vec<usize> = Vec::new();

        for f in &snap.files {
            if f.size == 0 {
                corpus.stats.empty_files_skipped += 1;
                continue;
            }
            if let Some(cap) = cfg.max_files {
                if corpus.files.len() >= cap {
                    corpus.stats.files_truncated_by_cap += 1;
                    continue;
                }
            }
            if f.size > budget {
                corpus.stats.files_truncated_by_cap += 1;
                continue;
            }

            let near_dup = !parents.is_empty() && rng.unit() < cfg.near_dup_rate;
            let size = f.size as usize;
            let data = if near_dup {
                let p = parents[rng.below(parents.len())];
                derive_near_dup(&corpus.files[p], size, cfg.mutation_bytes, &mut rng)
            } else {
                synthesize(f, cfg.seed)
            };
            debug_assert_eq!(data.len(), size, "every replayed file matches its trace size");

            budget -= size as u64;
            corpus.stats.near_dup_files += near_dup as usize;
            parents.push(corpus.files.len());
            corpus.files.push(data);
        }

        corpus.stats.files = corpus.files.len();
        corpus.stats.logical_bytes = corpus.files.iter().map(|f| f.len() as u64).sum();
        corpus
    }
}

/// Rebuilds one file's bytes. A digest seen in the trace always yields the same byte
/// string, which is what makes the trace's repeated regions reappear in the corpus.
fn synthesize(file: &FslFile, salt: u64) -> Vec<u8> {
    if file.chunks.is_empty() {
        // The collector recorded a size but no chunks (unreadable file, or a
        // collector quirk). Keep the byte count exact so the workload size stays
        // faithful; the content is unique by construction and contributes nothing to
        // deduplication.
        let mut buf = vec![0u8; file.size as usize];
        fill_det(&mut buf, mix(salt ^ 0x9E37_79B9 ^ file.size));
        return buf;
    }
    let total: usize = file.chunks.iter().map(|c| c.size as usize).sum();
    let mut buf = Vec::with_capacity(total);
    for c in &file.chunks {
        let start = buf.len();
        buf.resize(start + c.size as usize, 0);
        // Keyed on the digest alone, so that two records sharing a digest also share
        // a byte prefix even when the collector recorded different lengths for them.
        fill_det(&mut buf[start..], mix(salt ^ c.digest.rotate_left(17)));
    }
    // The trace occasionally records chunk sizes that do not add up to `file_size`
    // (a dropped trailing chunk). Pad so the replayed size matches the trace.
    if (buf.len() as u64) < file.size {
        let start = buf.len();
        buf.resize(file.size as usize, 0);
        fill_det(&mut buf[start..], mix(salt ^ 0xA5A5_A5A5 ^ file.size));
    }
    buf
}

/// Produces a copy of `parent` resized to `target_len`, with `n` bytes overwritten at
/// evenly spaced offsets.
///
/// The length always matches the trace record even though the parent is an unrelated
/// file: a shorter parent is extended, a longer one truncated, both with deterministic
/// filler. The verbatim shared prefix is what matters, so Gdelta represents the result
/// with a few long match operations, the delta compresses far below `fp_threshold`, and
/// Palantir gets a realistic candidate.
fn derive_near_dup(parent: &[u8], target_len: usize, n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut child = parent.to_vec();
    child.resize(target_len, 0);
    if child.len() > parent.len() {
        // Only the tail past the parent's end is new; key it off the file it belongs to
        // so the extension is reproducible.
        fill_det(&mut child[parent.len()..], mix(0xC0FF_EE ^ target_len as u64));
    }
    let len = child.len();
    if len == 0 || n == 0 {
        return child;
    }
    // Overwrite at most 1/1024 of the file so a child stays a genuine near-duplicate.
    let n = n.min((len / 1024).max(1));
    let step = len / (n + 1);
    let jitter = rng.below(step.max(1));
    for i in 0..n {
        let pos = (i + 1) * step + jitter.min(step.saturating_sub(1));
        if pos >= len {
            break;
        }
        child[pos] = rng.byte();
    }
    child
}

/// splitmix64.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut x = z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Fills `buf` from a stream keyed by `seed`. Same `seed` and length always give the
/// same bytes, which is the whole point of the replay.
fn fill_det(buf: &mut [u8], seed: u64) {
    let mut s = seed;
    for b in buf.iter_mut() {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut x = s;
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
        *b = x as u8;
    }
}

/// Small deterministic PRNG, used only for the injection draw so that a given `seed`
/// always yields the same corpus.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(mix(seed))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }
    /// Uniform in `[0, n)`; returns 0 for `n == 0`.
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grows `b` to `o + 4` if needed, then stores `v` little-endian.
    fn put32(b: &mut Vec<u8>, o: usize, v: u32) {
        if b.len() < o + 4 {
            b.resize(o + 4, 0);
        }
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Grows `b` to `o + 8` if needed, then stores `v` little-endian.
    fn put64(b: &mut Vec<u8>, o: usize, v: u64) {
        if b.len() < o + 8 {
            b.resize(o + 8, 0);
        }
        b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// Builds a v7 image with `files` records, the last one optionally cut short.
    fn synth_image(n_files: u64, truncate_last: bool) -> Vec<u8> {
        let digest_len = 6usize;
        let rec = 4 + digest_len + 1;
        let mut raw = vec![0u8; HEADER_LEN];
        put32(&mut raw, OFF_MAGIC, MAGIC);
        put32(&mut raw, OFF_VERSION, VERSION);
        put64(&mut raw, OFF_FILES, n_files);
        put64(&mut raw, OFF_CHUNKS, 0);
        put32(&mut raw, OFF_CHNK_METHOD, 2);
        put32(&mut raw, OFF_PARAMS, 3);
        put32(&mut raw, VAR_PARAMS_MIN_OFF, 32768);
        put32(&mut raw, VAR_PARAMS_MAX_OFF, 262144);
        put32(&mut raw, OFF_HASH_SIZE, 48);
        put64(&mut raw, OFF_BYTES, 0);

        for i in 0..n_files {
            let mut h = Vec::new();
            put64(&mut h, 0, 100); // file_size, matches the 60 + 40 chunk sum below
            put64(&mut h, 8, 0); // blocks
            put32(&mut h, 16, 0); // uid
            put32(&mut h, 20, 0); // gid
            put64(&mut h, 24, 0o100644); // perm
            for k in 0..4 {
                put64(&mut h, 32 + 8 * k, 0); // atime, mtime, ctime, hardlinks
            }
            put64(&mut h, 64, 1); // deviceid
            put64(&mut h, 72, 1); // inodenum
            put64(&mut h, 80, 2); // chunks
            let path = format!("/a/b/c/file{i}.bin");
            put32(&mut h, 88, path.len() as u32);
            put32(&mut h, 92, 0);
            h.extend_from_slice(path.as_bytes());
            // two chunks: same digest in every file, different second digest
            for (d, s) in [(0xAAAAu64, 60u32), (0x1000 + i, 40)] {
                let at = h.len();
                put32(&mut h, at, s);
                let be = d.to_be_bytes();
                h.extend_from_slice(&be[2..8]);
                h.push(120);
            }
            raw.extend_from_slice(&h);
        }
        if truncate_last {
            let cut = rec + 4;
            raw.truncate(raw.len() - cut);
        }
        raw
    }

    #[test]
    fn parses_wellformed_image() {
        let raw = synth_image(3, false);
        let s = FslSnapshot::parse_bytes(&raw).unwrap();
        assert_eq!(s.files.len(), 3);
        assert_eq!(s.declared_files, 3);
        assert_eq!(s.truncated_files, 0);
        assert_eq!(s.files[0].size, 100);
        assert_eq!(s.files[0].chunks.len(), 2);
        assert_eq!(s.parsed_bytes(), 300);
        assert_eq!(s.files[0].extension().as_deref(), Some("bin"));
        // digest 0xAAAA is shared by all three files -> not unique
        assert_eq!(s.unique_chunks, 4);
        // 300 total, unique = 60 (shared) + 40 * 3
        assert_eq!(s.unique_chunk_bytes, 180);
        assert_eq!(s.min_chunk, Some(32768));
        assert_eq!(s.max_chunk, Some(262144));
    }

    #[test]
    fn survives_truncated_stream() {
        let raw = synth_image(3, true);
        let s = FslSnapshot::parse_bytes(&raw).unwrap();
        assert_eq!(s.files.len(), 2, "third record is cut short");
        assert_eq!(s.truncated_files, 1);
    }

    #[test]
    fn rejects_foreign_and_short_files() {
        assert!(FslSnapshot::parse_bytes(&[0u8; 16]).is_err());
        let mut raw = synth_image(1, false);
        put_magic(&mut raw, 0x1234);
        assert!(FslSnapshot::parse_bytes(&raw).is_err());
    }

    fn put_magic(raw: &mut [u8], v: u32) {
        raw[0..4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn replay_is_deterministic_and_reuses_digests() {
        let s = FslSnapshot::parse_bytes(&synth_image(4, false)).unwrap();
        let cfg = ReplayConfig { seed: 7, ..Default::default() };
        let a = ReplayCorpus::build(&s, &cfg);
        let b = ReplayCorpus::build(&s, &cfg);
        assert_eq!(a.files, b.files, "same seed must give identical bytes");
        assert_eq!(a.stats.logical_bytes, 400);
        // all files share the first 60-byte chunk verbatim
        let head: Vec<&[u8]> = a.files.iter().map(|f| &f[..60]).collect();
        assert!(head.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn near_dup_keeps_length_and_perturbs_bytes() {
        let s = FslSnapshot::parse_bytes(&synth_image(4, false)).unwrap();
        let none =
            ReplayCorpus::build(&s, &ReplayConfig { near_dup_rate: 0.0, ..Default::default() });
        let all = ReplayCorpus::build(
            &s,
            &ReplayConfig { near_dup_rate: 1.0, mutation_bytes: 8, ..Default::default() },
        );
        assert_eq!(none.stats.near_dup_files, 0);
        assert_eq!(all.stats.near_dup_files, 3, "first file has no parent");
        for (a, b) in none.files.iter().zip(&all.files) {
            assert_eq!(a.len(), b.len(), "near-duplicate keeps its length");
        }
        // The contract is not "differs from file 0" but "is a near-duplicate of
        // *some* earlier file", so assert that directly.
        for i in 1..all.files.len() {
            let near = (0..i).any(|j| {
                all.files[i].iter().zip(&all.files[j]).filter(|(x, y)| x != y).count() <= 8
            });
            assert!(near, "file {i} is not a near-duplicate of any earlier file");
        }
        // And it must actually be a *different* file from the same-size independent one.
        assert_ne!(all.files[1], none.files[1]);
    }

    #[test]
    fn pads_when_chunk_sizes_undercount_the_file() {
        // A dropped trailing chunk leaves sum(chunk sizes) < file_size; the replay must
        // still emit the full logical size.
        let mut raw = synth_image(1, false);
        // rewrite the single file's size field to 250
        put64(&mut raw, HEADER_LEN, 250);
        let s = FslSnapshot::parse_bytes(&raw).unwrap();
        assert_eq!(s.files[0].size, 250);
        assert_eq!(s.parsed_bytes(), 250);
        let c = ReplayCorpus::build(&s, &ReplayConfig::default());
        assert_eq!(c.files[0].len(), 250);
        assert_eq!(c.stats.logical_bytes, 250);
    }

    #[test]
    fn near_dups_match_their_own_trace_size_not_the_parents() {
        // Sizes here deliberately differ per file, so inheriting the parent's length
        // would be visible.
        let mut raw = synth_image(3, false);
        for i in 0..3 {
            // file i occupies HEADER_LEN + i*(FILE_HEADER_LEN + pathlen + 2*rec)
            let base = HEADER_LEN + i * (FILE_HEADER_LEN + "/a/b/c/file0.bin".len() + 2 * 11);
            put64(&mut raw, base, (100 + i * 7) as u64);
        }
        let s = FslSnapshot::parse_bytes(&raw).unwrap();
        let c = ReplayCorpus::build(
            &s,
            &ReplayConfig { near_dup_rate: 1.0, mutation_bytes: 0, ..Default::default() },
        );
        let sizes: Vec<usize> = c.files.iter().map(|f| f.len()).collect();
        assert_eq!(sizes, vec![100, 107, 114]);
        assert_eq!(c.stats.logical_bytes, 100 + 107 + 114);
    }

    #[test]
    fn caps_bound_the_corpus() {
        let s = FslSnapshot::parse_bytes(&synth_image(10, false)).unwrap();
        let c = ReplayCorpus::build(
            &s,
            &ReplayConfig { max_files: Some(4), max_bytes: Some(250), ..Default::default() },
        );
        assert_eq!(c.files.len(), 2, "250 bytes fits two 100-byte files");
        assert_eq!(c.stats.files_truncated_by_cap, 8);
    }

    #[test]
    fn intrinsic_ratio_reflects_shared_digests() {
        let s = FslSnapshot::parse_bytes(&synth_image(4, false)).unwrap();
        // 400 bytes total, 60 shared + 40*4 unique = 220
        assert!((s.intrinsic_dedup_ratio() - 400.0 / 220.0).abs() < 1e-9);
    }
}
