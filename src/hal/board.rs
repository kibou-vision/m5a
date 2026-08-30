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

/// SD カードを安全に取り外せる状態にする。
///
/// `fs::write` が成功を返しても、SDカード自身の書き込みキャッシュに
/// データが残ったままのことがある。電源を断つ前にアンマウントして
/// ファイルシステムをきちんと同期させないと、直前に保存したはずの
/// 設定やログが失われる（実機で確認済み。`power_off()` 参照）。
pub fn unmount_sd_card() -> Result<()> {
    esp!(unsafe { bsp::bsp_sdcard_unmount() }).context("SDカードを片付けられません")?;
    Ok(())
}

/// タッチの入力装置。LVGL が読み取りを担う。
pub fn touch_device() -> *mut bsp::lv_indev_t {
    unsafe { bsp::bsp_display_get_input_dev() }
}

/// 電源を落とす（実際には deep sleep で代用する）。
///
/// AXP2101 の「共通設定」レジスタ（0x10）の bit0 を立てて本体ごと電源を
/// 切る方式も試したが、それ以前に**毎回別の原因でクラッシュしてリブート
/// していた**ことが実機のパニックログで判明した。`bsp_display_enter_sleep()`
/// は内部でタッチパネルもスリープさせようとするが、この機種のタッチ
/// ドライバはスリープに対応しておらず`ESP_FAIL`を返す。BSP側はこれを
/// `ESP_ERROR_CHECK`で無条件に`abort()`させる実装になっているため、
/// `bsp_display_enter_sleep()`を呼ぶたびに必ずパニック→リブートしていた
/// （詳細は [CoreS3 の制約](../../docs/design/hardware.md) 参照）。
/// そのためパネルのスリープは諦め、`bsp_display_backlight_off()`による
/// バックライト消灯だけで画面を暗くする。
///
/// 電源を切る方式は `esp_deep_sleep_start()` による代用とする。起床要因は
/// 一切設定しないため、目覚めは実機の電源ボタンによる起動に任せる。
///
/// deep sleep 中も設定やログの保存が失われないよう、眠る前に必ず
/// [`unmount_sd_card()`] でファイルシステムを同期させる
/// （実機で、明るさの保存が次回起動に反映されない不具合として確認済み）。
pub fn power_off() -> ! {
    unsafe { bsp::bsp_display_backlight_off() };

    if let Err(error) = unmount_sd_card() {
        log::warn!("SDカードを同期できません（{error:#}）。設定の保存が失われる恐れがあります");
    }

    unsafe { esp_idf_svc::sys::esp_deep_sleep_start() }
}

/// いま使える内部メモリの様子。
///
/// LVGL の描画バッファ・Wi-Fi・音声が内部 DRAM を大きく使うため、
/// PSRAM が空いていても仕事や DMA バッファを作れなくなることがある。
pub fn report_memory(stage: &str) {
    let free = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
    let internal = largest_free_block(esp_idf_svc::sys::MALLOC_CAP_INTERNAL);
    let dma = dma_headroom();

    log::info!(
        "メモリ[{stage}]: 空き {free} / 内部の最大連続 {internal} / DMA の最大連続 {dma}"
    );
}

/// DMA に使える連続した空き。SD カードへの書き込みはここから確保される。
pub fn dma_headroom() -> usize {
    largest_free_block(esp_idf_svc::sys::MALLOC_CAP_DMA)
}

fn largest_free_block(capability: u32) -> usize {
    unsafe { esp_idf_svc::sys::heap_caps_get_largest_free_block(capability) }
}
