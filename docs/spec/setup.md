# 初回セットアップ

## 手順

1. FAT32 で初期化した microSD カードを CoreS3 に挿し、電源を入れる
2. 本体が `/.m5a/config.toml` を作る（記入例つきの雛形）
3. 電源を切って SD カードを取り出し、PC に挿す
4. `/.m5a/config.toml` をテキストエディタで開き、`""` の中を書き換えて保存する
5. SD カードを本体に戻して電源を入れる

## 設定ファイル

置き場所は `/.m5a/config.toml`。書式は TOML。

| 節 | 鍵 | 内容 | 省略 |
|---|---|---|---|
| `child` | `name` | 子どもの名前。アシスタントがこの名前で呼びかける | 不可 |
| `child` | `age` | 年齢。話し方の難易度の目安 | 可（5） |
| `assistant` | `name` | アシスタント自身の名前。名前を聞かれたらこの名前で答える | 可（`アシスタント`） |
| `wifi` | `ssid` | 接続先の Wi-Fi 名。2.4GHz のみ | 不可 |
| `wifi` | `password` | Wi-Fi のパスワード | 不可 |
| `openai` | `api_key` | OpenAI の APIキー | 不可 |
| `openai` | `model` | 使うモデル | 可（`gpt-realtime-2.1-mini`） |
| `openai` | `voice` | 声の種類 | 可（`marin`） |
| `openai` | `audio_format` | `ulaw` または `pcm16` | 可（`ulaw`） |
| `search` | `api_key` | Tavily の APIキー。web検索を使う場合のみ記入する | 可（空欄なら検索を使わない） |

`voice` に使える値は
`alloy` / `ash` / `ballad` / `coral` / `echo` / `sage` / `shimmer` / `verse` /
`marin` / `cedar`。OpenAI は `marin` と `cedar` を推奨している。

`search.api_key` は [Tavily](https://www.tavily.com/) の無料アカウントで
発行できる。空欄のままなら、アシスタントはモデルが知っている範囲でだけ
答え、web検索は行わない。

## 記入漏れの扱い

雛形のままの値や空欄が残っていると起動を止め、何が足りず、どう直せばよいかを
表示する。記入漏れは親がファイルを直せば回復するため、端末側では再起動以外の
操作を求めない。

## APIキーの取り扱い

OpenAI は標準の APIキーを「安全なサーバ上でのみ使う」構成を推奨しており、
持ち出される端末に置く本構成はその推奨から外れる。次の対策を勧める。

* この端末専用の OpenAI プロジェクトを作り、そのプロジェクトのキーを使う
* プロジェクトに毎月の利用上限を設定する
* 端末や SD カードを紛失したら、そのキーを失効させる

## 会話ログ

文字起こしが `/.m5a/logs/YYYY-MM-DD.txt` に追記される。時刻が取れていない間は
`date-unknown.txt` に入る。音声は保存しない。
