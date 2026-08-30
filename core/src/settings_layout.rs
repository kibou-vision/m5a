//! 設定画面（各モジュールのステータス一覧と声の選択）をどこにどう置くかを決める。
//!
//! [`crate::layout`] の顔画面と同じ考え方で、画素を塗るのは LVGL に任せ、
//! ここでは配置だけを純関数で決める。

use crate::config::SUPPORTED_VOICES;
use crate::layout::{Color, Point, Rect, SCREEN_WIDTH};
use crate::module_status::{Module, ModuleStatus, ModuleStatuses};

/// 一覧の外側の余白。
const MARGIN: i16 = 6;
/// 画面いちばん上、閉じるボタンだけのための帯の高さ。
const HEADER_HEIGHT: i16 = 20;
/// モジュール1行分の高さ。標準構成（WebSearch無効・6行）なら、
/// ヘッダーぶんを差し引いても画面高さ240に収まる大きさにしてある。
const ROW_HEIGHT: u16 = 34;
/// アイコンの正方形の一辺。
const ICON_SIZE: u16 = 26;
/// アイコンと文字の間隔。
const ICON_GAP: i16 = 8;
/// バッジ（読込中／OK／警告）の一辺。
const BADGE_SIZE: u16 = 14;

/// スライダー本体の太さ。行の高さの中で縦方向に中央寄せする。
const SLIDER_HEIGHT: u16 = 12;
/// スピーカー音量スライダーの範囲（百分率）。`hal::audio` 側の
/// クランプにも使うため公開する。
pub const SPEAKER_VOLUME_MIN: i32 = 0;
pub const SPEAKER_VOLUME_MAX: i32 = 100;
/// マイク感度スライダーの範囲（dB）。
pub const MIC_GAIN_MIN: i32 = 0;
pub const MIC_GAIN_MAX: i32 = 42;
/// 画面の明るさスライダーの範囲（百分率）。下限を30%に留めているのは、
/// それ未満まで暗くすると実機で表示がほとんど見えなくなるため。
pub const BRIGHTNESS_MIN: i32 = 30;
pub const BRIGHTNESS_MAX: i32 = 100;

/// 声を選ぶコンボボックスの大きさ。スピーカーの音量スライダーと
/// 同じ行に並べるため、スライダーはその分だけ幅を譲る。
const VOICE_COMBO_WIDTH: u16 = 96;
const VOICE_COMBO_HEIGHT: u16 = 22;
const VOICE_COMBO_GAP: i16 = 6;

/// 閉じるボタンの一辺。
const CLOSE_BUTTON_SIZE: u16 = 20;

const READY_COLOR: Color = Color::new(90, 200, 120);
const ERROR_COLOR: Color = Color::new(230, 90, 70);
const PENDING_COLOR: Color = Color::new(120, 190, 255);

/// アイコンとして使うLVGLシンボルの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSymbol {
    Display,
    SdCard,
    Microphone,
    Speaker,
    Wifi,
    RealtimeSession,
    WebSearch,
}

impl IconSymbol {
    fn of(module: Module) -> Self {
        match module {
            Module::Display => Self::Display,
            Module::SdCard => Self::SdCard,
            Module::Microphone => Self::Microphone,
            Module::Speaker => Self::Speaker,
            Module::Wifi => Self::Wifi,
            Module::RealtimeSession => Self::RealtimeSession,
            Module::WebSearch => Self::WebSearch,
        }
    }
}

/// 行の右端に重ねる小さな状態バッジ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSymbol {
    Checking,
    Ok,
    Warning,
}

impl BadgeSymbol {
    fn of(status: &ModuleStatus) -> Self {
        match status {
            ModuleStatus::NotChecked | ModuleStatus::Checking => Self::Checking,
            ModuleStatus::Ready => Self::Ok,
            ModuleStatus::Error => Self::Warning,
        }
    }
}

/// モジュール1行分の配置。
///
/// アイコンだけでどのモジュールかは伝わるため、モジュール名の文字は
/// 出さない。状態文（`message`）だけをアイコンの横に置く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub icon: Rect,
    pub icon_symbol: IconSymbol,
    pub message_area: Rect,
    pub badge: Rect,
    pub badge_symbol: BadgeSymbol,
    /// 準備できていれば空文字。エラーなら「何が起きたか＋どう直すか」。
    /// スライダーがある行では、準備できた時点でこちらも空文字にする。
    pub message: String,
    pub color: Color,
    /// 画面・スピーカー・マイクの行が準備完了になったときだけ現れる、
    /// 明るさ・音量・感度を変えるスライダー。`message` の位置を置き換える。
    pub slider: Option<SliderSpec>,
}

/// アイコンの横、状態文の位置に置くスライダー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderSpec {
    pub area: Rect,
    pub min: i32,
    pub max: i32,
    pub value: i32,
}

/// 声を選ぶコンボボックス。話す相手（RealtimeSession）の準備ができ、
/// スピーカーの行が描かれたときだけ現れる。当たり判定・開閉・選択の
/// 描画は LVGL の `lv_dropdown` にまかせ、ここでは配置と選択肢だけを持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePicker {
    pub area: Rect,
    pub options: &'static [&'static str],
    /// `options` の中で選択中の位置。
    pub selected_index: usize,
}

/// 設定画面ひとコマ分の配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLayout {
    pub background: Color,
    pub rows: Vec<StatusRow>,
    pub voice_picker: Option<VoicePicker>,
    /// アシスタント画面へ戻るための閉じるボタン。常に右上に置く。
    pub close_button: Rect,
}

/// 設定画面の配置を決める。
///
/// `current_voice` は選択中の声。`SUPPORTED_VOICES` に含まれない値が渡された
/// 場合でも一覧はそのまま描き、先頭の声を選択中として扱う（`Config::validate()`
/// を通った設定は必ず `SUPPORTED_VOICES` に含まれるため、実際には起こらない）。
/// `brightness`（百分率）・`speaker_volume`（百分率）・`mic_gain_db`（dB）は、
/// それぞれの行が準備完了になったときにスライダーへ渡す現在値。
pub fn lay_out_settings(
    statuses: &ModuleStatuses,
    current_voice: &str,
    brightness: u8,
    speaker_volume: u8,
    mic_gain_db: u8,
) -> SettingsLayout {
    let entries = statuses.entries();
    let realtime_ready = statuses.realtime_session.is_ready();
    let mut rows = Vec::with_capacity(entries.len());
    let mut voice_picker = None;

    for (index, (module, status)) in entries.iter().enumerate() {
        let y = HEADER_HEIGHT + MARGIN + index as i16 * ROW_HEIGHT as i16;
        let icon = Rect {
            x: MARGIN,
            y: y + (ROW_HEIGHT as i16 - ICON_SIZE as i16) / 2,
            width: ICON_SIZE,
            height: ICON_SIZE,
        };
        let text_x = icon.right() + ICON_GAP;
        let text_width = (SCREEN_WIDTH - text_x - MARGIN - BADGE_SIZE as i16 - ICON_GAP) as u16;
        let message_area = Rect {
            x: text_x,
            y,
            width: text_width,
            height: ROW_HEIGHT,
        };
        let badge = Rect {
            x: SCREEN_WIDTH - MARGIN - BADGE_SIZE as i16,
            y: y + (ROW_HEIGHT as i16 - BADGE_SIZE as i16) / 2,
            width: BADGE_SIZE,
            height: BADGE_SIZE,
        };

        // 声のコンボボックスは、スピーカーの行にだけ音量スライダーと並べて置く。
        let show_combo = *module == Module::Speaker && realtime_ready;
        if show_combo {
            voice_picker = Some(VoicePicker {
                area: combo_area(message_area),
                options: &SUPPORTED_VOICES,
                selected_index: SUPPORTED_VOICES
                    .iter()
                    .position(|voice| *voice == current_voice)
                    .unwrap_or(0),
            });
        }
        let slider_width = if show_combo {
            message_area
                .width
                .saturating_sub(VOICE_COMBO_WIDTH + VOICE_COMBO_GAP as u16)
        } else {
            message_area.width
        };

        let slider = status
            .is_ready()
            .then(|| {
                slider_of(
                    *module,
                    message_area,
                    slider_width,
                    brightness,
                    speaker_volume,
                    mic_gain_db,
                )
            })
            .flatten();
        let message = if slider.is_some() { String::new() } else { message_of(status) };

        rows.push(StatusRow {
            icon,
            icon_symbol: IconSymbol::of(*module),
            message_area,
            badge,
            badge_symbol: BadgeSymbol::of(status),
            message,
            color: color_of(status),
            slider,
        });
    }

    let close_button = Rect {
        x: SCREEN_WIDTH - MARGIN - CLOSE_BUTTON_SIZE as i16,
        y: (HEADER_HEIGHT - CLOSE_BUTTON_SIZE as i16) / 2,
        width: CLOSE_BUTTON_SIZE,
        height: CLOSE_BUTTON_SIZE,
    };

    SettingsLayout {
        background: Color::new(12, 16, 24),
        rows,
        voice_picker,
        close_button,
    }
}

/// タップ位置が閉じるボタンに当たったか。
pub fn close_button_at(layout: &SettingsLayout, at: Point) -> bool {
    contains(&layout.close_button, at)
}

fn contains(rect: &Rect, at: Point) -> bool {
    (rect.x..rect.right()).contains(&at.x) && (rect.y..rect.bottom()).contains(&at.y)
}

/// スピーカーの状態文の位置のうち、右端をコンボボックスに割り当てる。
fn combo_area(message_area: Rect) -> Rect {
    Rect {
        x: message_area.right() - VOICE_COMBO_WIDTH as i16,
        y: message_area.y + (message_area.height as i16 - VOICE_COMBO_HEIGHT as i16) / 2,
        width: VOICE_COMBO_WIDTH,
        height: VOICE_COMBO_HEIGHT,
    }
}

/// 画面は英語の短い単語だけを出す。何が起きてどう直すかの詳しい理由は
/// シリアルログにだけ残す（実機に日本語フォントを組み込んでおらず、
/// 子ども向け画面に長い英文を出しても読めないため）。
fn message_of(status: &ModuleStatus) -> String {
    match status {
        ModuleStatus::NotChecked => String::new(),
        ModuleStatus::Checking => "Checking...".to_string(),
        ModuleStatus::Ready => "Ready".to_string(),
        ModuleStatus::Error => "Failed".to_string(),
    }
}

/// 準備完了になった行のうち、画面・スピーカー・マイクだけにスライダーを与える。
/// `width` はコンボボックスと並べる分だけ削られていることがある。
fn slider_of(
    module: Module,
    message_area: Rect,
    width: u16,
    brightness: u8,
    speaker_volume: u8,
    mic_gain_db: u8,
) -> Option<SliderSpec> {
    let area = Rect {
        x: message_area.x,
        y: message_area.y + (message_area.height as i16 - SLIDER_HEIGHT as i16) / 2,
        width,
        height: SLIDER_HEIGHT,
    };

    match module {
        Module::Display => Some(SliderSpec {
            area,
            min: BRIGHTNESS_MIN,
            max: BRIGHTNESS_MAX,
            value: i32::from(brightness).clamp(BRIGHTNESS_MIN, BRIGHTNESS_MAX),
        }),
        Module::Speaker => Some(SliderSpec {
            area,
            min: SPEAKER_VOLUME_MIN,
            max: SPEAKER_VOLUME_MAX,
            value: i32::from(speaker_volume).clamp(SPEAKER_VOLUME_MIN, SPEAKER_VOLUME_MAX),
        }),
        Module::Microphone => Some(SliderSpec {
            area,
            min: MIC_GAIN_MIN,
            max: MIC_GAIN_MAX,
            value: i32::from(mic_gain_db).clamp(MIC_GAIN_MIN, MIC_GAIN_MAX),
        }),
        _ => None,
    }
}

fn color_of(status: &ModuleStatus) -> Color {
    match status {
        ModuleStatus::NotChecked | ModuleStatus::Checking => PENDING_COLOR,
        ModuleStatus::Ready => READY_COLOR,
        ModuleStatus::Error => ERROR_COLOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::SCREEN_HEIGHT;
    use crate::module_status::ModuleStatuses;

    /// テストで使う既定の明るさ・スピーカー音量・マイク感度。
    const BRIGHTNESS: u8 = 50;
    const VOLUME: u8 = 80;
    const GAIN_DB: u8 = 36;

    fn ready_statuses() -> ModuleStatuses {
        let mut statuses = ModuleStatuses::booting();
        statuses.sd_card = ModuleStatus::Ready;
        statuses.microphone = ModuleStatus::Ready;
        statuses.speaker = ModuleStatus::Ready;
        statuses.wifi = ModuleStatus::Ready;
        statuses.realtime_session = ModuleStatus::Ready;
        statuses
    }

    fn layout_of(statuses: &ModuleStatuses, current_voice: &str) -> SettingsLayout {
        lay_out_settings(statuses, current_voice, BRIGHTNESS, VOLUME, GAIN_DB)
    }

    /// モジュールの数が多い（WebSearchを含む）場合、縦方向は画面高さを
    /// 超えることがある。設定画面はスクロールを許すため、横方向だけを
    /// 画面内に収める規則として検証する。
    #[test]
    fn everything_stays_within_the_screen_width() {
        let mut statuses = ready_statuses();
        statuses.web_search = Some(ModuleStatus::Error);
        let layout = layout_of(&statuses, "marin");

        for row in &layout.rows {
            assert!(row.icon.x >= 0 && row.icon.right() <= SCREEN_WIDTH);
            assert!(row.message_area.right() <= SCREEN_WIDTH);
            assert!(row.badge.right() <= SCREEN_WIDTH);
        }
        let picker = layout.voice_picker.expect("realtime is ready");
        assert!(picker.area.x >= 0 && picker.area.right() <= SCREEN_WIDTH);
        assert!(layout.close_button.x >= 0 && layout.close_button.right() <= SCREEN_WIDTH);
    }

    /// モジュール数が少ない標準構成（WebSearch無効）では、声の一覧まで
    /// 含めて画面の高さにも収まる。
    #[test]
    fn typical_layout_without_web_search_fits_the_screen_height() {
        let layout = layout_of(&ready_statuses(), "marin");

        let lowest = layout
            .rows
            .iter()
            .map(|row| row.message_area.bottom())
            .max()
            .unwrap_or(0);
        assert!(lowest <= SCREEN_HEIGHT);
    }

    #[test]
    fn web_search_row_appears_only_when_configured() {
        let without_search = layout_of(&ready_statuses(), "marin");
        assert_eq!(without_search.rows.len(), 6);

        let mut with_search = ready_statuses();
        with_search.web_search = Some(ModuleStatus::Ready);
        let with_search = layout_of(&with_search, "marin");
        assert_eq!(with_search.rows.len(), 7);
    }

    #[test]
    fn voice_picker_appears_only_when_realtime_session_is_ready() {
        let mut statuses = ready_statuses();
        statuses.realtime_session = ModuleStatus::Checking;
        let layout = layout_of(&statuses, "marin");

        assert!(layout.voice_picker.is_none());
    }

    #[test]
    fn voice_picker_lists_every_supported_voice_and_marks_the_current_one() {
        let layout = layout_of(&ready_statuses(), "cedar");
        let picker = layout.voice_picker.expect("realtime is ready");

        assert_eq!(picker.options, SUPPORTED_VOICES);
        assert_eq!(picker.options[picker.selected_index], "cedar");
    }

    #[test]
    fn voice_picker_defaults_to_the_first_option_for_an_unknown_voice() {
        let layout = layout_of(&ready_statuses(), "does-not-exist");
        let picker = layout.voice_picker.expect("realtime is ready");

        assert_eq!(picker.selected_index, 0);
    }

    #[test]
    fn close_button_hits_only_its_own_area() {
        let layout = layout_of(&ready_statuses(), "marin");
        let inside = Point::new(layout.close_button.x + 1, layout.close_button.y + 1);
        let outside = Point::new(0, SCREEN_HEIGHT - 1);

        assert!(close_button_at(&layout, inside));
        assert!(!close_button_at(&layout, outside));
    }

    #[test]
    fn error_status_shows_a_short_failed_label() {
        let mut statuses = ready_statuses();
        statuses.wifi = ModuleStatus::Error;
        let layout = layout_of(&statuses, "marin");

        let wifi_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Wifi)
            .expect("wifi row exists");
        assert_eq!(wifi_row.message, "Failed");
    }

    #[test]
    fn display_speaker_and_microphone_show_a_slider_once_ready() {
        let layout = layout_of(&ready_statuses(), "marin");

        let display_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Display)
            .expect("display row exists");
        let slider = display_row.slider.expect("display is always ready");
        assert_eq!(slider.value, i32::from(BRIGHTNESS));
        assert_eq!((slider.min, slider.max), (30, 100));
        assert!(display_row.message.is_empty());

        let speaker_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Speaker)
            .expect("speaker row exists");
        let slider = speaker_row.slider.expect("speaker is ready");
        assert_eq!(slider.value, i32::from(VOLUME));
        assert_eq!((slider.min, slider.max), (0, 100));
        assert!(speaker_row.message.is_empty());

        let mic_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Microphone)
            .expect("microphone row exists");
        let slider = mic_row.slider.expect("microphone is ready");
        assert_eq!(slider.value, i32::from(GAIN_DB));
        assert!(mic_row.message.is_empty());
    }

    #[test]
    fn slider_is_absent_until_the_module_is_ready() {
        let mut statuses = ready_statuses();
        statuses.microphone = ModuleStatus::Checking;
        statuses.speaker = ModuleStatus::Error;
        let layout = layout_of(&statuses, "marin");

        let mic_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Microphone)
            .expect("microphone row exists");
        assert!(mic_row.slider.is_none());
        assert_eq!(mic_row.message, "Checking...");

        let speaker_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Speaker)
            .expect("speaker row exists");
        assert!(speaker_row.slider.is_none());
        assert_eq!(speaker_row.message, "Failed");
    }

    #[test]
    fn other_rows_never_get_a_slider() {
        let layout = layout_of(&ready_statuses(), "marin");

        assert!(layout
            .rows
            .iter()
            .filter(|row| !matches!(
                row.icon_symbol,
                IconSymbol::Display | IconSymbol::Speaker | IconSymbol::Microphone
            ))
            .all(|row| row.slider.is_none()));
    }

    #[test]
    fn slider_area_stays_within_the_message_area() {
        let layout = layout_of(&ready_statuses(), "marin");

        for row in &layout.rows {
            if let Some(slider) = row.slider {
                assert!(slider.area.x >= row.message_area.x);
                assert!(slider.area.right() <= row.message_area.right());
                assert!(slider.area.y >= row.message_area.y);
                assert!(slider.area.bottom() <= row.message_area.bottom());
            }
        }
    }

    #[test]
    fn voice_combo_sits_beside_the_speaker_slider_without_overlap() {
        let layout = layout_of(&ready_statuses(), "marin");

        let speaker_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Speaker)
            .expect("speaker row exists");
        let slider = speaker_row.slider.expect("speaker is ready");
        let picker = layout.voice_picker.expect("realtime is ready");

        assert!(slider.area.right() <= picker.area.x);
        assert!(picker.area.right() <= speaker_row.message_area.right());
    }

    #[test]
    fn speaker_slider_uses_the_full_width_when_the_combo_is_absent() {
        let mut statuses = ready_statuses();
        statuses.realtime_session = ModuleStatus::Checking;
        let layout = layout_of(&statuses, "marin");

        let speaker_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Speaker)
            .expect("speaker row exists");
        let slider = speaker_row.slider.expect("speaker is ready");

        assert_eq!(slider.area.width, speaker_row.message_area.width);
        assert!(layout.voice_picker.is_none());
    }
}
