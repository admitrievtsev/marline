use std::collections::HashSet;

const WORD_LEN: usize = 8;
const COUNT_WORDS: usize = 5;
const RABIN_HASH_X: u32 = 43;
const RABIN_HASH_Q: u32 = (1 << 31) - 1;

fn set_for_chunk(data: &[u8]) -> HashSet<u32> {
    let block_size = WORD_LEN * COUNT_WORDS;
    let mut set_blocks = HashSet::new();
    let mut rabin_hash = rabin_hash_simple(&data[0..std::cmp::min(block_size, data.len())]);

    for index_word in (0..data.len()).step_by(WORD_LEN) {
        set_blocks.insert(rabin_hash);
        if index_word + block_size > data.len() {
            break;
        }
        rabin_hash = rabin_hash_next(
            rabin_hash,
            hash_word(&data[index_word..index_word + WORD_LEN]),
            hash_word(
                &data[index_word + block_size
                    ..std::cmp::min(index_word + block_size + WORD_LEN, data.len())],
            ),
        );
    }
    set_blocks
}

fn rabin_hash_simple(data: &[u8]) -> u32 {
    let mut rabin_hash = 0;
    for i in (0..data.len()).step_by(WORD_LEN) {
        rabin_hash += hash_word(&data[i..i + WORD_LEN])
            * RABIN_HASH_X.pow((COUNT_WORDS - i / WORD_LEN) as u32)
            % RABIN_HASH_Q;
    }
    rabin_hash
}

fn hash_word(word: &[u8]) -> u32 {
    let mut hash_word = 0;
    for byte in word {
        hash_word += *byte as u32;
    }
    hash_word
}

fn rabin_hash_next(past_hash: u32, hash_start_word: u32, hash_next_word: u32) -> u32 {
    ((past_hash - hash_start_word * RABIN_HASH_X.pow(COUNT_WORDS as u32 - 1)) * RABIN_HASH_X
        + hash_next_word)
        % RABIN_HASH_Q
}
