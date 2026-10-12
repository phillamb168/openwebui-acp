//! Private local files. Never follow a final symlink or read unbounded content.
use crate::error::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};

pub const MAX_SECRET_BYTES: u64 = 64 * 1024;

pub fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    options
}

pub fn check_file(file: &File, limit: u64) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|_| Error::State("Cannot inspect private file"))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(Error::State("Private file must be a bounded regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.nlink() != 1
        {
            return Err(Error::State(
                "Private file must be owned by this user, have mode 0600 or stricter, and no hard links",
            ));
        }
    }
    Ok(())
}

pub fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = private_options()
        .read(true)
        .open(path)
        .map_err(|_| Error::State("Cannot open private file (symlinks are rejected)"))?;
    check_file(&file, limit)?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::State("Cannot read private file"))?;
    if bytes.len() as u64 > limit {
        return Err(Error::Capacity);
    }
    Ok(bytes)
}

pub fn read_secret(path: &Path) -> Result<String> {
    String::from_utf8(read_private(path, MAX_SECRET_BYTES)?)
        .map_err(|_| Error::Config("Secret file must be UTF-8".into()))
}
