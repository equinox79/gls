# gls

[English](README.md) | **日本語**

**画像のための `ls`。** フォルダの画像を、ファイル名つきのサムネイル一覧としてターミナルに表示します。
*`ls` for images — list and view images in your terminal as thumbnails.*

- 端末の画像プロトコル（**Sixel / Kitty / iTerm2**）で本物の画像を表示。非対応の端末では、**ハーフブロック**（TrueColor / 256色）、さらに**アスキーアート**へ自動でフォールバック
- PNG・JPEG・GIF・WebP・TIFF・BMP・**SVG**・**動画**・**PDF**・**HEIC** に対応
- 正規表現での絞り込み、並び替え、件数の制限、再帰、EXIF の向きの反映、`-vv` で EXIF などの詳細表示
- ファイル名を `Ctrl+クリック` で既定のアプリで開ける（OSC 8 ハイパーリンク）
- 並列デコード・先読み・キャッシュで、大量の画像でも速い
- Windows / macOS / Linux（WSL を含む）。ビルド済みのバイナリを [Releases のページ](https://github.com/equinox79/gls/releases)で配布
- スクリプトから使える: 構造化した出力の `--json`、`xargs -0` 向けの `-0`、`ls -l` 風の `-l`
- メッセージは10言語（English・日本語・简体中文・繁體中文・한국어・Español・Français・Deutsch・Português (Brasil)・Русский）に対応し、環境から自動で選びます。言語は簡単に追加できます（[言語](#言語)を参照）

![gls のデモ動画: パブリックドメインの画像6枚のサムネイル一覧が1行ずつ現れ、続いて -vv の詳細表示、ハーフブロック、アスキーアートで表示する](docs/images/demo.gif)

<sub>画像プロトコルに対応した端末での `gls -s m -v`（文字ではなく本物の画像）に続けて、`-vv`、`-m half`、`-m text`。サンプルの6枚はパブリックドメインです（[docs/CREDITS.md](docs/CREDITS.md)）。どのコマも `gls` の実際の出力から作ったもので、画面の録画ではありません（入力する様子は作り物です）。</sub>

## インストール

### 方法1: ビルド済みのバイナリをダウンロードする（Rust は不要）

[Releases のページ](https://github.com/equinox79/gls/releases)から、お使いの OS 用のアーカイブをダウンロードして展開し、`gls`（Windows は `gls.exe`）を `PATH` の通ったフォルダに置きます。

| OS | ファイル |
| --- | --- |
| Windows（x64） | `gls-<バージョン>-x86_64-pc-windows-msvc.zip` |
| macOS（Apple シリコン） | `gls-<バージョン>-aarch64-apple-darwin.tar.gz` |
| macOS（Intel） | `gls-<バージョン>-x86_64-apple-darwin.tar.gz` |
| Linux / WSL（x64） | `gls-<バージョン>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux（ARM64） | `gls-<バージョン>-aarch64-unknown-linux-gnu.tar.gz` |

同じページの `SHA256SUMS` に、チェックサムがあります。macOS のバイナリは署名していないので、開けないと言われたら、`xattr -d com.apple.quarantine gls` を一度実行してください。Linux 版には glibc 2.35 以上（Ubuntu 22.04 以降）が必要です。

### 方法2: ソースからビルドする

#### 1. Rust とリンカを用意する

gls は [Rust](https://rustup.rs/) でビルドします。Rust は OS のリンカを使うので、先にそれを入れます。

**macOS**

```bash
xcode-select --install                                          # Xcode コマンドラインツール（リンカ）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  # Rust
```

**Linux / WSL**（Debian / Ubuntu の場合。ほかのディストリビューションでは `build-essential` に当たるものを入れてください）

```bash
sudo apt install build-essential curl                           # C コンパイラとリンカ
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  # Rust
```

**Windows**

```powershell
winget install Rustlang.Rustup
```

Windows の Rust には Visual Studio の C++ ビルドツールが必要です。入っていなければ `rustup` がインストールを案内するので、「C++ によるデスクトップ開発」を選んでください。

終わったら、`cargo` を PATH に通すためにターミナルを開き直してください。

#### 2. gls をインストールする

```bash
cargo install --git https://github.com/equinox79/gls
```

ソースから:

```bash
git clone https://github.com/equinox79/gls
cd gls
cargo install --path .
```

### 必要なら: 動画・PDF・HEIC 用のツール

画像と SVG は追加のツールなしで表示できます。動画・PDF・HEIC/AVIF にはサムネイルを作る手段が必要です（[対応形式](#対応形式)を参照）。

| OS | 標準で使えるもの | 必要に応じて入れるもの |
| --- | --- | --- |
| macOS | Quick Look が動画・PDF・HEIC に対応 | `brew install ffmpeg`（`-vv` で動画の詳細を出す場合） |
| Windows | エクスプローラーのサムネイルが動画・PDF に対応 | HEIC: Microsoft Store の「HEIF 画像拡張機能」。`winget install Gyan.FFmpeg`（`-vv` で動画の詳細を出す場合） |
| Linux / WSL | なし | `sudo apt install ffmpeg mupdf-tools`（動画と PDF）。HEIC/AVIF には ImageMagick 7（`magick` コマンド）か、HEIF を読める `ffmpeg` |

> Ubuntu の `imagemagick` パッケージはバージョン 6 で `magick` コマンドが無いため、gls からは使われません。

> **名前について:** macOS で Homebrew の `coreutils` を入れていると、GNU の `ls` が `gls` という名前で入っています。
> 衝突する場合は、`cargo install` のあとに実行ファイルの名前を変えるか、シェルでエイリアスを使ってください。

## 使い方

```bash
gls photo.jpg                      # 1枚を表示
gls                                # 引数なし: カレントディレクトリの画像を、サムネイル一覧で表示
gls ./*                            # カレントディレクトリの画像を、サムネイル一覧で表示
gls photos/ -s m                   # サムネイルを大きく
gls . -R -e '\.png$' --sort date -n 20   # サブフォルダも含めて、PNG を新しい順に20枚
gls ./* -vv                        # ファイル名の下に、形式・EXIF などの詳細も表示
```

- 引数なしなら、カレントディレクトリを一覧にします。画像を複数指定すると一覧になります。ワイルドカードやディレクトリも指定できます（PowerShell や cmd でも `*` が使えます）。画像以外のファイルは無視します。
- 出力が画面の高さを超えると、`more` と同じように1画面ごとに止まります（`Space`/`f`/`PageDown`: 次の画面、`Enter`/`j`/`↓`: 1行、`q`/`Esc`/`Ctrl+C`: 終了）。パイプやリダイレクトのときは止まらず全部出力します。
- 長いファイル名は、サムネイルの幅で最大3行まで折り返して表示します。3行に収まらないときは、最終行を `...png` のように拡張子を残して省略します。
- 端末の横幅に収まるだけ、サムネイルを横に並べます（`-g COLSxROWS` で列数を指定）。
- 表示に 0.15 秒以上かかるときは、標準エラー出力に `⠋ 読み込み中 12/86`（英語の環境では `⠋ loading 12/86`）のようなインジケータを出します（リダイレクト時は出しません）。

## オプション

### 絞り込み・並び替え

| オプション | 説明 |
| --- | --- |
| `-e`, `--regexp <REGEX>` | ファイル名（ディレクトリ部分を除く）を正規表現で絞り込む。複数指定すると、どれかに一致すれば表示。大文字小文字を無視するには `(?i)` を付ける（例: `-e '(?i)\.jpe?g$'`） |
| `-R`, `--recursive` | サブディレクトリも探す（`.git` などの隠しディレクトリは除く）。ファイル名の前に親ディレクトリ名も表示する |
| `--max-depth <N>` | サブディレクトリに入る深さ（`0`: 直下のみ、`1`: 1階層下まで）。指定すると `-R` がなくても再帰する |
| `--sort <KEY>` | `name`（既定。名前の昇順で、`img2` < `img10` のように数字は数値として比較）/ `date`（更新日時の新しい順）/ `size`（大きい順）/ `exif-date`（EXIF の撮影日時の新しい順。なければ更新日時）/ `none`（指定された順のまま） |
| `-r`, `--reverse` | 並び順を逆にする |
| `-n`, `--head <N>` | 並び替えのあと、先頭から N 枚だけ表示する |

### サイズ・レイアウト

| オプション | 説明 |
| --- | --- |
| `-s`, `--size <xs\|s\|m\|l\|xl>` | サイズのプリセット（既定: `s`）。単体表示は 24/40/80/120/160 文字幅、一覧はサムネイル1枚が 12/20/40/60/80 文字幅 |
| `-w`, `--width <N>` | 横幅を文字数で指定（`--size` とは併用不可）。一覧では全体の幅 |
| `-g`, `--grid <COLSxROWS>` | 一覧のグリッド。省略すると、端末の横幅に収まるだけ列数を増やす（行数は2）。`--width` だけを指定したときは `3x2` |
| `-v`, `-vv` | ファイル名の下に画像情報を表示。`-v`: ピクセルの縦横とファイルサイズ。`-vv`: さらに形式・色・画素数・縦横比・更新日・EXIF（カメラ、絞り・シャッター速度・ISO・焦点距離、撮影日、GPS の有無）。取得できない項目は省く |

### 詳細表示

| オプション | 説明 |
| --- | --- |
| `-l`, `--long` | `ls -l` のような詳細表示。1ファイル1行で、サイズ・縦横・画素数・縦横比・形式と色・更新日時・EXIF の撮影日時・カメラ・撮影設定（絞り・シャッター速度・ISO・焦点距離。動画は長さとコーデック）・GPS の有無を並べる。サムネイルは、`--thumbs` を付けたときだけ出す。表示するものがない列は省き、値がない項目は `-` にする。絞り込み・並び替え・`-R`・`-n` もそのまま使える。見出しとリンクは、端末に出すときだけ付く |
| `--thumbs` | `-l` と合わせて、各画像のサムネイルを情報の左に表示する（下の説明を参照）。`-l` を含む |

```
$ gls -l
273.8KB   960x966  0.9MP  0.99:1  JPEG RGB 8bit  2026-10-08 11:24  apollo11-aldrin.jpg
230.3KB   960x960  0.9MP  1:1     JPEG RGB 8bit  2026-10-08 11:24  blue-marble.jpg
 62.9KB   960x960  0.9MP  1:1     JPEG RGB 8bit  2026-10-08 11:24  earthrise.jpg
249.1KB   960x645  0.6MP  1.49:1  JPEG RGB 8bit  2026-10-08 11:24  great-wave.jpg
505.2KB  960x1431  1.4MP  0.67:1  JPEG RGB 8bit  2026-10-08 11:24  mona-lisa.jpg
341.8KB   960x760  0.7MP  24:19   JPEG RGB 8bit  2026-10-08 11:24  starry-night.jpg
```

EXIF のある写真では、さらに撮影日時・カメラ・撮影設定（例: `f/11 1/125s ISO100 16mm`）の列が付きます。この例はパイプ出力なので見出しがありません。端末に出すと、太字の見出し行が付きます。

`--thumbs` を付けると、1ファイルが小さなカードになります。左にサムネイル、右に名前、続けてサイズ・縦横・形式、さらに日時と EXIF（あるものだけ。1グループ1行）を並べます。`--thumbs` は `-l` を含み、`-s xs|s|m|l|xl` でカードの高さ（3/4/5/7/9 行）を変えられます。サムネイル一覧と同じ描画なので、ハーフブロックやアスキーアートへのフォールバックも同じです。

```
gls -l --thumbs -s m
```

### スクリプト向け

| オプション | 説明 |
| --- | --- |
| `--json` | 条件に合うファイルの情報を JSON で出力して終了する。1ファイル1オブジェクトの配列。絞り込み・並び替え・`-R`・`-n` もそのまま使える。`-l` / `--thumbs` / `-0` とは併用できない |
| `-0`, `--null` | 条件に合うファイルのパスだけを、NUL 文字区切りで出力して終了する（`find -print0` と同じ）。`xargs -0` 用。パスはバイト列のまま出す |

```
$ gls --json tests/data/gradient.png
[
{"path":"tests/data/gradient.png","name":"gradient.png","type":"image","bytes":2571,"width":96,"height":64,"megapixels":0.01,"aspect":"3:2","format":"PNG","color":"RGBA 8bit","modified":"2026-10-08T11:05:04.335445900+09:00","details":[],"exif":null}
]
```

どのオブジェクトも同じキーを持ち、`null` は「取得できなかった」を表します。

| キー | 意味 |
| --- | --- |
| `path`, `name` | gls が見つけたパスと、ファイル名 |
| `type` | `image`、`svg`、`video`、`pdf`、`heif` のどれか |
| `bytes`, `width`, `height`, `megapixels`, `aspect` | 数値（`aspect` は `16:9` のような文字列）。動画・PDF・HEIC は大きさを読まないので `null` |
| `format`, `color` | 例: `JPEG`、`RGB 8bit` |
| `modified` | 更新日時（RFC 3339） |
| `details` | 動画の長さやコーデックなど（`ffprobe` が必要）。なければ空 |
| `exif` | `null`、または `camera`、`aperture`、`exposure`、`iso`、`focal_length`、`taken`（ファイルの記録のままの文字列。例: `f/11`、`1/125s`、`ISO100`）と `gps`（真偽値）を持つオブジェクト |

```bash
# 特定のカメラで撮った写真を、撮影日時つきで一覧する
gls --json -R photos | jq -r '.[] | select(.exif.camera // "" | test("SONY")) | [.exif.taken, .path] | @tsv'

# 新しい PNG 20枚を別の場所へコピーする（スペースのあるファイル名でも大丈夫）
gls -0 -R -e '\.png$' --sort date -n 20 | xargs -0 cp -t backup/
```

警告とエラーは標準エラー出力に出るので、この出力には混ざりません。

### 描画
| オプション | 説明 |
| --- | --- |
| `-m`, `--mode <image\|half\|text>` | 描画モード（既定: `image`。使えない端末では下記のとおり自動でフォールバック） |
| `--protocol <auto\|sixel\|kitty\|iterm2>` | 画像プロトコルの指定（既定: `auto`） |
| `--cell-size <WxH>` | 文字セル1つのピクセルサイズ（例: `10x20`）。Sixel で画像と文字の位置がずれるときに調整する |
| `-p`, `--palette <NAME>` | `text` モードの文字セット: `standard` `block` `simple` `binary` `retro` `dots` |
| `--custom-palette <CHARS>` | 任意の文字セット（暗い → 明るい順。`--palette` より優先） |
| `-i`, `--invert` | 明暗を反転（`text` モード） |
| `--contrast <F>` / `--gamma <F>` | コントラスト・ガンマの補正（既定 `1.0`） |
| `--no-color` | カラーを使わない（モノクロの `text` モードになる） |
| `--no-links` | ファイル名にハイパーリンクを付けない |
| `--no-pager` | 画面に収まらなくても止まらない |
| `--lang <コード>` | メッセージの言語（`en`、`ja`、`zh-cn`、`zh-tw`、`ko`、`es`、`fr`、`de`、`pt-br`、`ru`）。省略時は環境から判定。[言語](#言語)を参照 |

### キャッシュ

| オプション | 説明 |
| --- | --- |
| `--no-cache` | 描画キャッシュを使わない |
| `--cache-max-mb <MB>` | キャッシュ容量の上限（既定 256、環境変数 `GLS_CACHE_MAX_MB`）。超えた分の古いものはすぐ削除する |
| `--cache-days <日数>` | この日数を使われなかったものを削除（既定 30、環境変数 `GLS_CACHE_DAYS`） |
| `--cache-info` | キャッシュの場所、種類ごとの件数と容量、最後に使われてからの経過日数（最も古い・新しい）、上限を表示して終了 |
| `--clear-cache` | 描画キャッシュをすべて削除して終了 |

## 描画モードとフォールバック

デフォルトの `image` は、端末の対応に合わせて、次の順に自動で切り替わります。
デフォルトのときは黙って切り替わり、`-m image` を明示して使えなかったときだけ警告します。

1. **`image`** — Sixel / Kitty / iTerm2 が使える端末
2. **`half`（TrueColor）** — ハーフブロック `▀` で、1文字に上下2ピクセルを描く。TrueColor 対応の端末（Windows、`COLORTERM=truecolor`、既知の端末）
3. **`half`（256色）** — `TERM` が `*256color` の端末
4. **`text`（モノクロ）** — 文字の濃淡で描くアスキーアート。上記のどれでもない端末

同じフォルダを、画像プロトコルのない端末（`half`、左）と `-m text`（右、アスキーアート）で表示した例です。

| `half`（TrueColor） | `-m text` |
| --- | --- |
| ![ハーフブロック描画](docs/images/demo-half.jpg) | ![アスキーアート描画](docs/images/demo-text.jpg) |

`-m half` / `-m text` を明示すれば、判定せずにそのモードで表示します。

画像プロトコルは、環境変数から自動で判定します。

| 端末 | プロトコル |
| --- | --- |
| Kitty、Ghostty | Kitty |
| iTerm2、WezTerm | iTerm2 |
| Windows Terminal（WSL 内も可、1.22 以降）、foot、mlterm | Sixel |

判定できない端末でも、`--protocol sixel` のように指定すれば使えます。tmux 経由では動かないことがあります。

## 対応形式

- **画像:** PNG・JPEG・GIF・BMP・WebP・TIFF・ICO・TGA・QOI。JPEG などの EXIF の向きは反映します。
- **SVG:** そのまま描画します（外部ツール不要。白背景。`<text>` の文字は描画されません）。
- **動画（mp4, mov, mkv, webm, avi, m4v, wmv, flv, mpg, 3gp）・PDF（1ページ目）・HEIC / HEIF / AVIF:** サムネイルを作れる環境が必要です。次の順に試します。
  1. **OS のサムネイル** — Windows はエクスプローラーと同じもの（動画・PDF は標準で、HEIC は Microsoft Store の「HEIF 画像拡張機能」を入れると対応）、macOS は Quick Look
  2. **外部ツール**（PATH に通っていれば使います） — 動画は `ffmpeg`、PDF は `mutool` → `pdftoppm` → ImageMagick、HEIC/AVIF は ImageMagick（`magick`）→ `ffmpeg`

  どれも使えないときは、そのセルを「(読み込み失敗)」にして、必要なツールを案内する警告を1回だけ出します。

## ファイル名のリンク

対応端末（Windows Terminal、iTerm2、WezTerm、Kitty、GNOME Terminal、VS Code など）では、ファイル名が
`Ctrl+クリック`（macOS は `Cmd+クリック`）で既定のアプリで開けるリンクになります（OSC 8 ハイパーリンク）。
出力が端末でないとき（リダイレクト・パイプ）は付けません。

WSL では、`/mnt/c/...` を `C:/...` に、それ以外を `\\wsl.localhost\<ディストリビューション>\...` 形式に変換して、Windows 側のアプリで開きます。

## 速さ

- 一覧は複数枚を並列にデコード・描画します。最初の1行ができたらすぐ表示を始め、数行先まで先読みします。
- JPEG は 1/2・1/4・1/8 に縮小しながらデコードします。一覧では、足りる大きさなら EXIF サムネイル（約160px）を使い、本体を読まずに済ませます。
- 描画結果をキャッシュします。画像のパス・更新日時・サイズと、表示設定が同じなら、2回目以降はデコードしません。`image` モードでは、時間のかかる動画・PDF・HEIC/AVIF・SVG の縮小サムネイルもキャッシュするので、ffmpeg などを再び起動しません。キャッシュするのは一覧のときだけで、1枚だけの指定（`gls video.mp4`）は毎回作り直します。何がキャッシュされているかは `--cache-info` で見られます。
  - 保存先: Windows は `%LOCALAPPDATA%\gls\cache`、Linux / macOS は `~/.cache/gls/cache`（`XDG_CACHE_HOME` 優先）
  - 古いキャッシュは、1日に1回、起動時にバックグラウンドで整理します。最後に使われてから 30 日を過ぎたものと、合計 256 MB を超えた分（古い順）を削除します。オプション `--cache-days` と `--cache-max-mb`（すぐ反映）、または環境変数 `GLS_CACHE_DAYS` と `GLS_CACHE_MAX_MB`（次の1日1回の整理で反映）で変えられます。オプションが環境変数より優先されます。

## 環境による注意

- `> file.txt` で保存するとき、Windows PowerShell 5 は UTF-16 で書き出します。UTF-8 にするには PowerShell 7 を使ってください。
- 旧 Windows コンソール（conhost）でも、ANSI の出力を自動で有効化します。ただし画像プロトコルは使えず、`half` で表示します。
- 出力が端末でないとき（パイプなど）は、横幅を環境変数 `COLUMNS` から取ります（なければ 80）。
- WSL の `/mnt/c` のように読み込みの遅い場所では、`-m half` や `-s xs` のほうが速くなります（EXIF サムネイルを使えるため）。

## 開発

```bash
cargo build --release
cargo test            # 単体テスト + tests/cli.rs（tests/data の小さな合成画像を表示）
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

ソースの構成:

| ファイル | 内容 |
| --- | --- |
| `src/main.rs` | 引数、入力の展開、並び替え、一覧の描画、ページャ |
| `src/gfx.rs` | Sixel / Kitty / iTerm2 の出力と端末の判定 |
| `src/ext.rs` | SVG・動画・PDF・HEIC の取得（OS のサムネイル、外部ツール） |
| `src/info.rs` | `-v` / `-vv` の画像情報 |
| `src/orient.rs`, `src/thumb.rs` | EXIF の向き、EXIF サムネイル |
| `src/link.rs` | ファイル名のハイパーリンク（OSC 8） |
| `src/i18n.rs`, `locales/*.txt` | 言語の選択とメッセージの辞書 |

## 言語

ヘルプ・警告・エラー・画像情報のラベルは、次の言語に対応しています。English が既定で、訳の基準です。日本語以外の訳は機械翻訳なので、修正はとても歓迎です（`locales/<コード>.txt` を直す PR がいちばん簡単です）。

| コード | 言語 | コード | 言語 |
| --- | --- | --- | --- |
| `en` | English | `es` | Español |
| `ja` | 日本語 | `fr` | Français |
| `zh-cn` | 简体中文 | `de` | Deutsch |
| `zh-tw` | 繁體中文 | `pt-br` | Português (Brasil) |
| `ko` | 한국어 | `ru` | Русский |

地域つきの名前も扱えます。`pt_BR.UTF-8`、`zh-Hant-TW`、`zh_HK` などは、いちばん近い辞書を選びます（`pt` は `pt-br`、`zh` と `zh-Hans` は `zh-cn`、`zh-HK` と `zh-Hant` は `zh-tw` になります）。
言語は次の順で決まります。

1. `--lang <コード>`（例: `--lang ja`）
2. 環境変数 `GLS_LANG`、`LC_ALL`、`LC_MESSAGES`、`LANG`（例: `ja_JP.UTF-8`）
3. Windows の表示言語
4. English

訳がない項目は English で表示します。（`--help` の `Usage:` など clap 自身の語句は English のままです。）

**言語を追加するには:**

1. `locales/en.txt` を `locales/<コード>.txt`（小文字、例: `fr`、`zh-cn`）にコピーして訳します。`{placeholder}` はそのまま残します。
2. `src/i18n.rs` の `CATALOGS` に1行足します: `("fr", include_str!("../locales/fr.txt")),`
3. `cargo test` を実行します。すべてのキーと `{placeholder}` が `en.txt` と合っているか確かめます。

## ライセンス

[MIT](LICENSE)
