use std::borrow::Cow;

use byte_slice_cast::AsMutSliceOf;

use lazy_static::lazy_static;

use crate::consts::*;
use crate::error::*;

lazy_static! {
    static ref CRYPTO_TABLE: [u32; 0x500] = generate_crypto_table();
}

fn generate_crypto_table() -> [u32; 0x500] {
    let mut crypto_table = [0u32; 0x500];
    let mut seed: u32 = 0x0010_0001;

    for i in 0..0x100 {
        for j in 0..5 {
            let index = i + j * 0x100;
            seed = (seed * 125 + 3) % 0x002A_AAAB;
            let t1 = (seed & 0xFFFF) << 0x10;
            seed = (seed * 125 + 3) % 0x002A_AAAB;
            let t2 = seed & 0xFFFF;

            // index is always < 0x500 (i < 0x100, j < 5)
            if let Some(slot) = crypto_table.get_mut(index) {
                *slot = t1 | t2;
            }
        }
    }

    crypto_table
}

fn crypto_lookup(hash_type: u32, upper: u32) -> u32 {
    let idx = (hash_type + upper) as usize;
    CRYPTO_TABLE.get(idx).copied().unwrap_or(0)
}

fn hash_string_with_table(source: &[u8], hash_type: u32, lookup: &[u8]) -> u32 {
    let mut seed1: u32 = 0x7FED_7FED;
    let mut seed2: u32 = 0xEEEE_EEEE;

    for byte in source {
        let upper = u32::from(
            lookup
                .get(*byte as usize)
                .copied()
                .unwrap_or(*byte),
        );

        seed1 = crypto_lookup(hash_type, upper) ^ seed1.overflowing_add(seed2).0;
        seed2 = upper
            .overflowing_add(seed1)
            .0
            .overflowing_add(seed2)
            .0
            .overflowing_add(seed2 << 5)
            .0
            .overflowing_add(3)
            .0;
    }

    seed1
}

pub fn hash_string(source: &[u8], hash_type: u32) -> u32 {
    hash_string_with_table(source, hash_type, &ASCII_UPPER_LOOKUP_SLASH_SENSITIVE)
}

fn process_mpq_block(data: &mut [u8], mut key: u32, encrypt: bool) {
    let iterations = data.len() >> 2;

    let mut key_secondary: u32 = 0xEEEE_EEEE;
    let mut temp: u32;

    // if the buffer is not aligned to u32s we need to truncate it
    // this is ok because the last bytes that don't fit into the
    // aligned slice are not encrypted
    let Some(u32_data) = data
        .get_mut(..iterations * 4)
        .and_then(|s| s.as_mut_slice_of::<u32>().ok())
    else {
        return;
    };

    for item in u32_data.iter_mut() {
        key_secondary = key_secondary
            .overflowing_add(crypto_lookup(MPQ_HASH_KEY2_MIX, key & 0xFF))
            .0;

        if encrypt {
            temp = *item;
            *item ^= key.overflowing_add(key_secondary).0;
        } else {
            *item ^= key.overflowing_add(key_secondary).0;
            temp = *item;
        }

        key = ((!key << 0x15).overflowing_add(0x1111_1111).0) | (key >> 0x0B);
        key_secondary = temp
            .overflowing_add(key_secondary)
            .0
            .overflowing_add(key_secondary << 5)
            .0
            .overflowing_add(3)
            .0;
    }
}

pub fn decrypt_mpq_block(data: &mut [u8], key: u32) {
    process_mpq_block(data, key, false);
}

pub fn encrypt_mpq_block(data: &mut [u8], key: u32) {
    process_mpq_block(data, key, true);
}

pub fn get_plain_name(input: &str) -> &[u8] {
    let bytes = input.as_bytes();
    let mut out = input.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\\' || byte == b'/' {
            out = bytes.get((i + 1)..).unwrap_or(&[]);
        }
    }

    out
}

pub fn calculate_file_key(
    file_name: &str,
    file_offset: u32,
    file_size: u32,
    adjusted: bool,
) -> u32 {
    let plain_name = get_plain_name(file_name);
    let mut key = hash_string(plain_name, MPQ_HASH_FILE_KEY);

    if adjusted {
        key = (key + file_offset) ^ file_size
    }

    key
}

/// This will try to perform the following two operations:
///
/// 1. If `encryption_key` is specified, it will decrypt the block using
///    that encryption key.
/// 2. If `input.len()` < `uncompressed_size`, it will try to decompress
///    the block. MPQ supports multiple compression types, and the compression
///    type used for a particular block is specified in the first byte of the block
///    as a set of bitflags.
pub fn decode_mpq_block(
    input: &'_ [u8],
    uncompressed_size: u64,
    encryption_key: Option<u32>,
) -> Result<Cow<'_, [u8]>, MpqError> {
    let compressed_size = input.len() as u64;
    let mut buf = Cow::Borrowed(input);

    if let Some(encryption_key) = encryption_key {
        decrypt_mpq_block(buf.to_mut(), encryption_key);
    }

    if compressed_size < uncompressed_size {
        let compression_type = *buf.first().ok_or(MpqError::Corrupted)?;

        if compression_type & COMPRESSION_IMA_ADPCM_MONO_MONO != 0 {
            return Err(MpqError::UnsupportedCompression {
                kind: "IMA ADCPM Mono".to_string(),
            });
        }

        if compression_type & COMPRESSION_IMA_ADPCM_MONO_STEREO != 0 {
            return Err(MpqError::UnsupportedCompression {
                kind: "IMA ADCPM Stereo".to_string(),
            });
        }

        if compression_type & COMPRESSION_HUFFMAN != 0 {
            return Err(MpqError::UnsupportedCompression {
                kind: "Huffman".to_string(),
            });
        }

        if compression_type & COMPRESSION_PKWARE != 0 {
            return Err(MpqError::UnsupportedCompression {
                kind: "PKWare DCL".to_string(),
            });
        }

        let uncompressed_usize =
            usize::try_from(uncompressed_size).map_err(|_| MpqError::Corrupted)?;

        if compression_type & COMPRESSION_BZIP2 != 0 {
            let compressed_data = buf.get(1..).ok_or(MpqError::Corrupted)?;
            let mut decompressed = vec![0u8; uncompressed_usize];
            let mut decompressor = bzip2::Decompress::new(false);
            let status = decompressor.decompress(compressed_data, &mut decompressed);

            match status {
                Ok(bzip2::Status::Ok) => {}
                _ => return Err(MpqError::Corrupted),
            }

            let total_out =
                usize::try_from(decompressor.total_out()).map_err(|_| MpqError::Corrupted)?;
            decompressed.resize(total_out, 0);
            buf = Cow::Owned(decompressed);
        }

        if compression_type & COMPRESSION_ZLIB != 0 {
            let compressed_data = buf.get(1..).ok_or(MpqError::Corrupted)?;
            let mut decompressed = vec![0u8; uncompressed_usize];
            let mut decompressor = flate2::Decompress::new(true);
            let status = decompressor.decompress(
                compressed_data,
                &mut decompressed,
                flate2::FlushDecompress::Finish,
            );

            match status {
                Ok(s) if s != flate2::Status::BufError => {}
                _ => return Err(MpqError::Corrupted),
            }

            let total_out =
                usize::try_from(decompressor.total_out()).map_err(|_| MpqError::Corrupted)?;
            decompressed.resize(total_out, 0);
            buf = Cow::Owned(decompressed);
        }
    }

    Ok(buf)
}

/// This will try to compress the block using zlib compression.
/// If the compression succeeded, the block will be prepended by a single
/// byte indicating which compression method was used.
/// The compression can fail if the compressed buffer turns out to be
/// larger than the uncompressed one, in which case it will simply
/// return the uncompressed buffer.
pub fn compress_mpq_block(input: &'_ [u8]) -> Cow<'_, [u8]> {
    let mut compressed: Vec<u8> = vec![0u8; input.len() + 1];

    let mut compressor = flate2::Compress::new(flate2::Compression::best(), true);
    let compress_dest = compressed.get_mut(1..).unwrap_or(&mut []);
    let status = compressor.compress(input, compress_dest, flate2::FlushCompress::Finish);

    if status.is_err() {
        return Cow::Borrowed(input);
    }

    if let Some(slot) = compressed.first_mut() {
        *slot = COMPRESSION_ZLIB;
    }

    let total_out_plus_one =
        usize::try_from(compressor.total_out() + 1).unwrap_or(usize::MAX);
    if total_out_plus_one >= input.len() {
        Cow::Borrowed(input)
    } else {
        compressed.truncate(total_out_plus_one);
        Cow::Owned(compressed)
    }
}

pub fn sector_count_from_size(size: u64, sector_count: u64) -> u64 {
    if size == 0 {
        1
    } else {
        ((size - 1) / sector_count) + 1
    }
}
