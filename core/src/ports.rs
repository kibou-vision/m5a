//! 外部依存を注入するためのトレイト定義。
//!
//! 実機のSDカードやネットワークを伴わずにロジックを検証できるよう、
//! ロジック層はこれらのトレイト越しにのみ外界へ触れる。

/// 記憶領域へのアクセス失敗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// 指定した経路にファイルが存在しない。
    NotFound,
    /// 読み書きに失敗した。カード未挿入や書き込み禁止などを含む。
    Io(String),
}

impl core::fmt::Display for StorageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound => write!(f, "ファイルが見つかりません"),
            Self::Io(detail) => write!(f, "読み書きに失敗しました: {detail}"),
        }
    }
}

/// SDカード上のファイル操作。経路はマウント点を含まない絶対経路で受け取る。
///
/// マウント点の前置はハードウェア層の責務とし、ロジック層は
/// `/.m5a/config.toml` のようなカード内の経路だけを扱う。
pub trait Storage {
    fn exists(&self, path: &str) -> bool;
    fn read_text(&self, path: &str) -> Result<String, StorageError>;
    fn write_text(&mut self, path: &str, contents: &str) -> Result<(), StorageError>;
    fn append_text(&mut self, path: &str, contents: &str) -> Result<(), StorageError>;
    fn create_dir(&mut self, path: &str) -> Result<(), StorageError>;
}

/// 起動からの経過時間。表情アニメーションの位相決定に使う。
pub trait Clock {
    fn now_ms(&self) -> u64;
}

#[cfg(test)]
pub mod mock {
    use super::{Storage, StorageError};
    use std::collections::{BTreeMap, BTreeSet};

    /// 単体テスト用のメモリ上ストレージ。
    #[derive(Debug, Default)]
    pub struct MemoryStorage {
        files: BTreeMap<String, String>,
        dirs: BTreeSet<String>,
        /// 書き込みを常に失敗させ、SDカード異常時の経路を検証する。
        pub fail_writes: bool,
        /// 成功を返しつつ内容を捨て、壊れたカードの挙動を再現する。
        pub discard_writes: bool,
    }

    impl MemoryStorage {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_file(path: &str, contents: &str) -> Self {
            let mut storage = Self::new();
            storage.files.insert(path.to_string(), contents.to_string());
            storage
        }

        pub fn peek(&self, path: &str) -> Option<&str> {
            self.files.get(path).map(String::as_str)
        }

        pub fn has_dir(&self, path: &str) -> bool {
            self.dirs.contains(path)
        }
    }

    impl Storage for MemoryStorage {
        fn exists(&self, path: &str) -> bool {
            self.files.contains_key(path)
        }

        fn read_text(&self, path: &str) -> Result<String, StorageError> {
            self.files.get(path).cloned().ok_or(StorageError::NotFound)
        }

        fn write_text(&mut self, path: &str, contents: &str) -> Result<(), StorageError> {
            if self.fail_writes {
                return Err(StorageError::Io("書き込み禁止".to_string()));
            }
            if self.discard_writes {
                return Ok(());
            }
            self.files.insert(path.to_string(), contents.to_string());
            Ok(())
        }

        fn append_text(&mut self, path: &str, contents: &str) -> Result<(), StorageError> {
            if self.fail_writes {
                return Err(StorageError::Io("書き込み禁止".to_string()));
            }
            self.files.entry(path.to_string()).or_default().push_str(contents);
            Ok(())
        }

        fn create_dir(&mut self, path: &str) -> Result<(), StorageError> {
            if self.fail_writes {
                return Err(StorageError::Io("書き込み禁止".to_string()));
            }
            self.dirs.insert(path.to_string());
            Ok(())
        }
    }
}
