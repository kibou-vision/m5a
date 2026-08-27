# 音声対話

## 接続先

* モデル: `gpt-realtime-2.1-mini`
* 接続先: `wss://api.openai.com/v1/realtime?model=<モデル>`
* ヘッダ: `Authorization: Bearer <APIキー>` のみ
  * GA 版のため `OpenAI-Beta: realtime=v1` は**付けない**（Beta は廃止済み）

## 話す順番の決め方

押している間だけ録音する方式のため、サーバ側の発話区切り検出は使わない。
`session.audio.input.turn_detection` を `null` にして、次の順で自分から送る。

1. ボタン押下 → 録音開始
2. 録音中 → `input_audio_buffer.append`（base64）
3. ボタン解放 → `input_audio_buffer.commit` → `response.create`
4. 応答中にボタン押下 → `response.cancel` して録音に戻る

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
