# 状態遷移

状態と、それを動かすきっかけの対応は `core/src/state.rs` に純粋関数として
置いている。画面の表情も音声の開始・停止も、すべてこの遷移から導く。

## 状態遷移図

```mermaid
stateDiagram-v2
  [*] --> Booting

  Booting --> SetupRequired : ConfigRejected<br/>／設定案内を表示
  Booting --> Connecting : ConfigLoaded<br/>／Wi-Fi接続

  Connecting --> Opening : NetworkReady<br/>／セッションを開く
  Opening --> Ready : SessionOpened

  Ready --> Listening : TalkPressed<br/>／録音開始
  Listening --> Thinking : SpeechEnded<br/>／録音停止・応答要求
  Listening --> Ready : SpeechNotDetected<br/>／何も送らず録音停止
  Thinking --> Speaking : ResponseStarted<br/>／再生開始
  Thinking --> Ready : ResponseFinished<br/>（音声なしで終了）
  Speaking --> Ready : ResponseFinished<br/>／再生停止

  Speaking --> Listening : TalkPressed<br/>／応答を打ち切り録音
  Thinking --> Listening : TalkPressed<br/>／応答を捨てて録音

  Connecting --> Recovering : Failed
  Opening --> Recovering : Failed
  Ready --> Recovering : Failed
  Listening --> Recovering : Failed
  Thinking --> Recovering : Failed
  Speaking --> Recovering : Failed

  Recovering --> Connecting : RetryRequested<br/>／再接続

  SetupRequired --> [*] : 親が設定を直して再起動
```

## 設計上の判断

**ボタンは押すだけで、押し続けなくてよい** — 録音の終わりはボタンの解放
ではなく、`core::turn_detector::TurnDetector` が実際の声と沈黙から決める。
声のあとに沈黙が1.4秒続けば `SpeechEnded`、声が一度も無いまま沈黙が続けば
`SpeechNotDetected` になり、どちらも `TalkReleased` は見ない
（[音声対話](../spec/conversation.md)参照）。

**応答中の割り込みを許す** — 子どもは相手の話し終わりを待たない。
`Speaking` や `Thinking` の最中にボタンを押されたら、生成中の応答を
打ち切って `Listening` に移る。これを許さないと会話が成立しない。

**失敗はどの状態からでも起こる** — `Failed` はどの状態から来ても
録音・再生の停止、セッションの切断、親への表示をまとめて行い
`Recovering` に移る。個々の状態ごとに書き分けると取りこぼしが出る。

**意味を持たないきっかけは捨てる** — たとえば `SetupRequired` での
ボタン操作は無視する。取りこぼしても害がないため、エラーにしない。

## 依頼する処理

遷移は次の処理を依頼する。実際に何をするかは実機側の担当。

| 依頼 | 内容 |
|---|---|
| `ConnectNetwork` / `OpenSession` / `CloseSession` | 接続の開始と終了 |
| `StartCapture` / `StopCapture` | マイクの取り込み |
| `RequestResponse` | 録音を確定して応答を求める |
| `CancelResponse` | 生成中の応答を打ち切る |
| `StartPlayback` / `StopPlayback` | 応答音声の再生 |
| `ShowSetupGuide` / `ShowFailure` | 親への表示。`main.rs` は `AppState` が `SetupRequired` / `Recovering` に入った瞬間を見て、[画面遷移](#画面遷移)の `ProblemDetected` を起こす |

## 画面遷移

どちらの画面（[設定画面](../spec/settings.md)／アシスタント画面）を
見せるかは `core/src/screen.rs` の `Screen` / `ScreenEvent` /
`transition_screen` が決める、`AppState` とは独立した小さな状態機械。
`AppState` は対話のフェーズだけを表し、画面の種類を混ぜると組み合わせが
爆発するため分けてある。

```mermaid
stateDiagram-v2
  [*] --> Settings

  Assistant --> Settings : SwipedToSettings（右→左）
  Settings --> Assistant : SwipedToAssistant（左→右）
  Assistant --> Settings : ProblemDetected<br/>（SetupRequired / Recovering に入った）
  Settings --> Assistant : AllModulesReady<br/>（監視対象の全モジュールが Ready）
```

`ProblemDetected` はスワイプでアシスタント画面に留まろうとしていても
上書きする。各モジュールの準備状況は `core/src/module_status.rs` の
`ModuleStatuses` が持ち、`core/src/settings_layout.rs` が設定画面の
配置に変換する。

`AllModulesReady` による自動復帰は、親が自分でスワイプして設定画面を
開いた場合には働かせない（`main.rs::auto_return_to_assistant`）。
詳細は [設定画面の設計](settings.md#全モジュール準備完了による自動復帰との競合)
を参照。
