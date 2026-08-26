//! microSD カードの読み書き。
//!
//! LCD の DC と SD の MISO が同じ端子 (GPIO35) に繋がっているため、
//! 画面を初期化する前に設定を読み終えて、カードを外す運用にしている。

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use m5a_core::ports::{Storage, StorageError};

use super::board::SD_MOUNT_POINT;

/// コア層に見せるカード上のファイル操作。
#[derive(Debug, Default)]
pub struct SdStorage;

impl SdStorage {
    pub fn new() -> Self {
        Self
    }

    fn absolute(path: &str) -> PathBuf {
        PathBuf::from(format!("{SD_MOUNT_POINT}{path}"))
    }
}

fn to_storage_error(error: std::io::Error) -> StorageError {
    match error.kind() {
        std::io::ErrorKind::NotFound => StorageError::NotFound,
        _ => StorageError::Io(error.to_string()),
    }
}

impl Storage for SdStorage {
    fn exists(&self, path: &str) -> bool {
        Self::absolute(path).exists()
    }

    fn read_text(&self, path: &str) -> Result<String, StorageError> {
        fs::read_to_string(Self::absolute(path)).map_err(to_storage_error)
    }

    fn write_text(&mut self, path: &str, contents: &str) -> Result<(), StorageError> {
        fs::write(Self::absolute(path), contents).map_err(to_storage_error)
    }

    fn append_text(&mut self, path: &str, contents: &str) -> Result<(), StorageError> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::absolute(path))
            .map_err(to_storage_error)?;

        file.write_all(contents.as_bytes()).map_err(to_storage_error)
    }

    fn create_dir(&mut self, path: &str) -> Result<(), StorageError> {
        match fs::create_dir_all(Self::absolute(path)) {
            Ok(()) => Ok(()),
            // 既にあるなら目的は果たされている。
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(to_storage_error(error)),
        }
    }
}
