//! LCD の初期化。
//!
//! ILI9342C は横長 320x240 が本来の向きなので、回転をかけずにそのまま使う。
//! CoreS3 のパネルは色が反転しているため反転指定が要る。
//! リセット線は AW9523 経由で電源投入時に解除済みのため、ここでは触らない。

use anyhow::{Context, Result};
use esp_idf_svc::hal::delay::Ets;
use esp_idf_svc::hal::gpio::{Output, PinDriver};
use esp_idf_svc::hal::spi::{SpiDeviceDriver, SpiDriver};
use mipidsi::interface::SpiInterface;
use mipidsi::models::ILI9342CRgb565;
use mipidsi::options::ColorInversion;
use mipidsi::{Builder, Display, NoResetPin};

use super::pins::{SCREEN_HEIGHT, SCREEN_WIDTH};

type Interface<'d> =
    SpiInterface<'d, SpiDeviceDriver<'d, SpiDriver<'d>>, PinDriver<'d, Output>>;

/// 初期化済みの LCD。
pub type Lcd<'d> = Display<Interface<'d>, ILI9342CRgb565, NoResetPin>;

/// 転送をまとめるための作業領域の大きさ。
pub const TRANSFER_BUFFER_LEN: usize = 512;

/// LCD を初期化する。
pub fn build_display<'d>(
    spi: SpiDeviceDriver<'d, SpiDriver<'d>>,
    data_command: PinDriver<'d, Output>,
    buffer: &'d mut [u8],
) -> Result<Lcd<'d>> {
    let interface = SpiInterface::new(spi, data_command, buffer);

    Builder::new(ILI9342CRgb565, interface)
        .display_size(SCREEN_WIDTH, SCREEN_HEIGHT)
        .invert_colors(ColorInversion::Inverted)
        .init(&mut Ets)
        .map_err(|error| anyhow::anyhow!("LCD を初期化できません: {error:?}"))
        .context("画面の準備に失敗しました")
}
