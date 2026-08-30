//! 設定画面（各モジュールのステータス一覧と声の選択）を LVGL の部品として置く。
//!
//! [`super::face::FaceView`] と同じ考え方で、部品は起動時に一度だけ作り、
//! 以後は位置・文字・色を変えるだけにする。どこに何を置くかは
//! [`m5a_core::settings_layout`] が決め、ここは反映だけを行う。

use std::ffi::CString;

use esp_idf_svc::sys::bsp;
use m5a_core::config::SUPPORTED_VOICES;
use m5a_core::layout::{Color, Rect};
use m5a_core::settings_layout::{BadgeSymbol, IconSymbol, SettingsLayout, SliderSpec, StatusRow};

/// 一覧表示できるモジュールの最大数。[`Module`] の種類数に合わせる。
const MAX_ROWS: usize = 7;

const MAIN_PART: u32 = 0;
const INDICATOR_PART: u32 = bsp::lv_part_t_LV_PART_INDICATOR;
const KNOB_PART: u32 = bsp::lv_part_t_LV_PART_KNOB;
const OPAQUE: u8 = 255;

const TEXT_COLOR: Color = Color::new(230, 235, 240);
/// スライダーの色。準備完了を示す色と揃える。
const SLIDER_COLOR: Color = Color::new(90, 200, 120);

/// スライダーを読み取った結果。
pub struct SliderReading {
    pub value: i32,
    /// 指を離した直後で、SDカードへ保存してよいタイミング。
    pub released: bool,
}

/// 画面に置かれた設定画面。
pub struct SettingsView {
    /// 部品をまとめる入れ物。アシスタント画面とはこれごと見せ隠しする。
    container: *mut bsp::lv_obj_t,
    rows: [ModuleRow; MAX_ROWS],
    /// 声を選ぶコンボボックス。スピーカーの行にだけ並べて置く。
    voice_combo: *mut bsp::lv_obj_t,
    /// アシスタント画面へ戻る閉じるボタン。
    close_button: *mut bsp::lv_obj_t,
    applied: Option<SettingsLayout>,
    speaker_was_pressed: bool,
    mic_was_pressed: bool,
}

struct ModuleRow {
    icon: *mut bsp::lv_obj_t,
    message: *mut bsp::lv_obj_t,
    badge: *mut bsp::lv_obj_t,
    slider: *mut bsp::lv_obj_t,
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
                    slider: make_slider(container),
                }
            });

            let voice_combo = make_combo(container);
            let close_button = make_label(container);
            bsp::lv_obj_set_style_text_font(close_button, &bsp::lv_font_montserrat_20, MAIN_PART);

            hide(container);

            Self {
                container,
                rows,
                voice_combo,
                close_button,
                applied: None,
                speaker_was_pressed: false,
                mic_was_pressed: false,
            }
        }
    }

    /// 声のコンボボックスで選ばれている声。表示されていなければ `None`。
    pub fn voice_selection(&self) -> Option<&'static str> {
        self.applied.as_ref()?.voice_picker.as_ref()?;
        let index = unsafe { bsp::lv_dropdown_get_selected(self.voice_combo) } as usize;
        SUPPORTED_VOICES.get(index).copied()
    }

    /// スピーカー音量スライダーの現在値。表示されていなければ `None`。
    pub fn speaker_volume(&mut self) -> Option<SliderReading> {
        let slider = self.slider_of(IconSymbol::Speaker)?;
        Some(unsafe { read_slider(slider, &mut self.speaker_was_pressed) })
    }

    /// マイク感度スライダーの現在値。表示されていなければ `None`。
    pub fn mic_gain(&mut self) -> Option<SliderReading> {
        let slider = self.slider_of(IconSymbol::Microphone)?;
        Some(unsafe { read_slider(slider, &mut self.mic_was_pressed) })
    }

    /// 直前に反映したレイアウトから、指定したモジュールの行が
    /// いま持っているスライダーの部品を探す。
    fn slider_of(&self, symbol: IconSymbol) -> Option<*mut bsp::lv_obj_t> {
        let index = self
            .applied
            .as_ref()?
            .rows
            .iter()
            .position(|row| row.icon_symbol == symbol && row.slider.is_some())?;
        Some(self.rows[index].slider)
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

        match layout.voice_picker.as_ref() {
            Some(picker) => {
                place(self.voice_combo, picker.area);
                bsp::lv_dropdown_set_selected(self.voice_combo, picker.selected_index as u32);
                show(self.voice_combo);
            }
            None => hide(self.voice_combo),
        }

        place(self.close_button, layout.close_button);
        set_text(self.close_button, "\u{F00D}"); // LV_SYMBOL_CLOSE
        bsp::lv_obj_set_style_text_color(self.close_button, color_of(TEXT_COLOR), MAIN_PART);
        bsp::lv_obj_set_style_text_align(
            self.close_button,
            bsp::lv_text_align_t_LV_TEXT_ALIGN_CENTER,
            MAIN_PART,
        );
        show(self.close_button);
    }
}

unsafe fn write_row(row: &ModuleRow, status_row: &StatusRow) {
    place(row.icon, status_row.icon);
    set_text(row.icon, icon_glyph(status_row.icon_symbol));
    bsp::lv_obj_set_style_text_color(row.icon, color_of(TEXT_COLOR), MAIN_PART);
    show(row.icon);

    // スライダーがある行では、状態文の代わりにスライダーを見せる。
    match status_row.slider {
        Some(slider) => {
            hide(row.message);
            write_slider(row.slider, slider);
        }
        None => {
            hide(row.slider);
            place(row.message, status_row.message_area);
            set_text(row.message, &status_row.message);
            bsp::lv_obj_set_style_text_color(row.message, color_of(status_row.color), MAIN_PART);
            show(row.message);
        }
    }

    place(row.badge, status_row.badge);
    set_text(row.badge, badge_glyph(status_row.badge_symbol));
    bsp::lv_obj_set_style_text_color(row.badge, color_of(status_row.color), MAIN_PART);
    show(row.badge);
}

unsafe fn hide_row(row: &ModuleRow) {
    hide(row.icon);
    hide(row.message);
    hide(row.badge);
    hide(row.slider);
}

/// スライダーの位置・範囲・値を反映する。
///
/// ドラッグ中は毎コマ `apply` から呼ばれる（値が動くたびにレイアウトも
/// 変わるため）。ただし `main.rs` はこのコマの描画を作る前に
/// `SettingsView::speaker_volume`/`mic_gain` でつまみの現在値を読み、
/// それを渡した上でレイアウトを作り直しているため、ここで書き戻す値は
/// つまみが今まさに指している値と一致する。つまみの動きを妨げない。
unsafe fn write_slider(slider: *mut bsp::lv_obj_t, spec: SliderSpec) {
    place(slider, spec.area);
    bsp::lv_slider_set_range(slider, spec.min, spec.max);
    bsp::lv_slider_set_value(slider, spec.value, false);
    show(slider);
}

/// スライダーの現在値と、指を離した直後かどうかを返す。
/// 隠れている間は指を離した扱いにし、再表示時に誤検知しないようにする。
unsafe fn read_slider(slider: *mut bsp::lv_obj_t, was_pressed: &mut bool) -> SliderReading {
    let pressed = bsp::lv_obj_has_state(slider, bsp::lv_state_t_LV_STATE_PRESSED);
    let released = *was_pressed && !pressed;
    *was_pressed = pressed;

    SliderReading {
        value: bsp::lv_slider_get_value(slider),
        released,
    }
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

/// 音量・感度を変えるスライダーを作る。ドラッグの検出・つまみの描画は
/// LVGL の `lv_slider` にまかせ、ここでは色と初期状態だけを整える。
unsafe fn make_slider(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let slider = bsp::lv_slider_create(parent);

    bsp::lv_obj_set_style_bg_color(slider, color_of(SLIDER_COLOR), INDICATOR_PART);
    bsp::lv_obj_set_style_bg_color(slider, color_of(SLIDER_COLOR), KNOB_PART);
    hide(slider);

    slider
}

/// 声を選ぶコンボボックスを作る。開閉・選択の当たり判定・一覧の描画は
/// すべて LVGL の `lv_dropdown` にまかせ、ここでは選択肢を渡すだけにする。
/// `SUPPORTED_VOICES` の並びは変わらないため、選択肢は最初の一回だけ渡す。
unsafe fn make_combo(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let dropdown = bsp::lv_dropdown_create(parent);

    let options = SUPPORTED_VOICES.join("\n");
    if let Ok(options) = CString::new(options) {
        bsp::lv_dropdown_set_options(dropdown, options.as_ptr());
    }
    hide(dropdown);

    dropdown
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
        IconSymbol::Speaker => "\u{F028}",          // LV_SYMBOL_VOLUME_MAX
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
