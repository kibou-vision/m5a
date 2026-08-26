//! CoreS3 の電源投入。
//!
//! AXP2101 と AW9523 を初期化しないと LCD もマイクも SD も動かない。
//! すべて成功ログを出しながら画面だけ真っ暗という失敗はここが原因になる。
//! レジスタ値は M5Unified の CoreS3 向け初期化列に合わせている。

use anyhow::{Context, Result};
use esp_idf_svc::hal::i2c::I2cDriver;

use super::pins::{AW9523_ADDRESS, AXP2101_ADDRESS};

/// I2C 転送の待ち時間。内部バスは短く、待たされる要因がない。
const TIMEOUT_TICKS: u32 = 1_000;

/// バックライトに使う DLDO1 の電圧範囲。
const BACKLIGHT_MIN_MV: u32 = 2_500;
const BACKLIGHT_MAX_MV: u32 = 3_300;

/// AXP2101 の電圧レジスタは 0.1V 刻みで 0.5V を原点に符号化されている。
const fn encode_decivolts(millivolts: u32) -> u8 {
    (millivolts / 100 - 5) as u8
}

/// I2C バス上の1デバイス。レジスタ操作をまとめて扱いやすくする。
struct Device<'a, 'd> {
    i2c: &'a mut I2cDriver<'d>,
    address: u8,
}

impl<'a, 'd> Device<'a, 'd> {
    fn new(i2c: &'a mut I2cDriver<'d>, address: u8) -> Self {
        Self { i2c, address }
    }

    fn write(&mut self, register: u8, value: u8) -> Result<()> {
        self.i2c
            .write(self.address, &[register, value], TIMEOUT_TICKS)
            .with_context(|| {
                format!("{:#04x} のレジスタ {register:#04x} に書けません", self.address)
            })?;
        Ok(())
    }

    fn read(&mut self, register: u8) -> Result<u8> {
        let mut buffer = [0_u8];
        self.i2c
            .write_read(self.address, &[register], &mut buffer, TIMEOUT_TICKS)
            .with_context(|| {
                format!("{:#04x} のレジスタ {register:#04x} を読めません", self.address)
            })?;
        Ok(buffer[0])
    }

    /// 読んでから書き戻す。同じレジスタにある他の設定を壊さないため。
    fn update(&mut self, register: u8, change: impl FnOnce(u8) -> u8) -> Result<()> {
        let current = self.read(register)?;
        self.write(register, change(current))
    }

    fn write_all(&mut self, settings: &[(u8, u8)]) -> Result<()> {
        for &(register, value) in settings {
            self.write(register, value)?;
        }
        Ok(())
    }
}

/// 周辺デバイスへの給電を開始する。
pub fn enable_peripherals(i2c: &mut I2cDriver) -> Result<()> {
    // 昇圧回路を先に入れないと後続のレールが立ち上がらない。
    Device::new(i2c, AW9523_ADDRESS)
        .update(0x03, |value| value | 0b1000_0000)
        .context("AW9523 の昇圧を有効にできません")?;

    Device::new(i2c, AXP2101_ADDRESS)
        .write_all(&[
            (0x90, 0xBF),                    // 各 LDO を有効にする
            (0x92, encode_decivolts(1_800)), // ALDO1 → スピーカーアンプ AW88298
            (0x93, encode_decivolts(3_300)), // ALDO2 → マイク ADC ES7210
            (0x94, encode_decivolts(3_300)), // ALDO3 → カメラ
            (0x95, encode_decivolts(3_300)), // ALDO4 → microSD
            (0x27, 0x00),                    // 電源ボタンの長押し時間
            (0x69, 0x11),                    // 充電表示 LED
            (0x10, 0x30),                    // PMU 共通設定
            (0x30, 0x0F),                    // 電池電圧の測定を有効にする
        ])
        .context("AXP2101 を初期化できません")?;

    Device::new(i2c, AW9523_ADDRESS)
        .write_all(&[
            (0x02, 0b0000_0111), // P0 出力値
            (0x03, 0b1000_0011), // P1 出力値。bit1 で LCD のリセットを解除する
            (0x04, 0b0001_1000), // P0 の入出力方向
            (0x05, 0b0000_1100), // P1 の入出力方向
            (0x11, 0b0001_0000), // P0 をプッシュプルにする
            (0x12, 0xFF),        // P0 の LED モードを使わない
            (0x13, 0xFF),        // P1 の LED モードを使わない
        ])
        .context("AW9523 を初期化できません")?;

    Ok(())
}

/// 画面の明るさを 0〜100 で指定する。
pub fn set_backlight(i2c: &mut I2cDriver, percent: u8) -> Result<()> {
    let percent = u32::from(percent.min(100));
    let millivolts = BACKLIGHT_MIN_MV + (BACKLIGHT_MAX_MV - BACKLIGHT_MIN_MV) * percent / 100;

    let mut axp = Device::new(i2c, AXP2101_ADDRESS);
    axp.write(0x99, encode_decivolts(millivolts))
        .context("バックライトの電圧を設定できません")?;
    // 電圧を決めても DLDO1 を有効にしないと点灯しない。
    axp.update(0x90, |value| value | 0b1000_0000)
        .context("バックライトの電源を入れられません")
}

/// SD カードスロットへの給電を切り替える。
pub fn set_sd_power(i2c: &mut I2cDriver, on: bool) -> Result<()> {
    Device::new(i2c, AW9523_ADDRESS)
        .update(0x02, |value| {
            if on {
                value | 0b0001_0000
            } else {
                value & !0b0001_0000
            }
        })
        .context("SDカードの電源を切り替えられません")
}
