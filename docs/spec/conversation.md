# 音声対話

## 接続先

* モデル: `gpt-realtime-2.1-mini`
* 接続先: `wss://api.openai.com/v1/realtime?model=<モデル>`
* ヘッダ: `Authorization: Bearer <APIキー>` のみ
  * GA 版のため `OpenAI-Beta: realtime=v1` は**付けない**（Beta は廃止済み）

## 話す順番の決め方

サーバ側の発話区切り検出は使わず、端末側で声と沈黙から区切りを決める
（`core::turn_detector::TurnDetector`、[状態遷移](../design/state.md)参照）。
`session.audio.input.turn_detection` を `null` にして、次の順で自分から送る。

1. ボタン押下 → 録音開始。押し続ける必要はない
2. 実際に声が聞こえるまでは、録音してもサーバへは送らない
3. 声が聞こえたら、以後の録音を順に `input_audio_buffer.append`（base64）で送る
4. 声のあとの無音が1.4秒続いたら → `input_audio_buffer.commit` → `response.create`
5. 声が一度も聞こえないまま無音が1.4秒続いたら → 何も送らず録音を終える
6. 録音を始めてから10秒経っても無音が訪れなければ、そこで区切りとみなし、
   4または5と同じ扱いで録音を終える（背景に音楽など常に鳴っている音が
   あると、しきい値を超え続けて無音が一度も訪れないことがあるため）
7. 応答中にボタン押下 → `response.cancel` して録音に戻る

## 音声の形式

| 形式 | 設定値 | 標本化周波数 | 片方向の通信量 |
|---|---|---|---|
| G.711 μ-law | `ulaw` | 8 kHz | 約 85 kbps |
| PCM16 | `pcm16` | 24 kHz | 約 512 kbps |

既定を μ-law にしているのは、音声を base64 で JSON に載せる仕様のため
PCM では ESP32-S3 の無線と TLS で途切れやすいため。会話の用途では
電話並みの音質で足りる。

音声はすべて JSON のテキストフレームで送受信する。バイナリフレームは使わない。

## 受け取る出来事

GA 版で名前が変わっている点に注意する。

| 出来事 | 意味 |
|---|---|
| `session.created` / `session.updated` | セッションが使える |
| `response.output_audio.delta` | 応答音声の断片 |
| `response.output_audio_transcript.delta` | アシスタントの発話の文字起こし |
| `conversation.item.input_audio_transcription.completed` | 子どもの発話の文字起こし |
| `response.done` | 応答の終わり |
| `error` | サーバからの報告 |

知らない種類の出来事は無視する。`error` の多くは回復可能なので、
受け取ってもセッションは切らずに記録に留める。

### 音声の先読みと再生の終わり

サーバーは `response.output_audio.delta` をリアルタイムより速く送ってくる
ことがある。そのため `response.done` が届いた時点でも、端末側の再生が
まだ追いついていないことがある。「応答が終わった」という状態遷移
（`AppEvent::ResponseFinished`、[状態遷移](../design/state.md)参照）は
`response.done` の到着ではなく、**実際に鳴らし終わったとき**に起こす。
先に状態を進めてしまうと、まだ声が鳴っているのに表情や口の動きだけ
先に待機中へ戻ってしまう。

## web検索（function calling）

検索用の APIキーが設定されているときだけ、セッションに検索の function tool
（`search_web`）を渡す。呼び出しは次の順で進める。

1. `session.update` の `session.tools` に `search_web` を含めて渡す
2. モデルが調べたいと判断すると、`response.done` の
   `response.output` に `type: "function_call"` の項目が載って届く
   （`call_id` / `name` / `arguments` を持つ）
3. `arguments` の `query` で Tavily に問い合わせ、結果の要約を
   `conversation.item.create`（`item.type: "function_call_output"`、
   同じ `call_id`）で返す
4. 続けて `response.create` を送り、応答を再開させる

検索が使えない・失敗した・結果が得られなかったときも、必ず何らかの
`function_call_output` を返す。モデルを待たせたままにせず、
ガードレールの指示文どおり「わからない」と答えさせる。
