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

## 第2段階で確かめること

音声を結線する際に確認が必要な点。

* AW88298 は 44.1kHz だと PLL がロックせず無音になる。48kHz を使う
* 新しい I2S ドライバは送信バッファが空だとクロックを止める。
  AW88298 を初期化する前に無音を流してクロックを出す
* ES7210 は MCLK が出るまで I2C に応答しない。
  I2S 設定 → 受信開始 → 待つ → AW88298 → ES7210 の順で初期化する
* マイクとスピーカーは BCLK/WS を共有するため、標本化周波数を揃える必要がある
* `EspWebSocketClient` は受信バッファを超えるフレームを組み立て直せない
  （esp-idf-svc の既知の問題）。`buffer_size` を想定より大きく取る

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
