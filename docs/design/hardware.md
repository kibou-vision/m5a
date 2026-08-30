# CoreS3 の制約

実機で踏みやすい落とし穴と、その回避方法を残す。

## GPIO35 を LCD と microSD が分け合う

LCD の DC（データ／コマンド切り替え）と microSD の MISO が同じ GPIO35 に
繋がっている。素直に書くと両方を同時には使えない。

BSP（`espressif/m5stack_core_s3`）がこの取り合いを処理するため、本実装では
**SD カードを挿したまま画面を使える**。自前で SPI を組む場合は、設定を
読んでからカードを外して画面を初期化する、という順序制御が必要になる。

## 電源を入れないと何も動かない

AXP2101（0x34）と AW9523（0x58）を初期化しないと、LCD もマイクも SD も
電源が来ない。初期化を飛ばすと**すべての処理が成功ログを出しながら
画面だけ真っ暗**という分かりにくい失敗になる。

本実装では `bsp_i2c_init()` と `bsp_display_start()` が引き受ける。
以下は BSP に任せずに書く場合の参考として残す。

| レール | 供給先 | 電圧 |
|---|---|---|
| ALDO1 | スピーカーアンプ AW88298 | 1.8V |
| ALDO2 | マイク ADC ES7210 | 3.3V |
| ALDO3 | カメラ | 3.3V |
| ALDO4 | microSD | 3.3V |
| DLDO1 | LCD バックライト | 2.5〜3.3V（明るさ） |

バックライトは GPIO ではなく AXP2101 の DLDO1。レジスタ 0x99 が電圧＝明るさ、
0x90 の bit7 が有効化。電圧を書いても有効化しないと点かない。

LCD のリセットは GPIO 直結ではなく AW9523 の P1_1。電源投入時に解除するため、
画面の初期化ではリセット端子を扱わない。

## 電源を落とすのは最終的に AXP2101 自身にやらせる

自動シャットダウン（[状態遷移](state.md)の `Idle` → `ShuttingDown`）は
最初 ESP-IDF 標準の `esp_deep_sleep_start()` だけで済ませようとしたが、
実機で「電源を切ったはずが再起動する」症状が繰り返し再現し、原因の
特定に何段階も踏んだ。

**問題1: `esp_deep_sleep_start()` は ESP32-S3 のチップしか止めない。**
LCD バックライトは AXP2101 の DLDO1 から給電されており、CPU が眠っても
そのレールは触れられないままなので、**バックライトが点いたまま**になる。
実機で確かめて気づいた。対策として deep sleep を呼ぶ前に
`bsp_display_backlight_off()` でバックライトを消すようにした
（BSP の標準 API）。

**問題2: `bsp_display_enter_sleep()` が呼ぶたびに必ずパニックで
再起動していた。** パネルもろとも休止させようとこの BSP 標準 API を
追加したところ、実機のパニックログに次が出た。

```
E (183592) TP: Sleep mode not supported!
ESP_ERROR_CHECK failed: esp_err_t 0xffffffff (ESP_FAIL) at ...
  bsp_display_enter_sleep
    expression: bsp_touch_enter_sleep()
abort() was called
```

この機種のタッチドライバは実装上スリープに対応しておらず、
`bsp_touch_enter_sleep()` が常に `ESP_FAIL` を返す。ところが
`bsp_display_enter_sleep()` はこの戻り値を `ESP_ERROR_CHECK` で無条件に
検査しているため、呼ぶたびに確実に `abort()` してリブートしていた。
ログを取らずに調べていた間は「AXP2101のVBUS誤検知」など的外れな仮説を
検討したが、いずれも見当違いだった。**タッチドライバがスリープ非対応の
機種では `bsp_display_enter_sleep()` を呼んではいけない**、というのが
教訓。パネルの休止は諦め、`bsp_display_backlight_off()` によるバックライト
消灯だけで画面を暗くしている。

**問題3: 続いて `bsp_sdcard_unmount()` を呼んだところ、これも必ず
パニックで再起動した。** 電源を切る前に念のためSDカードをアンマウント
しようとしたところ、実機のパニックログに次が出た。

```
E (183562) spi_master: spi_master_deinit_driver(374): not all CSses freed
ESP_ERROR_CHECK failed: esp_err_t 0x103 (ESP_ERR_INVALID_STATE) at ...
  bsp_spi_deinit
    expression: spi_bus_free((SPI3_HOST))
abort() was called
```

[GPIO35 を LCD と microSD が分け合う](#gpio35-を-lcd-と-microsd-が分け合う)
の通り、LCDパネルとmicroSDは同じ SPI バス（SPI3_HOST）に相乗りしている。
`bsp_sdcard_unmount()` はSDカード用のデバイスを外したあとバスごと
`bsp_spi_deinit()` で解放しようとするが、LCDパネルのSPIデバイスが
まだそのバスに載ったままだと `spi_bus_free()` が
「CSが残っている」で失敗する。これも `ESP_ERROR_CHECK` で無条件に
`abort()` する実装のため、画面を使い続けたまま電源を切ろうとする限り
必ず再現する。**LCDパネルを使用中は `bsp_sdcard_unmount()` を呼んでは
いけない。**

対策として `power_off()` からこの呼び出しを削除した。もともと
アンマウントは「念のため」の二重の備えで、本丸は
`hal::storage::SdStorage` が書き込むたびに `File::sync_all()` を呼び、
`power_off()` を経由するかどうかに関わらずその場でディスクへ確実に
落とす仕組みの方だった（自動シャットダウンは無操作から3分待つため、
実際には電池を抜く・電源ボタンの長押しなど給電を直接断たれることの
方が多く、`power_off()` を経由しない経路にはどのみち備える必要が
あった）。そのため二重の備えを諦めても実害はない。

問題1〜3を潰した結果、実機で deep sleep へ確実に入れる（画面が消え、
再起動しない）ことを確認できた。

**そこで改めて AXP2101 自身にシャットダウンさせる方式に切り替えた。**
`esp_deep_sleep_start()` は ESP32-S3 のチップしか止めず、VRTC以外の
レール（ESP32-S3 本体の3.3V系も含む）は給電されたままなので、本当の
意味での電源断ではない。AXP2101 の「共通設定」レジスタ（0x10）の bit0
に1を立てると、VRTC を除くすべてのレールが切れる。実機の M5Stack
製品群で使われている
[`lewisxhe/XPowersLib`](https://github.com/lewisxhe/XPowersLib) の
`shutdown()` 実装と同じレジスタ・ビットを踏んでいる。BSP は AXP2101 用の
取っ手を `bsp_feature_en.c` の中に静的に持つが外へは公開していないため、
`bsp_i2c_get_handle()` で共有 I2C バスを取り、同じアドレス（0x34）へ
自分の取っ手を新たに `i2c_master_bus_add_device()` で作って使う
（同じアドレスに複数の取っ手を持つこと自体は `esp_driver_i2c` 側で
禁止されていない）。他のビットを壊さないよう、読んでから bit0 だけ
立てて書き戻す。AXP2101 と通信できなかった場合だけ、保険として
`esp_deep_sleep_start()` にも落とす。

念のため、ここに至るまでの調査で「AXP2101のVBUS誤検知（Baseモジュールの
5VがVBUSと区別が付かない）」という仮説も検討したが、実際には無関係
だった。問題2・問題3のパニックを潰す前は、この AXP2101 直接シャットダウン
を試しても手前の `bsp_display_enter_sleep()` で毎回パニックしており、
「電源を切ろうとすると再起動する」という同じ症状に見えたため誤診断の
原因になった。**似た症状でも実機のパニックログを取らずに憶測で原因を
決め打ちしない**、というのがこの一連の調査の教訓。

起床要因は deep sleep へ落ちる保険の経路も含めてあえて設定しない。
設定すると「切ったつもりが何かの拍子に動き出す」という分かりにくい
挙動になるため、目覚めは実機の電源ボタンによる起動に任せる。

## I2C は 100kHz

FT6336 タッチパネルが 400kHz では応答しない。内部バス全体を 100kHz で動かす。

## 公式ドキュメントの I2S 記載が誤っている

M5Stack 公式ドキュメントは BCLK/LRCK と DIN/DOUT を入れ替えて記載している。
Espressif の BSP と M5Unified が一致する側を採る。

* MCLK = GPIO0、BCLK = GPIO34、WS = GPIO33、DIN = GPIO14、DOUT = GPIO13

## PSRAM は Quad

CoreS3 の 8MB PSRAM は Quad 接続。Octal は GPIO33-37 を占有してしまい、
I2S と LCD/SD の配線と両立しない。

`CONFIG_SPIRAM_TRY_ALLOCATE_WIFI_LWIP=y` が無いと、Wi-Fi と lwIP が
バッファを確保できず再起動を繰り返す。

**`CONFIG_MBEDTLS_EXTERNAL_MEM_ALLOC=y` にしてはいけない。** ESP32-S3 の
AES/SHA アクセラレータは DMA 駆動で、DMA は PSRAM に届かない。mbedTLS の
バッファを PSRAM に移すとハードウェア暗号が壊れ、Wi-Fi ごと落ちる。

## 音を出すには給電が要る

コーデックを開いて書き込みが成功を返しても、それだけでは鳴らない。
`bsp_feature_enable(BSP_FEATURE_SPEAKER, true)` がアンプの有効化と
1.8V の供給を行う。これを飛ばすと **`esp_codec_dev_write` は成功を返し続け、
音だけが出ない**。ログからは正常に見えるため気付きにくい。

マイクも同様に `BSP_FEATURE_MIC` で給電する。

## 内部メモリの取り合い

PSRAM が 8MB あっても、**内部 DRAM の連続した空き**が足りずに失敗することがある。
実測では画面を用意した時点で最大連続ブロックが 31KB まで落ち、
WebSocket の受信タスク（8KB）を作れずに `Error create websocket task` となった。

LVGL の描画バッファが既定で 320×100×2 = 64KB を2枚使うのが主因。
`CONFIG_BSP_LCD_DRAW_BUF_HEIGHT=40` に下げて確保できるようにした。
部分更新なので見た目は変わらない。

音声を足すとさらに逼迫するため、困ったら `board::report_memory` で
「空き」ではなく**最大連続ブロック**を見ること。

## 接続の順序

`EspWebSocketClient::new` は繋ぎ終える前に戻る。直後に `session.update` を
送ると「まだ繋がっていない」と拒まれる。サーバは接続時に `session.created` を
寄越すので、それを合図に設定を送り、`session.updated` が返ってから
話しかけられる状態にする。

## 音を鳴らすまでに踏んだこと

いずれも「書き込みは成功を返すのに音だけ出ない」という形で現れ、
コードを読んでも気付けなかった。実機で一つずつ潰した記録を残す。

1. **音量と感度が未設定** — `esp_codec_dev_set_out_vol` と
   `esp_codec_dev_set_in_gain` を呼ばないと、鳴らないし拾えない。
   さらに AW88298 のドライバは要求音量から `pa_gain` を**差し引く**ため、
   BSP が決め打ちしている 15dB のぶん小さくなる。`pa_gain` は外部アンプの
   増幅分を申告する値で、上げると逆に小さくなる点に注意。本実装では
   音量設定のあとにレジスタ 0x0C を書き戻して目減りを打ち消している
2. **設定時にクロックが止まっていた** — AW88298 は設定するときに BCK が
   流れていないと PLL がロックせず、以後どれだけ書き込んでも無音になる。
   **先にマイクを開いて読み始め、クロックが出ている状態でスピーカーを設定する**
3. **黙っている間にクロックが止まる** — 鳴らす音が無いときも無音を流し続ける
4. **受信バッファが小さすぎた** — 音声の断片は 4KB を超える。
   `EspWebSocketClient` はこれを超えるフレームを組み立て直せず
   （esp-idf-svc の既知の問題）、**切れた JSON が届いて音声だけが失われる**。
   文字起こしは小さく届くため、会話が成立しているように見えるのが厄介
5. **SD 書き込みで DMA バッファを確保できず落ちた** — 描画バッファを1枚に
   減らして内部メモリを空け、書く前に空きを確かめる
6. **応答が長いほど再生が途切れ、最後は途中で終わる** —
   サーバーは音声をリアルタイムより速く送ってくるため、再生の待ち行列が
   浅い（数百 ms 分）と一瞬で溢れて音を捨て続ける。ヒープ自体は
   実測で常に 8MB 以上空いており、メモリ不足ではなかった。待ち行列を
   PSRAM に余裕がある分だけ深く（15秒分）した。ただし深くした分だけ
   割り込み時に「もう聞かない古い音声」が溜まりうるので、
   `AppAction::CancelResponse` のときだけ待ち行列を空にする仕組み
   （`Audio::interrupt`）を別に設けた
7. **口パクが発話よりずっと早く終わる** — 6.の対策で音声自体は最後まで
   鳴るようになったが、`response.done`（応答終了の知らせ）は
   まだ再生の待ち行列が残っている段階で届く。これをそのまま
   状態遷移の `ResponseFinished` に使うと、鳴っている途中で
   `Speaking` を抜けて口だけ先に閉じる。`Audio::is_speaking()` で
   実際に鳴らし終わったかを確かめ、鳴らし終わってから
   `ResponseFinished` を起こすようにした（[音声対話](../spec/conversation.md)参照）
8. **録音の待ち行列を空にし続けないとタスクウォッチドッグに落ちる** —
   声を検出するまで録音をサーバーへ送らない仕組み
   （`core::turn_detector::TurnDetector`）を入れた際、待ち行列
   （`sync_channel`、深さ24）を「送るときだけ」空にしていたところ、
   声を待つ数秒の間ずっと満杯のままになり、録音の仕事側の
   `try_send` がタスクウォッチドッグの時間内に空きを得られず
   `task_wdt_timeout_handling` で落ちた。声の有無にかかわらず
   待ち行列は毎回空にし、サーバーへ送るかどうかだけを声の検出で
   分けるようにした

9. **web検索のスレッドを作れない** — OpenAI との WebSocket 接続を張ったまま
   web検索用に別の TLS 接続を張ろうとすると、内部 DRAM の連続空きが
   8.7KB ほどまで落ち、検索スレッド自体を作れず
   `pthread: Failed to create task!`（`Not enough space`）になった。
   Bluetooth はもともと無効（`CONFIG_BT_ENABLED` 未設定）で削れる余地が
   無かったため、代わりに WiFi の受信・送信バッファ数（既定16〜32個）を
   半分に減らした。この端末は音声（μ-law 約85kbps）と検索の小さい
   HTTPリクエストしか使わず高いスループットは要らないため、trade-off は
   小さい。実機で連続空きが 8.7KB → 31.7KB に増え、検索が動くようになった

マイクとスピーカーは BCLK/WS を共有するため標本化周波数を揃える必要がある。
本実装では両方 16kHz で開き、通信は 8kHz の μ-law に変換している。

## 実機で確認できたこと（2026-08-27）

第1段階の実機確認で、次が動作することを確かめた。

* PSRAM 8MB を Quad 80MHz で認識、CPU 240MHz
* AXP2101・AW9523 による給電（エラーなし）
* microSD のマウントと設定ファイルの雛形生成
  * `CONFIG_FATFS_LFN_HEAP=y` が必要。既定の 8.3 形式では `/.m5a` も
    `config.toml` も不正な名前として `EINVAL` になり作成に失敗する
* LCD の初期化と描画、タッチの読み取り
* タッチは BSP の `esp_lcd_touch_ft5x06` 経由で座標が取れる。
  CHSC6540 は FT5x06 互換のため、どちらの部品でもこの経路で読める

## 旧 I2C ドライバとの衝突

BSP は新しい I2C ドライバ（`driver/i2c_master.h`）を使う。一方 `esp-idf-hal`
は使っていなくても `I2cDriver` の後始末関数を参照するため、旧ドライバが
リンクされてしまう。旧ドライバは起動時に「新ドライバも居る」と検知して
`abort()` するので、そのままでは起動を繰り返す。

```
E i2c: CONFLICT! driver_ng is not allowed to be used with this old driver
```

実行時に触るのは新ドライバだけなので、`CONFIG_I2C_SKIP_LEGACY_CONFLICT_CHECK=y`
で検査を外している。

## 画面まわりで解決した3点

LVGL と BSP に移行して次が解消した。自前で SPI に描いていたときの記録として残す。

1. **色の赤青反転** — CoreS3 のパネルは BGR 並びで、赤が青として出ていた。
   BSP の `esp_lcd_ili9341` が正しい色順を設定するため不要になった
2. **困り眉の向き** — 外側が高い「怒り」の向きになっていた。
   `core/src/layout.rs` で「内側が上がるハの字」に直し、試験で固定した
3. **まばたきが点滅に見える** — 表情が変わるたびに全画面を消していたため。
   LVGL が変わった部分だけを描くようになり解消した
