//! 設定画面（各モジュールのステータス一覧と声の選択）をどこにどう置くかを決める。
//!
//! [`crate::layout`] の顔画面と同じ考え方で、画素を塗るのは LVGL に任せ、
//! ここでは配置だけを純関数で決める。

use crate::config::SUPPORTED_VOICES;
use crate::layout::{Color, Point, Rect, SCREEN_WIDTH};
use crate::module_status::{Module, ModuleStatus, ModuleStatuses};

/// 一覧の外側の余白。
const MARGIN: i16 = 6;
/// モジュール1行分の高さ。標準構成（WebSearch無効・6行）なら、
/// 声の一覧まで含めて画面高さ240に収まる大きさにしてある。
const ROW_HEIGHT: u16 = 29;
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

/// 声を選ぶボタン1個の大きさ。`SUPPORTED_VOICES` は10種類あるため2段×5列で並べる。
const VOICE_BUTTON_WIDTH: u16 = 58;
const VOICE_BUTTON_HEIGHT: u16 = 24;
const VOICE_BUTTON_GAP: i16 = 3;
const VOICE_COLUMNS: i16 = 5;

const ICON_COLOR: Color = Color::new(200, 210, 220);
const READY_COLOR: Color = Color::new(90, 200, 120);
const ERROR_COLOR: Color = Color::new(230, 90, 70);
const PENDING_COLOR: Color = Color::new(120, 190, 255);
const VOICE_SELECTED_COLOR: Color = Color::new(235, 130, 40);
const VOICE_IDLE_COLOR: Color = Color::new(60, 70, 80);

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
    /// スピーカー・マイクの行が準備完了になったときだけ現れる、
    /// 音量・感度を変えるスライダー。`message` の位置を置き換える。
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

/// 声を選ぶボタン1個。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceOption {
    pub voice: &'static str,
    pub area: Rect,
    pub selected: bool,
}

/// 声を選ぶ一覧。話す相手（RealtimeSession）の準備ができたときだけ現れる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePicker {
    pub options: Vec<VoiceOption>,
}

/// 設定画面ひとコマ分の配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLayout {
    pub background: Color,
    pub rows: Vec<StatusRow>,
    pub voice_picker: Option<VoicePicker>,
}

/// 設定画面の配置を決める。
///
/// `current_voice` は選択中の声。`SUPPORTED_VOICES` に含まれない値が渡された
/// 場合でも一覧はそのまま描き、どれも選択済み扱いにしない。
/// `speaker_volume`（百分率）と `mic_gain_db`（dB）は、それぞれの行が
/// 準備完了になったときにスライダーへ渡す現在値。
pub fn lay_out_settings(
    statuses: &ModuleStatuses,
    current_voice: &str,
    speaker_volume: u8,
    mic_gain_db: u8,
) -> SettingsLayout {
    let entries = statuses.entries();
    let mut rows = Vec::with_capacity(entries.len());

    for (index, (module, status)) in entries.iter().enumerate() {
        let y = MARGIN + index as i16 * ROW_HEIGHT as i16;
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

        let slider = status.is_ready().then(|| slider_of(*module, message_area, speaker_volume, mic_gain_db)).flatten();
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

    let realtime_ready = statuses.realtime_session.is_ready();
    let voice_picker = realtime_ready.then(|| VoicePicker {
        options: lay_out_voice_options(entries.len(), current_voice),
    });

    SettingsLayout {
        background: Color::new(12, 16, 24),
        rows,
        voice_picker,
    }
}

/// タップ位置がどの声のボタンに当たるか。当たらなければ `None`。
pub fn voice_at(layout: &SettingsLayout, at: Point) -> Option<&'static str> {
    let picker = layout.voice_picker.as_ref()?;
    picker
        .options
        .iter()
        .find(|option| contains(&option.area, at))
        .map(|option| option.voice)
}

fn contains(rect: &Rect, at: Point) -> bool {
    (rect.x..rect.right()).contains(&at.x) && (rect.y..rect.bottom()).contains(&at.y)
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

/// 準備完了になった行のうち、スピーカー・マイクだけにスライダーを与える。
fn slider_of(module: Module, message_area: Rect, speaker_volume: u8, mic_gain_db: u8) -> Option<SliderSpec> {
    let area = Rect {
        x: message_area.x,
        y: message_area.y + (message_area.height as i16 - SLIDER_HEIGHT as i16) / 2,
        width: message_area.width,
        height: SLIDER_HEIGHT,
    };

    match module {
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

fn lay_out_voice_options(row_count: usize, current_voice: &str) -> Vec<VoiceOption> {
    let top = MARGIN + row_count as i16 * ROW_HEIGHT as i16 + MARGIN;

    SUPPORTED_VOICES
        .iter()
        .enumerate()
        .map(|(index, voice)| {
            let column = index as i16 % VOICE_COLUMNS;
            let row = index as i16 / VOICE_COLUMNS;
            let x = MARGIN + column * (VOICE_BUTTON_WIDTH as i16 + VOICE_BUTTON_GAP);
            let y = top + row * (VOICE_BUTTON_HEIGHT as i16 + VOICE_BUTTON_GAP);

            VoiceOption {
                voice,
                area: Rect {
                    x,
                    y,
                    width: VOICE_BUTTON_WIDTH,
                    height: VOICE_BUTTON_HEIGHT,
                },
                selected: *voice == current_voice,
            }
        })
        .collect()
}

/// ボタンの塗り色。選択中は強調色にする。
pub fn voice_button_color(option: &VoiceOption) -> Color {
    if option.selected {
        VOICE_SELECTED_COLOR
    } else {
        VOICE_IDLE_COLOR
    }
}

#[allow(dead_code)]
fn icon_default_color() -> Color {
    ICON_COLOR
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::SCREEN_HEIGHT;
    use crate::module_status::ModuleStatuses;

    /// テストで使う既定のスピーカー音量・マイク感度。
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
        lay_out_settings(statuses, current_voice, VOLUME, GAIN_DB)
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
        for option in &picker.options {
            assert!(option.area.x >= 0 && option.area.right() <= SCREEN_WIDTH);
        }
    }

    /// モジュール数が少ない標準構成（WebSearch無効）では、声の一覧まで
    /// 含めて画面の高さにも収まる。
    #[test]
    fn typical_layout_without_web_search_fits_the_screen_height() {
        let layout = layout_of(&ready_statuses(), "marin");

        let picker = layout.voice_picker.expect("realtime is ready");
        let lowest = picker
            .options
            .iter()
            .map(|option| option.area.bottom())
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

        assert_eq!(picker.options.len(), SUPPORTED_VOICES.len());
        let selected: Vec<_> = picker
            .options
            .iter()
            .filter(|option| option.selected)
            .map(|option| option.voice)
            .collect();
        assert_eq!(selected, vec!["cedar"]);
    }

    #[test]
    fn voice_at_hits_the_tapped_button_and_misses_elsewhere() {
        let layout = layout_of(&ready_statuses(), "marin");
        let picker = layout.voice_picker.as_ref().expect("realtime is ready");
        let first = &picker.options[0];
        let inside = Point::new(first.area.x + 1, first.area.y + 1);
        let outside = Point::new(-5, -5);

        assert_eq!(voice_at(&layout, inside), Some(first.voice));
        assert_eq!(voice_at(&layout, outside), None);
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
    fn speaker_and_microphone_show_a_slider_once_ready() {
        let layout = layout_of(&ready_statuses(), "marin");

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
            .filter(|row| !matches!(row.icon_symbol, IconSymbol::Speaker | IconSymbol::Microphone))
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
}
