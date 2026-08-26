//! M5Stack CoreS3 の公式 BSP を Rust から扱う。
//!
//! 電源IC・LCD・タッチ・SDカードの初期化は BSP に任せる。LCD の DC と
//! microSD の MISO が同じ端子を分け合う扱いも BSP 側で解決されている。

use anyhow::{bail, Context, Result};
use esp_idf_svc::sys::{bsp, esp};

/// SD カードが見える場所。BSP の設定値と一致させる必要がある。
pub const SD_MOUNT_POINT: &str = "/sdcard";

/// LVGL を操作してよい間だけ生きる鍵。落とすと描画側へ返す。
///
/// LVGL は別のタスクが描画しているため、部品をいじる間は必ず止める。
pub struct DisplayLock;

impl DisplayLock {
    /// 描画を止めて LVGL を操作する権利を得る。取れるまで待つ。
    pub fn acquire() -> Self {
        unsafe { bsp::bsp_display_lock(0) };
        Self
    }
}

impl Drop for DisplayLock {
    fn drop(&mut self) {
        unsafe { bsp::bsp_display_unlock() };
    }
}

/// 内部 I2C を立ち上げる。電源ICとタッチがこのバスにいる。
pub fn init_bus() -> Result<()> {
    esp!(unsafe { bsp::bsp_i2c_init() }).context("内部 I2C を初期化できません")?;
    Ok(())
}

/// LCD と LVGL を立ち上げる。タッチもここで LVGL に繋がれる。
pub fn start_display() -> Result<()> {
    if unsafe { bsp::bsp_display_start() }.is_null() {
        bail!("画面を初期化できません");
    }
    Ok(())
}

/// 画面の明るさを 0〜100 で指定する。
pub fn set_brightness(percent: u8) -> Result<()> {
    esp!(unsafe { bsp::bsp_display_brightness_set(i32::from(percent.min(100))) })
        .context("画面の明るさを変えられません")?;
    Ok(())
}

/// SD カードをマウントする。
pub fn mount_sd_card() -> Result<()> {
    esp!(unsafe { bsp::bsp_sdcard_mount() }).context(
        "SDカードを読めません。カードが入っているか、FAT32で初期化されているか確かめてください",
    )?;
    Ok(())
}

/// タッチの入力装置。LVGL が読み取りを担う。
pub fn touch_device() -> *mut bsp::lv_indev_t {
    unsafe { bsp::bsp_display_get_input_dev() }
}
