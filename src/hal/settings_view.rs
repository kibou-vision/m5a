//! 設定画面（各モジュールのステータス一覧と声の選択）を LVGL の部品として置く。
//!
//! [`super::face::FaceView`] と同じ考え方で、部品は起動時に一度だけ作り、
//! 以後は位置・文字・色を変えるだけにする。どこに何を置くかは
//! [`m5a_core::settings_layout`] が決め、ここは反映だけを行う。

use std::ffi::CString;

use esp_idf_svc::sys::bsp;
use m5a_core::layout::{Color, Rect};
use m5a_core::settings_layout::{
    self, BadgeSymbol, IconSymbol, SettingsLayout, StatusRow, VoiceOption,
};

/// 一覧表示できるモジュールの最大数。[`Module`] の種類数に合わせる。
const MAX_ROWS: usize = 6;
/// [`m5a_core::config::SUPPORTED_VOICES`] の数。
const MAX_VOICE_BUTTONS: usize = 10;

const MAIN_PART: u32 = 0;
const OPAQUE: u8 = 255;

const TEXT_COLOR: Color = Color::new(230, 235, 240);
const VOICE_TEXT_COLOR: Color = Color::new(240, 240, 240);

/// 画面に置かれた設定画面。
pub struct SettingsView {
    /// 部品をまとめる入れ物。アシスタント画面とはこれごと見せ隠しする。
    container: *mut bsp::lv_obj_t,
    rows: [ModuleRow; MAX_ROWS],
    voice_buttons: [*mut bsp::lv_obj_t; MAX_VOICE_BUTTONS],
    applied: Option<SettingsLayout>,
}

struct ModuleRow {
    icon: *mut bsp::lv_obj_t,
    message: *mut bsp::lv_obj_t,
    badge: *mut bsp::lv_obj_t,
}

impl SettingsView {
    /// 設定画面の部品を作る。呼ぶ前に画面の鍵を取っておくこと。
    pub fn create() -> Self {
        unsafe {
            let screen = bsp::lv_screen_active();
            let container = make_container(screen);

            let rows = std::array::from_fn(|_| {
                let icon = make_label(container);
                // アイコンだけ一回り大きいフォントで見せる。
                bsp::lv_obj_set_style_text_font(icon, &bsp::lv_font_montserrat_20, MAIN_PART);

                ModuleRow {
                    icon,
                    message: make_label(container),
                    badge: make_label(container),
                }
            });

            let voice_buttons = std::array::from_fn(|_| make_label(container));

            hide(container);

            Self {
                container,
                rows,
                voice_buttons,
                applied: None,
            }
        }
    }

    /// 直前と同じ配置なら `apply` は何もしない仕組みのため、隠している
    /// 間に憶えていた配置を捨て、次の `apply` で必ず見せ直させる。
    pub fn show(&mut self) {
        unsafe { show(self.container) };
        self.applied = None;
    }

    pub fn hide(&self) {
        unsafe { hide(self.container) };
    }

    /// 配置を画面に反映する。前回と同じなら何もしない。
    pub fn apply(&mut self, layout: &SettingsLayout) {
        if self.applied.as_ref() == Some(layout) {
            return;
        }

        unsafe { self.write(layout) };
        self.applied = Some(layout.clone());
    }

    unsafe fn write(&mut self, layout: &SettingsLayout) {
        bsp::lv_obj_set_style_bg_color(self.container, color_of(layout.background), MAIN_PART);

        for (index, row) in self.rows.iter().enumerate() {
            match layout.rows.get(index) {
                Some(status_row) => write_row(row, status_row),
                None => hide_row(row),
            }
        }

        let options: &[VoiceOption] = layout
            .voice_picker
            .as_ref()
            .map(|picker| picker.options.as_slice())
            .unwrap_or(&[]);

        for (index, button) in self.voice_buttons.iter().enumerate() {
            match options.get(index) {
                Some(option) => write_voice_button(*button, option),
                None => hide(*button),
            }
        }
    }
}

unsafe fn write_row(row: &ModuleRow, status_row: &StatusRow) {
    place(row.icon, status_row.icon);
    set_text(row.icon, icon_glyph(status_row.icon_symbol));
    bsp::lv_obj_set_style_text_color(row.icon, color_of(TEXT_COLOR), MAIN_PART);
    show(row.icon);

    place(row.message, status_row.message_area);
    set_text(row.message, &status_row.message);
    bsp::lv_obj_set_style_text_color(row.message, color_of(status_row.color), MAIN_PART);
    show(row.message);

    place(row.badge, status_row.badge);
    set_text(row.badge, badge_glyph(status_row.badge_symbol));
    bsp::lv_obj_set_style_text_color(row.badge, color_of(status_row.color), MAIN_PART);
    show(row.badge);
}

unsafe fn hide_row(row: &ModuleRow) {
    hide(row.icon);
    hide(row.message);
    hide(row.badge);
}

unsafe fn write_voice_button(button: *mut bsp::lv_obj_t, option: &VoiceOption) {
    place(button, option.area);
    set_text(button, option.voice);
    bsp::lv_obj_set_style_text_color(button, color_of(VOICE_TEXT_COLOR), MAIN_PART);
    bsp::lv_obj_set_style_bg_color(
        button,
        color_of(settings_layout::voice_button_color(option)),
        MAIN_PART,
    );
    bsp::lv_obj_set_style_text_align(button, bsp::lv_text_align_t_LV_TEXT_ALIGN_CENTER, MAIN_PART);
    show(button);
}

/// 文字だけの板を作る。ラベルにも、色を塗るボタンにも使う。
unsafe fn make_label(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let label = bsp::lv_label_create(parent);

    bsp::lv_obj_set_style_bg_opa(label, OPAQUE, MAIN_PART);
    bsp::lv_obj_set_style_bg_color(label, color_of(Color::new(0, 0, 0)), MAIN_PART);
    bsp::lv_obj_set_style_pad_top(label, 0, MAIN_PART);
    bsp::lv_obj_set_style_pad_bottom(label, 0, MAIN_PART);
    bsp::lv_obj_set_style_pad_left(label, 0, MAIN_PART);
    bsp::lv_obj_set_style_pad_right(label, 0, MAIN_PART);
    bsp::lv_obj_remove_flag(
        label,
        bsp::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE | bsp::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE,
    );
    hide(label);

    label
}

/// 設定画面全体をまとめる、画面いっぱいの入れ物を作る。
unsafe fn make_container(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let container = bsp::lv_obj_create(parent);

    bsp::lv_obj_remove_style_all(container);
    bsp::lv_obj_set_style_bg_opa(container, OPAQUE, MAIN_PART);
    bsp::lv_obj_set_size(
        container,
        i32::from(m5a_core::layout::SCREEN_WIDTH),
        i32::from(m5a_core::layout::SCREEN_HEIGHT),
    );
    bsp::lv_obj_set_pos(container, 0, 0);
    // 一覧が画面高さを超える構成（WebSearchを含む場合）もあるため、
    // 縦方向のスクロールだけ許す。
    bsp::lv_obj_set_scroll_dir(container, bsp::lv_dir_t_LV_DIR_VER);
    bsp::lv_obj_remove_flag(container, bsp::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);

    container
}

unsafe fn place(object: *mut bsp::lv_obj_t, area: Rect) {
    bsp::lv_obj_set_pos(object, i32::from(area.x), i32::from(area.y));
    bsp::lv_obj_set_size(object, i32::from(area.width), i32::from(area.height));
}

unsafe fn set_text(object: *mut bsp::lv_obj_t, text: &str) {
    // LVGL は末尾に \0 を持つ C 文字列を要る。毎フレーム作り直すのは
    // 内容が変わったときだけなので、確保のコストは無視できる。
    if let Ok(text) = CString::new(text) {
        bsp::lv_label_set_text(object, text.as_ptr());
    }
}

unsafe fn show(object: *mut bsp::lv_obj_t) {
    bsp::lv_obj_remove_flag(object, bsp::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
}

unsafe fn hide(object: *mut bsp::lv_obj_t) {
    bsp::lv_obj_add_flag(object, bsp::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
}

fn color_of(color: Color) -> bsp::lv_color_t {
    unsafe { bsp::lv_color_make(color.r, color.g, color.b) }
}

/// モジュールを表す LVGL 組み込みシンボル。バイト列は LVGL 9.5.0 の
/// `lv_symbol_def.h` に定義された値をそのまま写している。
fn icon_glyph(symbol: IconSymbol) -> &'static str {
    match symbol {
        IconSymbol::Display => "\u{F03E}",         // LV_SYMBOL_IMAGE
        IconSymbol::SdCard => "\u{F7C2}",           // LV_SYMBOL_SD_CARD
        IconSymbol::Microphone => "\u{F001}",       // LV_SYMBOL_AUDIO
        IconSymbol::Wifi => "\u{F1EB}",             // LV_SYMBOL_WIFI
        IconSymbol::RealtimeSession => "\u{F095}",  // LV_SYMBOL_CALL
        IconSymbol::WebSearch => "\u{F124}",        // LV_SYMBOL_GPS
    }
}

/// 行の右端に重ねる状態バッジのシンボル。
fn badge_glyph(symbol: BadgeSymbol) -> &'static str {
    match symbol {
        BadgeSymbol::Checking => "\u{F079}", // LV_SYMBOL_LOOP
        BadgeSymbol::Ok => "\u{F00C}",       // LV_SYMBOL_OK
        BadgeSymbol::Warning => "\u{F071}",  // LV_SYMBOL_WARNING
    }
}
