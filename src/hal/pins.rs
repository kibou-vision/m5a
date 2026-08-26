//! M5Stack CoreS3 の配線。
//!
//! 端子そのものは `Peripherals` から型付きで取り出すため、ここには番号を
//! 重ねて持たない。実際の割り当ては次のとおり。
//!
//! | 用途 | 端子 |
//! |---|---|
//! | 内部 I2C SDA / SCL | GPIO12 / GPIO11 |
//! | LCD・microSD 共有 SPI SCLK / MOSI | GPIO36 / GPIO37 |
//! | LCD の DC ／ microSD の MISO | GPIO35（共有） |
//! | LCD の CS | GPIO3 |
//! | microSD の CS | GPIO4 |
//! | タッチの割り込み | GPIO21 |
//!
//! I2S は M5Stack 公式ドキュメントが BCLK/LRCK と DIN/DOUT を入れ替えて
//! 記載している。Espressif の BSP と M5Unified が一致する側
//! （MCLK=GPIO0, BCLK=GPIO34, WS=GPIO33, DIN=GPIO14, DOUT=GPIO13）を採る。

/// FT6336 が 400kHz では応答しないため、内部バス全体を 100kHz で動かす。
pub const I2C_HZ: u32 = 100_000;

/// 電源管理 IC。
pub const AXP2101_ADDRESS: u8 = 0x34;
/// GPIO 拡張。LCD のリセットや SD の電源を握る。
pub const AW9523_ADDRESS: u8 = 0x58;
/// 静電容量タッチ。
pub const FT6336_ADDRESS: u8 = 0x38;

/// 画面の大きさ。ILI9342C は横長が本来の向き。
pub const SCREEN_WIDTH: u16 = 320;
pub const SCREEN_HEIGHT: u16 = 240;
