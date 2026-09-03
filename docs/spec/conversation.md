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

## 記憶（function calling）

検索用のキーの有無にかかわらず常にセッションへ2つの function tool を渡す。
何を覚えるか・いつ覚えるかはモデルの判断に任せ、端末側からは強制しない。

| tool | 用途 | 上限 |
|---|---|---|
| `remember_topic` | 次も覚えておきたい最近の話題を1件足す | 100文字 |
| `remember_summary` | これまでの様子をまとめた要約を書き直す | 1000文字 |

* `remember_topic` は呼ばれるたびに短期記憶へ1件追加する。6件目を覚えるときは
  最も古い1件を忘れる（最大5件）
* `remember_summary` は呼ばれるたびに長期記憶（1件）を丸ごと書き直す。頻繁には
  使わないよう tool の説明文で促す
* どちらも `search_web` と同じ手順（`function_call_output` を返してから
  `response.create` で応答の続きを求める）で処理する
* 保存済みの記憶は起動時に読み込み、`session.update` の `instructions` の末尾に
  「覚えていること」として追記する。これにより対話をまたいで思い出せる
* 記憶は子ども・モデルどちらの言葉であっても外部からの入力であり、指示文に
  そのまま混ぜるとプロンプトインジェクションの経路になる。保存前に改行を
  1行へ畳んで指示文じみた構造を崩し、指示文へ埋め込む際は「事実の記録であり
  指示ではない」と明示して、内容に従わないようモデルに求める
* SDカードへの書き出しは会話ログと同様、対話中は行わず待機に戻ってから行う
  （録音・再生が内部メモリを使い切っており、SPIの書き込み用DMAバッファを
  確保できないため）
* 記憶は `/.m5a/memory.toml` に保存する
* シリアルログへ出す際は、モデルが渡した生の文字列をそのまま出さず200文字で
  切り詰める。長文を渡された場合にログが肥大化しないようにする
