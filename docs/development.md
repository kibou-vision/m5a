# 開発環境

## ESP-IDF

* バージョン: release/v5.3
* 対象チップ: esp32, esp32s3

## Rust プロジェクト（本リポジトリの実装）

* [design](./design.md) の方針に従い、`esp-rs/esp-idf-template` を
  `cargo generate` で展開したプロジェクト構成を採用する。
* 対象チップ: esp32s3（`.cargo/config.toml` の `MCU` / `target` で指定）
* ESP-IDF は embuild が `ESP_IDF_TOOLS_INSTALL_DIR=workspace`（`.cargo/config.toml`
  で設定）によりプロジェクト配下へ自動取得・管理する。上記「セットアップ手順」で
  `~/esp/esp-idf` に取得する ESP-IDF（C言語実装用）とは別管理であり、
  バージョンも `ESP_IDF_VERSION`（`.cargo/config.toml`）で個別に指定する。

### セットアップ手順（macOS）

1. [espup](https://github.com/esp-rs/espup) で Rust の esp32s3 向けツールチェーンを
   インストールする

   ```sh
   cargo install espup
   espup install
   . $HOME/export-esp.sh
   ```

2. `ldproxy` と `espflash`（書き込み・モニタ用）をインストールする

   ```sh
   cargo install ldproxy espflash
   ```

3. ビルドする（初回は ESP-IDF がワークスペース配下に自動取得される）

   ```sh
   cargo build
   ```

4. 書き込み・モニタする

   ```sh
   cargo run
   ```

   M5Stack を USB で接続すると `/dev/cu.usbmodem*` が現れる。ポートを明示する
   場合は次のようにする。

   ```sh
   espflash flash --port /dev/cu.usbmodem101 --monitor target/xtensa-esp32s3-espidf/debug/m5a
   ```

5. コア層の単体テストを実行する

   ```sh
   cargo test-core
   ```

   詳細は [テスト方針](testing.md) を参照。

## C言語プロジェクト（Rust 実装が存在しない場合のフォールバック）

### セットアップ手順（macOS）

1. 前提ツールをインストールする

   ```sh
   brew install cmake ninja dfu-util
   ```

2. ESP-IDF を `~/esp/esp-idf` に取得する

   ```sh
   mkdir -p ~/esp
   git clone -b release/v5.3 --recursive https://github.com/espressif/esp-idf.git ~/esp/esp-idf
   ```

3. ツールチェーンをインストールする

   ```sh
   cd ~/esp/esp-idf
   ./install.sh esp32,esp32s3
   ```

   Python の SSL 証明書検証エラー（`CERTIFICATE_VERIFY_FAILED`）が出る場合は、
   `python.org` 版 Python の証明書がシステムに未反映であることが原因。
   `/Applications/Python <version>/Install Certificates.command` を実行してから
   再度 `install.sh` を実行する。

4. シェルごとに環境変数を読み込む

   ```sh
   . ~/esp/esp-idf/export.sh
   ```

### 動作確認

```sh
idf.py --version
```
