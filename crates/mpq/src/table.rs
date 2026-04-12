use std::io::Error as IoError;
use std::io::{Read, Seek, Write};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::consts::*;
use crate::error::MpqError;
use crate::seeker::*;
use crate::util::*;

#[derive(Debug)]
pub(crate) struct FileHashTable {
    entries: Vec<HashEntry>,
}

impl FileHashTable {
    pub fn from_seeker<R>(seeker: &mut Seeker<R>) -> Result<FileHashTable, MpqError>
    where
        R: Read + Seek,
    {
        let info = seeker.info().hash_table_info;
        let expected_size = info.entries * u64::from(HASH_TABLE_ENTRY_SIZE);
        let raw_data = seeker.read(info.offset, info.size)?;
        let decoded_data = decode_mpq_block(&raw_data, expected_size, Some(HASH_TABLE_KEY))?;

        let entries_count = usize::try_from(info.entries).map_err(|_| MpqError::Corrupted)?;
        let mut entries = Vec::with_capacity(entries_count);
        let mut slice = &decoded_data[..];
        for _ in 0..info.entries {
            entries.push(HashEntry::from_reader(&mut slice)?);
        }

        Ok(FileHashTable { entries })
    }

    pub fn find_by_block_index(&self, block_index: usize) -> Option<crate::archive::RawHashEntry> {
        for entry in &self.entries {
            if !entry.is_blank() && entry.block_index as usize == block_index {
                return Some(crate::archive::RawHashEntry {
                    hash_a: entry.hash_a,
                    hash_b: entry.hash_b,
                    locale: entry.locale,
                    platform: entry.platform,
                });
            }
        }
        None
    }

    pub fn find_entry(&self, name: &str) -> Option<&HashEntry> {
        let hash_mask = self.entries.len().checked_sub(1)?;
        let part_a = hash_string(name.as_bytes(), MPQ_HASH_NAME_A);
        let part_b = hash_string(name.as_bytes(), MPQ_HASH_NAME_B);
        let index = hash_string(name.as_bytes(), MPQ_HASH_TABLE_INDEX) as usize;

        let start_index = index & hash_mask;
        let mut index = start_index;

        loop {
            let inspected = self.entries.get(index)?;

            if inspected.block_index == HASH_TABLE_EMPTY_ENTRY {
                break;
            }

            if inspected.hash_a == part_a && inspected.hash_b == part_b && inspected.locale == 0 {
                return Some(inspected);
            }

            index = (index + 1) & hash_mask;
            if index == start_index {
                break;
            }
        }

        None
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HashEntry {
    pub hash_a: u32,
    pub hash_b: u32,
    pub locale: u16,
    pub platform: u16,
    pub block_index: u32,
}

impl HashEntry {
    pub fn new(hash_a: u32, hash_b: u32, block_index: u32) -> HashEntry {
        HashEntry {
            hash_a,
            hash_b,
            locale: 0,
            platform: 0,
            block_index,
        }
    }

    pub fn from_reader<R: Read>(mut reader: R) -> Result<HashEntry, MpqError> {
        let hash_a = reader.read_u32::<LE>()?;
        let hash_b = reader.read_u32::<LE>()?;
        let locale = reader.read_u16::<LE>()?;
        let platform = reader.read_u16::<LE>()?;
        let block_index = reader.read_u32::<LE>()?;

        Ok(HashEntry {
            hash_a,
            hash_b,
            locale,
            platform,
            block_index,
        })
    }

    pub fn blank() -> HashEntry {
        HashEntry {
            hash_a: 0xFFFF_FFFF,
            hash_b: 0xFFFF_FFFF,
            locale: 0xFFFF,
            platform: 0x00FF,
            block_index: 0xFFFF_FFFF,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.block_index == 0xFFFF_FFFF
    }

    pub fn write<W: Write>(&self, mut writer: W) -> Result<(), IoError> {
        writer.write_u32::<LE>(self.hash_a)?;
        writer.write_u32::<LE>(self.hash_b)?;
        writer.write_u16::<LE>(self.locale)?;
        writer.write_u16::<LE>(self.platform)?;
        writer.write_u32::<LE>(self.block_index)?;

        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct FileBlockTable {
    entries: Vec<BlockEntry>,
}

impl FileBlockTable {
    pub fn from_seeker<R>(seeker: &mut Seeker<R>) -> Result<FileBlockTable, MpqError>
    where
        R: Read + Seek,
    {
        let info = seeker.info().block_table_info;
        let expected_size = info.entries * u64::from(BLOCK_TABLE_ENTRY_SIZE);
        let raw_data = seeker.read(info.offset, info.size)?;
        let decoded_data = decode_mpq_block(&raw_data, expected_size, Some(BLOCK_TABLE_KEY))?;

        let entries_count = usize::try_from(info.entries).map_err(|_| MpqError::Corrupted)?;
        let mut entries = Vec::with_capacity(entries_count);
        let mut slice = &decoded_data[..];
        for _ in 0..info.entries {
            entries.push(BlockEntry::from_reader(&mut slice)?);
        }

        Ok(FileBlockTable { entries })
    }

    pub fn get(&self, index: usize) -> Option<&BlockEntry> {
        self.entries.get(index)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug)]
pub(crate) struct BlockEntry {
    pub file_pos: u64,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub flags: u32,
}

impl BlockEntry {
    pub fn new(
        file_pos: u64,
        compressed_size: u64,
        uncompressed_size: u64,
        flags: u32,
    ) -> BlockEntry {
        BlockEntry {
            file_pos,
            compressed_size,
            uncompressed_size,
            flags,
        }
    }

    pub fn from_reader<R: Read>(mut reader: R) -> Result<BlockEntry, MpqError> {
        let file_pos = u64::from(reader.read_u32::<LE>()?);
        let compressed_size = u64::from(reader.read_u32::<LE>()?);
        let uncompressed_size = u64::from(reader.read_u32::<LE>()?);
        let flags = reader.read_u32::<LE>()?;

        Ok(BlockEntry {
            file_pos,
            compressed_size,
            uncompressed_size,
            flags,
        })
    }

    pub fn write<W: Write>(&self, mut writer: W) -> Result<(), IoError> {
        writer.write_u32::<LE>(u32::try_from(self.file_pos).map_err(|_| {
            IoError::new(std::io::ErrorKind::InvalidData, "file_pos overflows u32")
        })?)?;
        writer.write_u32::<LE>(u32::try_from(self.compressed_size).map_err(|_| {
            IoError::new(
                std::io::ErrorKind::InvalidData,
                "compressed_size overflows u32",
            )
        })?)?;
        writer.write_u32::<LE>(u32::try_from(self.uncompressed_size).map_err(|_| {
            IoError::new(
                std::io::ErrorKind::InvalidData,
                "uncompressed_size overflows u32",
            )
        })?)?;
        writer.write_u32::<LE>(self.flags)?;

        Ok(())
    }

    pub fn is_imploded(&self) -> bool {
        (self.flags & MPQ_FILE_IMPLODE) != 0
    }

    pub fn is_compressed(&self) -> bool {
        (self.flags & MPQ_FILE_COMPRESS) != 0
    }

    pub fn is_encrypted(&self) -> bool {
        (self.flags & MPQ_FILE_ENCRYPTED) != 0
    }

    pub fn is_key_adjusted(&self) -> bool {
        (self.flags & MPQ_FILE_ADJUST_KEY) != 0
    }
}

#[derive(Debug)]
pub(crate) struct SectorOffsets {
    offsets: Vec<u32>,
}

impl SectorOffsets {
    pub fn from_reader<R>(
        seeker: &mut Seeker<R>,
        block_entry: &BlockEntry,
        encryption_key: Option<u32>,
    ) -> Result<SectorOffsets, MpqError>
    where
        R: Read + Seek,
    {
        let sector_count =
            sector_count_from_size(block_entry.uncompressed_size, seeker.info().sector_size);
        let mut raw_data = seeker.read(block_entry.file_pos, (sector_count + 1) * 4)?;

        if let Some(encryption_key) = encryption_key {
            decrypt_mpq_block(&mut raw_data, encryption_key);
        }

        let mut slice = &raw_data[..];
        let offsets_count =
            usize::try_from(sector_count + 1).map_err(|_| MpqError::Corrupted)?;
        let mut offsets = vec![0u32; offsets_count];
        for offset in &mut offsets {
            *offset = slice.read_u32::<LE>()?;
        }

        Ok(SectorOffsets { offsets })
    }

    pub fn one(&self, index: usize) -> Option<(u32, u32)> {
        let current = self.offsets.get(index).copied()?;
        let next = self.offsets.get(index + 1).copied()?;
        Some((current, next - current))
    }

    pub fn all(&self) -> Option<(u32, u32)> {
        let first = self.offsets.first().copied()?;
        let last = self.offsets.last().copied()?;
        Some((first, last - first))
    }

    pub fn count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }
}
