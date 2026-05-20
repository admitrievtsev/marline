use crate::encoder::GEAR;
use crate::hasher::{SBCHash, SBCHasher};
use std::hash::{DefaultHasher, Hash, Hasher};
use rand;
use crate::hasher::odess_hasher::IndexType::SuperFeatured;

pub const SUPER_FEATURES_NUM: usize = 8;

/// Configuración para la generación de super características en el algoritmo Odess.
///
/// Esta estructura define los parámetros necesarios para crear super características,
/// que son combinaciones de características individuales agrupadas para mejorar
/// la eficiencia del cálculo de hash.
///
/// # Campos
///
/// * `groups` - Número de grupos de características a crear. Debe ser mayor que cero.
/// * `sf_num` - Número de super características por grupo. Debe ser mayor que cero.
pub struct SuperFeatureConfig {
    groups: usize,
    sf_num: usize,
}

impl SuperFeatureConfig {
    /// Crea una nueva configuración de super características.
    ///
    /// # Argumentos
    ///
    /// * `groups` - Número de grupos de características. Debe ser mayor que cero.
    /// * `sf_num` - Número de super características por grupo. Debe ser mayor que cero.
    ///
    /// # Panics
    ///
    /// Esta función entrará en pánico si `groups` o `sf_num` son cero.
    pub fn new(groups: usize, sf_num: usize) -> Self {
        if groups == 0 {
            panic!("Super Feature groups number cannot be zero");
        }
        if sf_num == 0 {
            panic!("Super Feature number cannot be zero");
        }
        Self { groups, sf_num }
    }
}

/// Tipo de índice utilizado para el cálculo de hash en el algoritmo Odess.
///
/// Este enumerador define dos estrategias diferentes para generar el hash:
/// utilizando super características o características crudas.
///
/// # Variantes
///
/// * `SuperFeatured` - Utiliza super características agrupadas para el cálculo de hash.
/// * `RawFeatured` - Utiliza características crudas sin agrupamiento para el cálculo de hash.
pub enum IndexType {
    /// Utiliza super características para el cálculo de hash.
    ///
    /// Esta variante agrupa las características en super características,
    /// lo que puede mejorar la eficiencia y la precisión del cálculo de hash.
    ///
    /// # Ejemplo
    ///
    /// ```
    /// use sbc_algorithm::hasher::odess_hasher::{IndexType, SuperFeatureConfig};
    ///
    /// let config = SuperFeatureConfig::new(2, 6);
    /// let index_type = IndexType::SuperFeatured(config);
    /// ```
    SuperFeatured(SuperFeatureConfig),
    
    /// Utiliza características crudas para el cálculo de hash.
    ///
    /// Esta variante utiliza las características directamente sin agrupamiento,
    /// especificando el número de características a utilizar.
    RawFeatured(usize),
}

#[derive(Default, Debug, Clone)]
pub struct OdessHash {
    hash: Vec<u64>,
}

impl Hash for OdessHash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state)
    }
}

impl Eq for OdessHash {}

impl PartialEq<Self> for OdessHash {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl SBCHash for OdessHash {
    fn new_with_u32(_: u32) -> Self {
        todo!()
    }

    fn next_hash(&self) -> Self {
        let mut odess_hash = self.clone();
        if odess_hash.hash[0] < u64::MAX {
            odess_hash.hash[0] += 1;
        } else if odess_hash.hash[1] < u64::MAX {
            odess_hash.hash[0] = 0;
            odess_hash.hash[1] += 1;
        } else if odess_hash.hash[2] < u64::MAX {
            odess_hash.hash[0] = 0;
            odess_hash.hash[1] = 0;
            odess_hash.hash[2] += 1;
        } else {
            odess_hash.hash = vec![u64::MAX; SUPER_FEATURES_NUM]
        }
        odess_hash
    }

    fn last_hash(&self) -> Self {
        let mut odess_hash = self.clone();
        if odess_hash.hash[0] > 0 {
            odess_hash.hash[0] -= 1;
        } else if odess_hash.hash[1] > 0 {
            odess_hash.hash[0] = u64::MAX;
            odess_hash.hash[1] -= 1;
        } else if odess_hash.hash[2] > 0 {
            odess_hash.hash[0] = u64::MAX;
            odess_hash.hash[1] = u64::MAX;
            odess_hash.hash[2] -= 1;
        } else {
            odess_hash.hash = vec![0u64; SUPER_FEATURES_NUM]
        }
        odess_hash
    }

    fn get_key_for_graph_clusterer(&self) -> u32 {
        todo!()
    }

    fn as_slice(&self) -> &[u64] {
        self.hash.as_slice()
    }
}


pub struct OdessHasher {
    sampling_rate: u64,
    linear_coefficients: Vec<u64>,
    index_type: IndexType,
    features_num: usize,
}

impl SBCHasher for OdessHasher {
    type Hash = OdessHash;
    fn calculate_hash(&self, chunk: &[u8]) -> OdessHash {
        let mut features = vec![u64::MAX; self.features_num];

        let mask = (1u64 << self.sampling_rate) - 1;
        let mut fp = 0u64;

        for &byte in chunk {
            // Gear rolling hash: FP = (FP << 1) + Gear[byte]
            fp = (fp << 1).wrapping_add(GEAR[byte as usize]);

            // Content-defined sampling
            if fp & mask == 0 {
                for (i, feature) in features.iter_mut().enumerate().take(self.features_num) {
                    let transform = self.linear_coefficients[i]
                        .wrapping_mul(fp)
                        .wrapping_add(byte as u64)
                        % (1u64 << 32);
                    if *feature > transform {
                        *feature = transform;
                    }
                }
            }
        }

        match &self.index_type {
            IndexType::SuperFeatured(sf_cfg) => {
                let mut super_features = vec![0u64; sf_cfg.sf_num];
                let mut sf_groups = Vec::with_capacity(sf_cfg.sf_num);
                for group_idx in 0..sf_cfg.groups {
                    let group_start = group_idx * sf_cfg.sf_num;
                    let group_end = group_start + sf_cfg.sf_num;
                    
                    if group_end <= features.len() {
                        let mut group: Vec<u64> = features[group_start..group_end].to_vec();
                        group.sort();
                        sf_groups.push(group);
                    }

                    for i in 0..sf_cfg.sf_num {
                        let mut hasher = DefaultHasher::new();
                        for item in sf_groups.iter() {
                            hasher.write_u64(item[i]);
                        }
                        let hash_value = hasher.finish();
                        super_features[i] = hash_value;
                    }
                }

                OdessHash { hash: super_features }
            },
            IndexType::RawFeatured(_) => {
                OdessHash { hash: features.to_vec() }
            }
        }


    }
}

impl Default for OdessHasher {

    fn default() -> Self {
        let config = SuperFeatureConfig::new(2, 6);

        Self::new(7, SuperFeatured(config))
    }
}

impl OdessHasher {
    pub fn new(sampling_rate: u64, index_type: IndexType) -> Self {
        let features_num = match &index_type {
            IndexType::SuperFeatured(sf_cfg) => sf_cfg.sf_num * sf_cfg.groups,
            IndexType::RawFeatured(rf_num) => *rf_num,
        };

        let mut linear_coefficients = Vec::with_capacity(features_num);

        for _ in 0..features_num {
            linear_coefficients.push(rand::random::<u64>());
        }

        OdessHasher {
            sampling_rate,
            linear_coefficients,
            index_type,
            features_num
        }
    }
}
