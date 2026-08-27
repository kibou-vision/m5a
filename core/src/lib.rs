//! m5a のハードウェアに依存しないロジック層。
//!
//! 実機なしで検証できるよう、外部依存は [`ports`] のトレイト経由でのみ扱う。

pub mod audio;
pub mod config;
pub mod face;
pub mod greeting;
pub mod guardrail;
pub mod layout;
pub mod logbook;
pub mod ports;
pub mod realtime;
pub mod search;
pub mod state;
pub mod turn_detector;
