//! 各モジュール（ハードウェア・接続先）の準備状況。
//!
//! 設定画面に縦一覧で並べる対象。個々のモジュールは初期化や接続の成否を
//! [`Result`] で返すだけなので、ここで「未確認／確認中／使える／使えない」
//! という共通の語彙に丸めて画面へ渡す。

/// 一覧表示の対象となるモジュール。この並び順のまま縦に並べる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Display,
    SdCard,
    Microphone,
    Wifi,
    RealtimeSession,
    WebSearch,
}

/// モジュールひとつの現在状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleStatus {
    /// まだ確認していない。
    NotChecked,
    /// 確認・接続を試みている。
    Checking,
    /// 使える。
    Ready,
    /// 使えない。詳しい理由はシリアルログにだけ残し、画面には
    /// 「Failed」とだけ出す（子ども向け画面に長い英文は不要なため）。
    Error,
}

impl ModuleStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// 全モジュールの状態一式。設定画面の入力になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleStatuses {
    /// 描画できている時点で動いているとみなせるため、常に `Ready` 固定。
    pub display: ModuleStatus,
    pub sd_card: ModuleStatus,
    pub microphone: ModuleStatus,
    pub wifi: ModuleStatus,
    pub realtime_session: ModuleStatus,
    /// config.toml に検索用APIキーが無ければ一覧にも出さないため `None`。
    pub web_search: Option<ModuleStatus>,
}

impl ModuleStatuses {
    /// 起動直後、まだ何も確認していない状態。
    ///
    /// `web_search` は検索が有効かどうかが設定を読むまで分からないため、
    /// 呼び出し側が設定を読んだ後に [`Self::web_search`] 相当のフィールドを
    /// 更新する想定で `None` から始める。
    pub fn booting() -> Self {
        Self {
            display: ModuleStatus::Ready,
            sd_card: ModuleStatus::NotChecked,
            microphone: ModuleStatus::NotChecked,
            wifi: ModuleStatus::NotChecked,
            realtime_session: ModuleStatus::NotChecked,
            web_search: None,
        }
    }

    /// 画面に並べる順で `(モジュール, 状態)` を返す。
    /// `web_search` が `None`（検索を使わない設定）なら一覧から外れる。
    pub fn entries(&self) -> Vec<(Module, &ModuleStatus)> {
        let mut entries = vec![
            (Module::Display, &self.display),
            (Module::SdCard, &self.sd_card),
            (Module::Microphone, &self.microphone),
            (Module::Wifi, &self.wifi),
            (Module::RealtimeSession, &self.realtime_session),
        ];
        if let Some(web_search) = &self.web_search {
            entries.push((Module::WebSearch, web_search));
        }
        entries
    }

    /// 一覧表示の対象がすべて `Ready` か。
    pub fn all_ready(&self) -> bool {
        self.entries().iter().all(|(_, status)| status.is_ready())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ModuleStatus {
        ModuleStatus::Ready
    }

    fn error() -> ModuleStatus {
        ModuleStatus::Error
    }

    #[test]
    fn booting_has_nothing_checked_except_display() {
        let statuses = ModuleStatuses::booting();

        assert_eq!(statuses.display, ModuleStatus::Ready);
        assert_eq!(statuses.sd_card, ModuleStatus::NotChecked);
        assert!(statuses.web_search.is_none());
        assert!(!statuses.all_ready());
    }

    #[test]
    fn web_search_is_excluded_from_entries_when_absent() {
        let mut statuses = ModuleStatuses::booting();
        statuses.sd_card = ready();
        statuses.microphone = ready();
        statuses.wifi = ready();
        statuses.realtime_session = ready();

        assert!(statuses
            .entries()
            .iter()
            .all(|(module, _)| *module != Module::WebSearch));
        assert!(statuses.all_ready());
    }

    #[test]
    fn web_search_must_also_be_ready_when_present() {
        let mut statuses = ModuleStatuses::booting();
        statuses.sd_card = ready();
        statuses.microphone = ready();
        statuses.wifi = ready();
        statuses.realtime_session = ready();
        statuses.web_search = Some(ModuleStatus::Checking);

        assert!(!statuses.all_ready());

        statuses.web_search = Some(ready());
        assert!(statuses.all_ready());
    }

    #[test]
    fn a_single_error_prevents_all_ready() {
        let mut statuses = ModuleStatuses::booting();
        statuses.sd_card = ready();
        statuses.microphone = ready();
        statuses.wifi = error();
        statuses.realtime_session = ready();

        assert!(!statuses.all_ready());
    }
}
