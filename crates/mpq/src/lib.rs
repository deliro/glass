//! A library for reading and writing Blizzard's proprietary MoPaQ archive format.
//!
//! Currently, only Version 1 MoPaQ archives are supported, as this is the only
//! version of the format still actively encountered in the wild, used by Warcraft III
//! custom maps.

#![allow(dead_code)]

mod archive;
mod consts;
mod creator;
mod error;
mod header;
mod seeker;
mod table;
mod util;

pub use archive::Archive;
pub use archive::RawBlock;
pub use archive::RawHashEntry;
pub use creator::Creator;
pub use creator::FileOptions;
pub use error::MpqError;
