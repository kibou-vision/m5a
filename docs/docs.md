# 文書の索引

m5a は M5Stack CoreS3 上で動く、子供用の音声チャットアシスタントです。

## 目的から探す

| やりたいこと | 読む文書 |
|---|---|
| 何を作っているのか知りたい | [仕様](spec.md) |
| 使う人（親）としてセットアップしたい | [初回セットアップ](spec/setup.md) |
| 開発環境を用意したい | [開発環境](development.md) |
| 全体の構造を把握したい | [設計](design.md) |
| コードのどこを直すか探したい | [構成と責務](design/architecture.md) |
| 画面や表情の挙動を知りたい | [顔と画面](spec/face.md) / [状態遷移](design/state.md) |
| 設定画面・モジュールのステータス表示を知りたい | [設定画面](spec/settings.md) / [設定画面の設計](design/settings.md) |
| 対話の流れを追いたい | [音声対話](spec/conversation.md) / [対話の流れ](design/conversation.md) |
| 安全対策の考え方を知りたい | [ガードレール](spec/guardrail.md) |
| 実機まわりでつまずいた | [CoreS3 の制約](design/hardware.md) |
| コードを書く | [コーディング規約](coding.md) / [テスト方針](testing.md) |

## 文書の構成

* [spec.md](spec.md) — 何を作るか。詳細は `spec/` 以下に分ける
* [design.md](design.md) — どう作るか。詳細は `design/` 以下に分ける
* [development.md](development.md) — 開発環境の用意と実機への書き込み
* [coding.md](coding.md) — コーディング規約
* [testing.md](testing.md) — テスト方針と実行方法
