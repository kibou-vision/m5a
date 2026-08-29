//! 設定画面（各モジュールのステータス一覧と声の選択）をどこにどう置くかを決める。
//!
//! [`crate::layout`] の顔画面と同じ考え方で、画素を塗るのは LVGL に任せ、
//! ここでは配置だけを純関数で決める。

use crate::config::SUPPORTED_VOICES;
use crate::layout::{Color, Point, Rect, SCREEN_WIDTH};
use crate::module_status::{Module, ModuleStatus, ModuleStatuses};

/// 一覧の外側の余白。
const MARGIN: i16 = 6;
/// モジュール1行分の高さ。標準構成（WebSearch無効・最大5行）なら、
/// 声の一覧まで含めて画面高さ240に収まる大きさにしてある。
const ROW_HEIGHT: u16 = 28;
/// アイコンの正方形の一辺。
const ICON_SIZE: u16 = 20;
/// アイコンと文字の間隔。
const ICON_GAP: i16 = 6;
/// バッジ（読込中／OK／警告）の一辺。
const BADGE_SIZE: u16 = 12;

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
            ModuleStatus::Error { .. } => Self::Warning,
        }
    }
}

/// モジュール1行分の配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub icon: Rect,
    pub icon_symbol: IconSymbol,
    pub label_area: Rect,
    pub message_area: Rect,
    pub badge: Rect,
    pub badge_symbol: BadgeSymbol,
    pub label: &'static str,
    /// 準備できていれば空文字。エラーなら「何が起きたか＋どう直すか」。
    pub message: String,
    pub color: Color,
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
pub fn lay_out_settings(statuses: &ModuleStatuses, current_voice: &str) -> SettingsLayout {
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
        let label_area = Rect {
            x: text_x,
            y,
            width: text_width,
            height: ROW_HEIGHT / 2,
        };
        let message_area = Rect {
            x: text_x,
            y: y + ROW_HEIGHT as i16 / 2,
            width: text_width,
            height: ROW_HEIGHT / 2,
        };
        let badge = Rect {
            x: SCREEN_WIDTH - MARGIN - BADGE_SIZE as i16,
            y: y + (ROW_HEIGHT as i16 - BADGE_SIZE as i16) / 2,
            width: BADGE_SIZE,
            height: BADGE_SIZE,
        };

        rows.push(StatusRow {
            icon,
            icon_symbol: IconSymbol::of(*module),
            label_area,
            message_area,
            badge,
            badge_symbol: BadgeSymbol::of(status),
            label: module.label(),
            message: message_of(status),
            color: color_of(status),
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

fn message_of(status: &ModuleStatus) -> String {
    match status {
        ModuleStatus::NotChecked => String::new(),
        ModuleStatus::Checking => "たしかめています".to_string(),
        ModuleStatus::Ready => "つかえます".to_string(),
        ModuleStatus::Error { describe, remedy } => format!("{describe}\n{remedy}"),
    }
}

fn color_of(status: &ModuleStatus) -> Color {
    match status {
        ModuleStatus::NotChecked | ModuleStatus::Checking => PENDING_COLOR,
        ModuleStatus::Ready => READY_COLOR,
        ModuleStatus::Error { .. } => ERROR_COLOR,
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

    fn ready_statuses() -> ModuleStatuses {
        let mut statuses = ModuleStatuses::booting();
        statuses.sd_card = ModuleStatus::Ready;
        statuses.microphone = ModuleStatus::Ready;
        statuses.wifi = ModuleStatus::Ready;
        statuses.realtime_session = ModuleStatus::Ready;
        statuses
    }

    /// モジュールの数が多い（WebSearchを含む）場合、縦方向は画面高さを
    /// 超えることがある。設定画面はスクロールを許すため、横方向だけを
    /// 画面内に収める規則として検証する。
    #[test]
    fn everything_stays_within_the_screen_width() {
        let mut statuses = ready_statuses();
        statuses.web_search = Some(ModuleStatus::Error {
            describe: "検索できません".to_string(),
            remedy: "APIキーを確かめてください".to_string(),
        });
        let layout = lay_out_settings(&statuses, "marin");

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
        let layout = lay_out_settings(&ready_statuses(), "marin");

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
        let without_search = lay_out_settings(&ready_statuses(), "marin");
        assert_eq!(without_search.rows.len(), 5);

        let mut with_search = ready_statuses();
        with_search.web_search = Some(ModuleStatus::Ready);
        let with_search = lay_out_settings(&with_search, "marin");
        assert_eq!(with_search.rows.len(), 6);
    }

    #[test]
    fn voice_picker_appears_only_when_realtime_session_is_ready() {
        let mut statuses = ready_statuses();
        statuses.realtime_session = ModuleStatus::Checking;
        let layout = lay_out_settings(&statuses, "marin");

        assert!(layout.voice_picker.is_none());
    }

    #[test]
    fn voice_picker_lists_every_supported_voice_and_marks_the_current_one() {
        let layout = lay_out_settings(&ready_statuses(), "cedar");
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
        let layout = lay_out_settings(&ready_statuses(), "marin");
        let picker = layout.voice_picker.as_ref().expect("realtime is ready");
        let first = &picker.options[0];
        let inside = Point::new(first.area.x + 1, first.area.y + 1);
        let outside = Point::new(-5, -5);

        assert_eq!(voice_at(&layout, inside), Some(first.voice));
        assert_eq!(voice_at(&layout, outside), None);
    }

    #[test]
    fn error_message_combines_describe_and_remedy() {
        let mut statuses = ready_statuses();
        statuses.wifi = ModuleStatus::Error {
            describe: "WiFiに繋がりません".to_string(),
            remedy: "SSIDを確かめてください".to_string(),
        };
        let layout = lay_out_settings(&statuses, "marin");

        let wifi_row = layout
            .rows
            .iter()
            .find(|row| row.icon_symbol == IconSymbol::Wifi)
            .expect("wifi row exists");
        assert!(wifi_row.message.contains("WiFiに繋がりません"));
        assert!(wifi_row.message.contains("SSIDを確かめてください"));
    }
}
