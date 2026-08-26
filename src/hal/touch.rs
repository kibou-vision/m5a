//! FT6336 タッチパネルの読み取り。
//!
//! 押している間だけ録音する方式のため、座標そのものより
//! 「押された／離された」の変わり目を取りこぼさないことを優先する。

use anyhow::Result;
use embedded_graphics::prelude::Point;
use esp_idf_svc::hal::i2c::I2cDriver;

use super::pins::FT6336_ADDRESS;

/// タッチ点の数と1点目の座標がまとまって置かれている先頭のレジスタ。
const TOUCH_STATUS_REGISTER: u8 = 0x02;
const TIMEOUT_TICKS: u32 = 1_000;

/// 指の状態の変わり目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchChange {
    Pressed(Point),
    Released,
}

/// 直前の状態を覚えて変わり目だけを返す。
#[derive(Debug, Default)]
pub struct TouchReader {
    was_touching: bool,
}

impl TouchReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// 1回読み取る。状態が変わっていなければ `None` を返す。
    ///
    /// 読み取りに失敗しても会話を止めたくないので、呼び出し側で握りつぶせるよう
    /// 結果を返すだけにしている。
    pub fn poll(&mut self, i2c: &mut I2cDriver) -> Result<Option<TouchChange>> {
        let mut status = [0_u8; 5];
        i2c.write_read(
            FT6336_ADDRESS,
            &[TOUCH_STATUS_REGISTER],
            &mut status,
            TIMEOUT_TICKS,
        )?;

        let touching = status[0] > 0;
        if touching == self.was_touching {
            return Ok(None);
        }
        self.was_touching = touching;

        if !touching {
            return Ok(Some(TouchChange::Released));
        }

        // 座標は上位4bitと下位8bitに分かれて置かれている。
        let x = i32::from(u16::from(status[1] & 0x0F) << 8 | u16::from(status[2]));
        let y = i32::from(u16::from(status[3] & 0x0F) << 8 | u16::from(status[4]));

        Ok(Some(TouchChange::Pressed(Point::new(x, y))))
    }
}
