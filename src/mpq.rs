use std::io::{BufReader, Cursor, Read, Seek};

use mpq_rs::{Archive, Creator, FileOptions, MpqError};

/// The HM3W magic bytes that identify a Warcraft 3 map header.
const HM3W_MAGIC: &[u8; 4] = b"HM3W";

/// Standard WC3 filenames to probe when (listfile) is missing (protected maps).
const STANDARD_WC3_FILES: &[&str] = &[
    "war3map.j",
    "war3map.lua",
    "war3map.w3e",
    "war3map.w3i",
    "war3map.wtg",
    "war3map.wct",
    "war3map.wts",
    "war3map.w3r",
    "war3map.w3c",
    "war3map.w3s",
    "war3map.w3u",
    "war3map.w3t",
    "war3map.w3a",
    "war3map.w3b",
    "war3map.w3d",
    "war3map.w3q",
    "war3map.w3h",
    "war3map.doo",
    "war3map.shd",
    "war3mapMap.blp",
    "war3mapMap.b00",
    "war3mapMap.tga",
    "war3mapPath.tga",
    "war3mapPreview.tga",
    "war3mapPreview.blp",
    "war3map.mmp",
    "war3mapMisc.txt",
    "war3mapSkin.txt",
    "war3mapExtra.txt",
    "war3mapUnits.doo",
    r"Scripts\war3map.j",
    "(listfile)",
    "(attributes)",
    "(signature)",
];

/// Errors that can occur during w3x patching.
#[derive(Debug)]
pub enum PatchError {
    Io(std::io::Error),
    Mpq(MpqError),
    NoScriptFile,
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Mpq(e) => write!(f, "MPQ error: {e}"),
            Self::NoScriptFile => write!(
                f,
                "no script file (war3map.j or war3map.lua) found in archive"
            ),
        }
    }
}

impl From<std::io::Error> for PatchError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<MpqError> for PatchError {
    fn from(e: MpqError) -> Self {
        Self::Mpq(e)
    }
}

/// Patch a .w3x map file by replacing its script file with the provided compiled code.
///
/// - `map_path`: path to the input .w3x file
/// - `script_content`: the compiled script (JASS or Lua) to inject
/// - `script_name`: the filename inside the MPQ (e.g. "war3map.j" or "war3map.lua")
/// - `output_path`: path for the patched .w3x output
pub fn patch_w3x(
    map_path: &str,
    script_content: &str,
    script_name: &str,
    output_path: &str,
) -> Result<(), PatchError> {
    eprintln!("[patch] reading map file: {map_path}");
    let map_data = std::fs::read(map_path)?;
    let map_len = map_data.len();
    eprintln!("[patch] map file size: {map_len} bytes");

    // Detect HM3W pre-MPQ header
    let pre_header = detect_pre_header(&map_data);
    let mpq_offset = pre_header.len();
    eprintln!("[patch] HM3W header: {} bytes", pre_header.len());
    if !pre_header.is_empty() {
        eprintln!("[patch] MPQ data starts at offset 0x{mpq_offset:X}");
    }

    // Open the MPQ archive from the MPQ portion
    let mpq_data = map_data.get(mpq_offset..).unwrap_or(&[]);
    let cursor = Cursor::new(mpq_data);
    let mut archive = Archive::open(BufReader::new(cursor))?;
    eprintln!("[patch] MPQ archive opened successfully");

    // Get file list
    let file_list = get_file_list(&mut archive);
    eprintln!("[patch] files in archive: {}", file_list.len());

    // Verify that the target script file exists in the archive
    let script_found = file_list.iter().any(|f| f == script_name);
    if !script_found {
        eprintln!("[patch] ERROR: script file '{script_name}' not found in archive");
        return Err(PatchError::NoScriptFile);
    }
    eprintln!("[patch] target script file found: {script_name}");

    // Create new archive, copying all files and replacing the script
    let mut creator = Creator::default();
    let mut copied = 0usize;
    let mut replaced = false;

    for file_name in &file_list {
        if file_name == script_name {
            eprintln!(
                "[patch] replacing '{file_name}' ({} bytes)",
                script_content.len()
            );
            creator.add_file(
                file_name,
                script_content.as_bytes().to_vec(),
                FileOptions {
                    encrypt: false,
                    compress: true,
                    adjust_key: false,
                },
            );
            replaced = true;
        } else {
            match archive.read_file(file_name) {
                Ok(data) => {
                    let size = data.len();
                    creator.add_file(
                        file_name,
                        data,
                        FileOptions {
                            encrypt: false,
                            compress: true,
                            adjust_key: false,
                        },
                    );
                    eprintln!("[patch] copied '{file_name}' ({size} bytes)");
                    copied += 1;
                }
                Err(e) => {
                    eprintln!("[patch] skipping '{file_name}': {e}");
                }
            }
        }
    }

    eprintln!("[patch] copied {copied} files, replaced: {replaced}");

    // Write new MPQ archive
    let mut mpq_buf = Cursor::new(Vec::new());
    creator
        .write(&mut mpq_buf)
        .map_err(PatchError::Io)?;
    let new_mpq_data = mpq_buf.into_inner();
    eprintln!("[patch] new MPQ archive size: {} bytes", new_mpq_data.len());

    // Assemble output: pre-header + new MPQ data
    let mut output = Vec::with_capacity(pre_header.len() + new_mpq_data.len());
    output.extend_from_slice(pre_header);
    output.extend_from_slice(&new_mpq_data);
    eprintln!("[patch] total output size: {} bytes", output.len());

    std::fs::write(output_path, &output)?;
    eprintln!("[patch] written to: {output_path}");

    Ok(())
}

/// Detect and return the pre-MPQ header (HM3W header) if present.
/// Returns an empty slice if no HM3W header is found.
fn detect_pre_header(data: &[u8]) -> &[u8] {
    // Check for HM3W magic at the start of the file
    if data.len() < 4 {
        return &[];
    }
    let magic = data.get(0..4).unwrap_or(&[]);
    if magic != HM3W_MAGIC {
        return &[];
    }
    eprintln!("[patch] HM3W magic detected at offset 0");

    // The MPQ header starts where we find the MPQ magic "MPQ\x1A"
    // Search for it in the file (typically at 0x200)
    let mpq_magic: &[u8; 4] = b"MPQ\x1a";
    let mut offset = 0usize;
    // MPQ headers are aligned to 512-byte boundaries
    while offset < data.len().saturating_sub(4) {
        let chunk = data.get(offset..offset + 4).unwrap_or(&[]);
        if chunk == mpq_magic {
            eprintln!("[patch] MPQ magic found at offset 0x{offset:X}");
            return data.get(..offset).unwrap_or(&[]);
        }
        offset += 512;
    }

    // No MPQ magic found, return empty (treat entire file as MPQ)
    &[]
}

/// Get the list of files in the archive, using (listfile) or fallback probing.
fn get_file_list<R: Read + Seek>(archive: &mut Archive<R>) -> Vec<String> {
    if let Some(files) = archive.files().filter(|f| !f.is_empty()) {
        eprintln!("[patch] (listfile) found with {} entries", files.len());
        return files;
    }

    eprintln!("[patch] (listfile) not found or empty, probing standard WC3 filenames");
    let mut found = Vec::new();
    for name in STANDARD_WC3_FILES {
        if archive.read_file(name).is_ok() {
            eprintln!("[patch] probed: '{name}' exists");
            found.push((*name).to_string());
        }
    }
    eprintln!("[patch] fallback probing found {} files", found.len());
    found
}
