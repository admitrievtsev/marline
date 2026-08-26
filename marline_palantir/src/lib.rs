//! Implementation of the **Palantir** method for similarity-based delta encoding
//! in chunk-level deduplication systems.
//!
//! The Palantir method identifies similar chunks via multi-tier
//! **super-features** — compact, similarity-preserving digests computed from
//! chunk content.  When a newly arrived chunk matches a previously stored
//! chunk at any tier of the index, the chunk is stored as a delta relative to
//! its nearest neighbour, significantly reducing storage overhead.
//!
//! # Algorithm overview
//!
//! 1. **Super-feature generation** — [`sf_generator::PalantirHasher`] scans a
//!    chunk with a gear-hash rolling hash and computes a set of minimum-value
//!    features.  Features are grouped by tier and hashed into super-features.
//! 2. **Index lookup** — Super-features are split into fixed-size sketches
//!    and queried against a multi-tier [`metadata_manager::MetadataManager`].
//! 3. **Delta encoding** — If a similar base chunk is found, the encoder
//!    ([`encoder::PalantirEncoder`]) produces a delta; otherwise the raw chunk
//!    is stored.
//! 4. **Scrubbing** — [`palantir_scrubber::PalantirScrubber`] orchestrates
//!    the full pipeline (feature generation → lookup → delta-or-store) for
//!    every chunk in a storage backend.
//!
//! # Modules
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`types`] | Core data types: [`Chunk`], [`SuperFeature`], [`ChunkDigest`], [`TierConfig`] |
//! | [`sf_generator`] | Feature generation: [`PalantirHasher`] and the [`SuperFeatureGenerator`] trait |
//! | [`encoder`] | Delta encoding: [`PalantirEncoder`] trait and [`GdeltaEncoder`] |
//! | [`palantir_scrubber`] | Scrubbing pipeline: [`PalantirScrubber`] |
//! | [`error`] | Error types for the crate |
//! | [`metadata_manager`] | Metadata management: [`MetadataManager`] — fingerprint and super-feature index |
//!
//! # Quick start
//!
//! ```rust,ignore
//! use marline_palantir::sf_generator::PalantirHasher;
//! use marline_palantir::palantir_scrubber::PalantirScrubber;
//! use marline_palantir::encoder::GdeltaEncoder;
//! use marline_palantir::types::TierConfig;
//! use marline_palantir::lifecycle_manager::LifecycleManager;
//!
//! let sf_gen = PalantirHasher::new(7, vec![4, 3, 2]);
//! let encoder = GdeltaEncoder;
//! let tier_config = TierConfig::new([4, 3, 2]);
//! let lifecycle_configs = LifecycleManager::default_configs();
//! let mut scrubber = PalantirScrubber::new(sf_gen, encoder, tier_config, lifecycle_configs);
//! ```
//!
//! [`Chunk`]: types::Chunk
//! [`SuperFeature`]: types::SuperFeature
//! [`SuperFeatureGenerator`]: types::SuperFeatureGenerator
//! [`ChunkDigest`]: types::ChunkDigest
//! [`TierConfig`]: types::TierConfig
//! [`PalantirHasher`]: sf_generator::PalantirHasher
//! [`PalantirEncoder`]: encoder::PalantirEncoder
//! [`GdeltaEncoder`]: encoder::GdeltaEncoder
//! [`MetadataManager`]: metadata_manager::MetadataManager
//! [`PalantirScrubber`]: palantir_scrubber::PalantirScrubber
//!
pub mod encoder;
pub mod error;
mod fastcdc;
pub mod lifecycle_manager;
pub mod metadata_manager;
pub mod mock_rocksdb;
pub mod palantir_online;
pub mod palantir_scrubber;
pub mod sf_generator;
mod tables;
pub mod types;
pub mod utils;

// Gear table taken from https://github.com/nlfiedler/fastcdc-rs
#[rustfmt::skip]
pub(crate) const GEAR: [u64; 256] = [
    0x3b5d3c7d207e37dc, 0x784d68ba91123086, 0xcd52880f882e7298, 0xeacf8e4e19fdcca7,
    0xc31f385dfbd1632b, 0x1d5f27001e25abe6, 0x83130bde3c9ad991, 0xc4b225676e9b7649,
    0xaa329b29e08eb499, 0xb67fcbd21e577d58, 0x0027baaada2acf6b, 0xe3ef2d5ac73c2226,
    0x0890f24d6ed312b7, 0xa809e036851d7c7e, 0xf0a6fe5e0013d81b, 0x1d026304452cec14,
    0x03864632648e248f, 0xcdaacf3dcd92b9b4, 0xf5e012e63c187856, 0x8862f9d3821c00b6,
    0xa82f7338750f6f8a, 0x1e583dc6c1cb0b6f, 0x7a3145b69743a7f1, 0xabb20fee404807eb,
    0xb14b3cfe07b83a5d, 0xb9dc27898adb9a0f, 0x3703f5e91baa62be, 0xcf0bb866815f7d98,
    0x3d9867c41ea9dcd3, 0x1be1fa65442bf22c, 0x14300da4c55631d9, 0xe698e9cbc6545c99,
    0x4763107ec64e92a5, 0xc65821fc65696a24, 0x76196c064822f0b7, 0x485be841f3525e01,
    0xf652bc9c85974ff5, 0xcad8352face9e3e9, 0x2a6ed1dceb35e98e, 0xc6f483badc11680f,
    0x3cfd8c17e9cf12f1, 0x89b83c5e2ea56471, 0xae665cfd24e392a9, 0xec33c4e504cb8915,
    0x3fb9b15fc9fe7451, 0xd7fd1fd1945f2195, 0x31ade0853443efd8, 0x255efc9863e1e2d2,
    0x10eab6008d5642cf, 0x46f04863257ac804, 0xa52dc42a789a27d3, 0xdaaadf9ce77af565,
    0x6b479cd53d87febb, 0x6309e2d3f93db72f, 0xc5738ffbaa1ff9d6, 0x6bd57f3f25af7968,
    0x67605486d90d0a4a, 0xe14d0b9663bfbdae, 0xb7bbd8d816eb0414, 0xdef8a4f16b35a116,
    0xe7932d85aaaffed6, 0x08161cbae90cfd48, 0x855507beb294f08b, 0x91234ea6ffd399b2,
    0xad70cf4b2435f302, 0xd289a97565bc2d27, 0x8e558437ffca99de, 0x96d2704b7115c040,
    0x0889bbcdfc660e41, 0x5e0d4e67dc92128d, 0x72a9f8917063ed97, 0x438b69d409e016e3,
    0xdf4fed8a5d8a4397, 0x00f41dcf41d403f7, 0x4814eb038e52603f, 0x9dafbacc58e2d651,
    0xfe2f458e4be170af, 0x4457ec414df6a940, 0x06e62f1451123314, 0xbd1014d173ba92cc,
    0xdef318e25ed57760, 0x9fea0de9dfca8525, 0x459de1e76c20624b, 0xaeec189617e2d666,
    0x126a2c06ab5a83cb, 0xb1321532360f6132, 0x65421503dbb40123, 0x2d67c287ea089ab3,
    0x6c93bff5a56bd6b6, 0x4ffb2036cab6d98d, 0xce7b785b1be7ad4f, 0xedb42ef6189fd163,
    0xdc905288703988f6, 0x365f9c1d2c691884, 0xc640583680d99bfe, 0x3cd4624c07593ec6,
    0x7f1ea8d85d7c5805, 0x014842d480b57149, 0x0b649bcb5a828688, 0xbcd5708ed79b18f0,
    0xe987c862fbd2f2f0, 0x982731671f0cd82c, 0xbaf13e8b16d8c063, 0x8ea3109cbd951bba,
    0xd141045bfb385cad, 0x2acbc1a0af1f7d30, 0xe6444d89df03bfdf, 0xa18cc771b8188ff9,
    0x9834429db01c39bb, 0x214add07fe086a1f, 0x8f07c19b1f6b3ff9, 0x56a297b1bf4ffe55,
    0x94d558e493c54fc7, 0x40bfc24c764552cb, 0x931a706f8a8520cb, 0x32229d322935bd52,
    0x2560d0f5dc4fefaf, 0x9dbcc48355969bb6, 0x0fd81c3985c0b56a, 0xe03817e1560f2bda,
    0xc1bb4f81d892b2d5, 0xb0c4864f4e28d2d7, 0x3ecc49f9d9d6c263, 0x51307e99b52ba65e,
    0x8af2b688da84a752, 0xf5d72523b91b20b6, 0x6d95ff1ff4634806, 0x562f21555458339a,
    0xc0ce47f889336346, 0x487823e5089b40d8, 0xe4727c7ebc6d9592, 0x5a8f7277e94970ba,
    0xfca2f406b1c8bb50, 0x5b1f8a95f1791070, 0xd304af9fc9028605, 0x5440ab7fc930e748,
    0x312d25fbca2ab5a1, 0x10f4a4b234a4d575, 0x90301d55047e7473, 0x3b6372886c61591e,
    0x293402b77c444e06, 0x451f34a4d3e97dd7, 0x3158d814d81bc57b, 0x034942425b9bda69,
    0xe2032ff9e532d9bb, 0x62ae066b8b2179e5, 0x9545e10c2f8d71d8, 0x7ff7483eb2d23fc0,
    0x00945fcebdc98d86, 0x8764bbbe99b26ca2, 0x1b1ec62284c0bfc3, 0x58e0fcc4f0aa362b,
    0x5f4abefa878d458d, 0xfd74ac2f9607c519, 0xa4e3fb37df8cbfa9, 0xbf697e43cac574e5,
    0x86f14a3f68f4cd53, 0x24a23d076f1ce522, 0xe725cd8048868cc8, 0xbf3c729eb2464362,
    0xd8f6cd57b3cc1ed8, 0x6329e52425541577, 0x62aa688ad5ae1ac0, 0x0a242566269bf845,
    0x168b1a4753aca74b, 0xf789afefff2e7e3c, 0x6c3362093b6fccdb, 0x4ce8f50bd28c09b2,
    0x006a2db95ae8aa93, 0x975b0d623c3d1a8c, 0x18605d3935338c5b, 0x5bb6f6136cad3c71,
    0x0f53a20701f8d8a6, 0xab8c5ad2e7e93c67, 0x40b5ac5127acaa29, 0x8c7bf63c2075895f,
    0x78bd9f7e014a805c, 0xb2c9e9f4f9c8c032, 0xefd6049827eb91f3, 0x2be459f482c16fbd,
    0xd92ce0c5745aaa8c, 0x0aaa8fb298d965b9, 0x2b37f92c6c803b15, 0x8c54a5e94e0f0e78,
    0x95f9b6e90c0a3032, 0xe7939faa436c7874, 0xd16bfe8f6a8a40c9, 0x44982b86263fd2fa,
    0xe285fb39f984e583, 0x779a8df72d7619d3, 0xf2d79a8de8d5dd1e, 0xd1037354d66684e2,
    0x004c82a4e668a8e5, 0x31d40a7668b044e6, 0xd70578538bd02c11, 0xdb45431078c5f482,
    0x977121bb7f6a51ad, 0x73d5ccbd34eff8dd, 0xe437a07d356e17cd, 0x47b2782043c95627,
    0x9fb251413e41d49a, 0xccd70b60652513d3, 0x1c95b31e8a1b49b2, 0xcae73dfd1bcb4c1b,
    0x34d98331b1f5b70f, 0x784e39f22338d92f, 0x18613d4a064df420, 0xf1d8dae25f0bcebe,
    0x33f77c15ae855efc, 0x3c88b3b912eb109c, 0x956a2ec96bafeea5, 0x1aa005b5e0ad0e87,
    0x5500d70527c4bb8e, 0xe36c57196421cc44, 0x13c4d286cc36ee39, 0x5654a23d818b2a81,
    0x77b1dc13d161abdc, 0x734f44de5f8d5eb5, 0x60717e174a6c89a2, 0xd47d9649266a211e,
    0x5b13a4322bb69e90, 0xf7669609f8b5fc3c, 0x21e6ac55bedcdac9, 0x9b56b62b61166dea,
    0xf48f66b939797e9c, 0x35f332f9c0e6ae9a, 0xcc733f6a9a878db0, 0x3da161e41cc108c2,
    0xb7d74ae535914d51, 0x4d493b0b11d36469, 0xce264d1dfba9741a, 0xa9d1f2dc7436dc06,
    0x70738016604c2a27, 0x231d36e96e93f3d5, 0x7666881197838d19, 0x4a2a83090aaad40c,
    0xf1e761591668b35d, 0x7363236497f730a7, 0x301080e37379dd4d, 0x502dea2971827042,
    0xc2c5eb858f32625f, 0x786afb9edfafbdff, 0xdaee0d868490b2a4, 0x617366b3268609f6,
    0xae0e35a0fe46173e, 0xd1a07de93e824f11, 0x079b8b115ea4cca8, 0x93a99274558faebb,
    0xfb1e6e22e08a03b3, 0xea635fdba3698dd0, 0xcf53659328503a5c, 0xcde3b31e6fd5d780,
    0x8e3e4221d3614413, 0xef14d0d86bf1a22c, 0xe1d830d3f16c5ddb, 0xaabd2b2a451504e1
];

// Gear table taken from https://github.com/nlfiedler/fastcdc-rs
#[rustfmt::skip]
pub const GEARU32: [u32; 256] = [
    0xAD12E0BB, 0x357DD2B9, 0xFD630473, 0xC4F9B2C1, 0x1F4ED49B, 0xE458DB03, 0xC3AE7455, 0x20FE3BA3,
    0x9C18477B, 0xC7C7AA07, 0x85B4E23F, 0x16C4B43D, 0x75092721, 0xB9DD3A91, 0x4E43540B, 0x4BA372E3,
    0x326000B9, 0x149353D3, 0x7675EFA7, 0xE3445045, 0x164C2519, 0xCAD0E223, 0x96DE811B, 0x6C057BFB,
    0xB5B3E78B, 0x5416269F, 0x90CB1983, 0x4CF479AF, 0xA35944AD, 0x862A4F8D, 0xCD5D5D47, 0x55C960C9,
    0xBB17592F, 0x98D78C05, 0x8858D561, 0xC332FE35, 0xB6836A19, 0xC3D339C3, 0x82F67CD7, 0xF45D4F6D,
    0x69CC9BB5, 0xEFE02D4B, 0xEC59DE23, 0x9682ACA7, 0x5312006B, 0x53679CE9, 0x7D63D90D, 0x31EC795F,
    0xBCDDD457, 0x08B43749, 0x95A6CDAF, 0x3D584237, 0x56D453AD, 0x88EBFD9B, 0x2EB22229, 0x741A6529,
    0x818ED83D, 0xE7D55013, 0xF2DF157B, 0xCA918829, 0x57219299, 0x9129F0E3, 0x8DA9D3AB, 0x74EF104F,
    0x1363E63F, 0xA1F559CF, 0x82D67E79, 0xF6B02563, 0x1C111649, 0xA02F7995, 0xB8DBEF1F, 0xFB07C54B,
    0xEDAD97C3, 0x26FF3795, 0x93A32325, 0xC28AC315, 0x95156E41, 0xF5DDFC1B, 0x5F6DD21D, 0xD7902725,
    0x492D5381, 0x10A21A4B, 0x0E5659CD, 0x51BDB6D1, 0x3C7F23A3, 0x13200EE1, 0xB0034795, 0x8BD1B1C1,
    0x7F3E0741, 0xC2F012D3, 0x31DF5AAD, 0x055B00E1, 0x8AA3C1B9, 0xA0DD0639, 0x4DD117C3, 0x47E875FF,
    0xF21B4CB3, 0xFCC20983, 0xBDB76F13, 0xA1A303BF, 0xED93AB6B, 0xC0125A49, 0xD1480D99, 0x41A717D5,
    0x302B5809, 0x6C95542F, 0xBB1C1A35, 0x7F6A0F5F, 0x033639D1, 0x2D624CAB, 0xB8D9539B, 0x0EA8E511,
    0x2E0A6857, 0xF21397B7, 0xC4E5C8C9, 0xC442186B, 0xDC292D9F, 0x9EA28187, 0x28485761, 0xC4982F7B,
    0x90338157, 0x39EF86DB, 0x312C192F, 0xFCAB464D, 0xB38F5219, 0x66F4DBA5, 0x5B4B6877, 0x4E0C8CD1,
    0xFDD2F3BB, 0x75234991, 0x43B860BD, 0x839470B3, 0x7EAEED9B, 0x7E61F77F, 0xB74C6BFD, 0xCAB38B7F,
    0x56E1A70F, 0xF16C9005, 0xA2F97F2B, 0xB6776771, 0xB5C42F8F, 0x2E87EB5D, 0xA16E51DD, 0x3824B7A7,
    0x67F9A6A3, 0x826C28DD, 0x6C3122C3, 0x2C5D2F2B, 0xF35769D3, 0xE7B238B5, 0x95873F93, 0xCEAA59CD,
    0xBCBE7F9B, 0x899A459F, 0xA7A6BDF5, 0x76901F69, 0xCCBC65A5, 0x44EEAF95, 0x3215B77D, 0x9C508977,
    0xF7F4B15D, 0x7E205FED, 0x836ECBA7, 0x8C4FFF9F, 0x1C68421B, 0x8FD9D161, 0x0071968B, 0x4023A40D,
    0x0844622F, 0xCEF92C83, 0x369849D1, 0xB0570A83, 0x2AA9F121, 0x29B85D37, 0x630F564D, 0x8C7C8D8D,
    0x4E4CD63B, 0xEBF07591, 0x75B6E101, 0x242B57FB, 0x3ABC0CCD, 0xB9AF3779, 0xF97A3697, 0xE470F76B,
    0xB28F69CB, 0x361A9D4F, 0x68461395, 0x6A276123, 0x086D6F83, 0x9E8AAB63, 0xFDEA326F, 0x095CF3CF,
    0x0F859D61, 0xF9D262C5, 0xC716616F, 0x35C347F1, 0xC5CE639B, 0x9E6A5929, 0xB1D86CA7, 0xF8E74E79,
    0x881AECC5, 0xF48C89BF, 0x26FFF02F, 0xAF1879CD, 0xA325A10F, 0x4667BBCB, 0x5A6EBCD7, 0xFC2AE1BB,
    0x6ACEB1F7, 0x0E5499A5, 0x7017FBAD, 0x5FC13BD5, 0x51B14671, 0x3D2DCFF9, 0x120DEB4D, 0x072A0577,
    0x608FF3CF, 0xC5AB6983, 0x9D7EE22D, 0x3ED97175, 0x4EEA965B, 0x5F5D9AA9, 0xFD8B06AD, 0xE28E1BA9,
    0xD96BB8ED, 0xBB269469, 0x96DE8F3F, 0x92025F0B, 0x7696AA61, 0x5F7AFDF7, 0x2591EF17, 0x307BF715,
    0x849E64D7, 0xB084D5AF, 0xB38EA8E5, 0x9FEFFA7F, 0x69C66EFB, 0x974A5C09, 0xDFB1AA57, 0x39F375F1,
    0x5811F1BB, 0x9C91083D, 0x80757D6D, 0xCB943BA7, 0x01241BBF, 0x37A81EB7, 0x871B353D, 0xFA2F7BC3,
    0xB2D71DB1, 0x7EF16EFD, 0x42CAA091, 0x676067BB, 0x80AFF14D, 0xD05A9DAB, 0xBB8C974D, 0x290C3E49,
];
