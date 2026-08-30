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

/// AXP2101 の I2C アドレス。内部 I2C バス（`init_bus()` が立ち上げる）に
/// BSP と相乗りする。同じアドレスに複数の `i2c_master_dev_handle_t` を
/// 持つこと自体はドライバ側で禁止されていない
/// （`esp_driver_i2c` は登録済みアドレスの重複を検査しない）。
const AXP2101_I2C_ADDR: u16 = 0x34;
/// AXP2101 の「共通設定」レジスタ。bit0 に1を立てると、RTC用の電源
/// （VRTC）を除くすべてのレールを切る（AXP2101 データシート、および
/// 実機で使われている `lewisxhe/XPowersLib` の `shutdown()` 実装を参照）。
const AXP2101_REG_COMMON_CONFIG: u8 = 0x10;
const AXP2101_SOFT_OFF_BIT: u8 = 0x01;

/// 電源を落とす。
///
/// `esp_deep_sleep_start()` だけでは ESP32-S3 のチップしか止まらず、
/// バックライトや周辺の電源レールは AXP2101 が給電し続けたままになる
/// （画面が点いたまま、実機によっては何かの拍子に起き上がって
/// 再起動したように見える不具合を確認済み）。そこで AXP2101 自身に
/// [`AXP2101_REG_COMMON_CONFIG`] のシャットダウンビットを立てさせ、
/// VRTC 以外のレール（ESP32-S3 本体の電源も含む）を実際に切る。
/// 復帰には実機の電源ボタンでの起動が要る。
///
/// AXP2101 の電源断は即座かつ完全なため、直前に保存した設定やログが
/// SDカードの書き込みキャッシュに残ったままだと失われる。電源を切る
/// 前に必ず [`unmount_sd_card()`] でファイルシステムを同期させておく
/// （実機で、明るさの保存が次回起動に反映されない不具合として確認済み）。
///
/// AXP2101 と通信できなかった場合だけ、保険として
/// `esp_deep_sleep_start()` にも落とす（起床要因は設定しないため、
/// 通常は電源ボタンでの再起動が要る点は変わらない）。
pub fn power_off() -> ! {
    unsafe {
        bsp::bsp_display_backlight_off();
        {
            let _lock = DisplayLock::acquire();
            bsp::bsp_display_enter_sleep();
        }
    }

    if let Err(error) = unmount_sd_card() {
        log::warn!("SDカードを同期できません（{error:#}）。設定の保存が失われる恐れがあります");
    }

    if let Err(error) = axp2101_shutdown() {
        log::warn!("AXP2101に電源を切らせられません（{error:#}）。deep sleepで代替します");
    }

    // AXP2101 のシャットダウンが成功していれば、この呼び出しへ実際に
    // たどり着く前に電源が切れる。ここに来るとすれば、切れるまでの
    // 一瞬の待ちか、AXP2101と通信できなかったときの保険。
    unsafe { esp_idf_svc::sys::esp_deep_sleep_start() }
}

/// AXP2101 自身にシャットダウンさせる。
///
/// BSP は AXP2101 用の取っ手を内部（`bsp_feature_en.c`）に静的に持つが
/// 外へは公開していないため、同じアドレスへ自分の取っ手を新たに作って使う。
fn axp2101_shutdown() -> Result<()> {
    let bus = unsafe { bsp::bsp_i2c_get_handle() };

    let config = bsp::i2c_device_config_t {
        dev_addr_length: bsp::i2c_addr_bit_len_t_I2C_ADDR_BIT_LEN_7,
        device_address: AXP2101_I2C_ADDR,
        scl_speed_hz: 100_000,
        scl_wait_us: 0,
        flags: Default::default(),
    };
    let mut device: bsp::i2c_master_dev_handle_t = std::ptr::null_mut();
    esp!(unsafe { bsp::i2c_master_bus_add_device(bus, &config, &mut device) })
        .context("AXP2101を掴めません")?;

    // 他のビットを保ったまま立てるため、読んでから書き戻す。
    let mut current = [0_u8; 1];
    esp!(unsafe {
        bsp::i2c_master_transmit_receive(
            device,
            [AXP2101_REG_COMMON_CONFIG].as_ptr(),
            1,
            current.as_mut_ptr(),
            1,
            1_000,
        )
    })
    .context("AXP2101のレジスタを読めません")?;

    let updated = [AXP2101_REG_COMMON_CONFIG, current[0] | AXP2101_SOFT_OFF_BIT];
    esp!(unsafe { bsp::i2c_master_transmit(device, updated.as_ptr(), updated.len(), 1_000) })
        .context("AXP2101に電源を切らせられません")?;

    Ok(())
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
