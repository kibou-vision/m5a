# 構成と責務

## 配置図

```mermaid
graph LR
  Child((こども))
  Parent((親))

  subgraph Device["M5Stack CoreS3"]
    FW["m5a<br/>(実機に触れる層)"]
    CORE["m5a-core<br/>(ロジック層)"]
    FW -->|トレイト実装を渡す| CORE
  end

  SD[("microSD<br>/.m5a/config.toml<br>/.m5a/logs/")]
  Router["Wi-Fi ルータ"]
  API["OpenAI Realtime API<br>gpt-realtime-2.1-mini"]
  PC["親のPC"]

  Child -->|話しかける / ボタン| Device
  Device -->|顔と音声| Child
  Device <--> SD
  Device <--> Router
  Router <-->|WebSocket over TLS| API
  Parent --> PC
  PC -->|設定を編集<br>ログを読む| SD
```

## クラス図

```mermaid
classDiagram
  direction LR

  class Storage {
    <<trait>>
    +exists(path) bool
    +read_text(path) Result~String~
    +write_text(path, contents) Result
    +append_text(path, contents) Result
    +create_dir(path) Result
  }

  class Config {
    +child: ChildConfig
    +wifi: WifiConfig
    +openai: OpenAiConfig
    +validate() Result
  }

  class ConfigError {
    <<enum>>
    TemplateCreated
    Unwritten
    Malformed
    Unreadable
    +describe() String
    +remedy() String
  }

  class AppState {
    <<enum>>
    Booting / SetupRequired
    Connecting / Opening
    Ready / Listening
    Thinking / Speaking
    Recovering
  }

  class Guardrail {
    -child_name: String
    -child_age: u8
    +build_instructions() String
    +inspect(text) Verdict
    +safe_reply(concern) String
  }

  class SessionSetup {
    +model: String
    +voice: String
    +audio_format: AudioFormat
    +instructions: String
  }

  class ServerEvent {
    <<enum>>
    SessionReady
    AudioDelta
    AssistantSaid / ChildSaid
    ResponseFinished
    Reported / Ignored
  }

  class FaceAnimator {
    +set_expression(e)
    +set_voice_level(level)
    +frame_at(now_ms) FaceFrame
  }

  class FaceFrame {
    +expression: Expression
    +eye_openness: u8
    +mouth_openness: u8
    +gaze_x: i8
  }

  class LogEntry {
    +at_unix: i64
    +speaker: Speaker
    +text: String
    +path() String
    +format() String
  }

  class SdStorage {
    実機のSDカード
  }

  Storage <|.. SdStorage : 実装
  Config ..> Storage : 読み書き
  Config --> ConfigError : 失敗
  Config --> SessionSetup : モデル・声・形式
  Guardrail --> SessionSetup : 指示文
  AppState --> FaceAnimator : 表情を決める
  FaceAnimator --> FaceFrame : 生成
  ServerEvent --> LogEntry : 文字起こしを記録
  LogEntry ..> Storage : 追記
```

## 責務の割り当て

### `m5a-core`

| モジュール | 責務 |
|---|---|
| `ports` | 外界に触れるためのトレイトと、試験用のメモリ実装 |
| `config` | TOML 設定の雛形生成・解析・検証 |
| `state` | 状態遷移。状態ときっかけから次の状態と依頼する処理を決める |
| `face` | 時刻から表情のかたちを決める |
| `render` | 表情のかたちを図形として描く |
| `guardrail` | 指示文の組み立てと、文字起こしの検査 |
| `realtime` | Realtime API の電文の組み立てと解析 |
| `audio` | μ-law 変換、標本化周波数の変換、音量の算出 |
| `logbook` | 会話ログの整形と追記 |

### `m5a`

| モジュール | 責務 |
|---|---|
| `hal::pins` | CoreS3 の配線の記録 |
| `hal::power` | AXP2101・AW9523 による給電とバックライト |
| `hal::display` | LCD の初期化 |
| `hal::touch` | FT6336 から押下・解放を読む |
| `hal::storage` | SD カードのマウントと `Storage` の実装 |
| `main` | 起動順序の制御と、状態遷移にもとづく処理の実行 |
