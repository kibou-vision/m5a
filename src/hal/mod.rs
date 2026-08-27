//! 実機に触れる層。ここだけが ESP-IDF と CoreS3 の BSP を知っている。

pub mod board;
pub mod face;
pub mod session;
pub mod storage;
pub mod touch;
pub mod wifi;
