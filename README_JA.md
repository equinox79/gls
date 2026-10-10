# gls

[English](README.md) | **日本語**

[![CI](https://github.com/equinox79/gls/actions/workflows/ci.yml/badge.svg)](https://github.com/equinox79/gls/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**画像のための `ls`。** フォルダの画像を、ファイル名つきのサムネイル一覧としてターミナルに表示し、名前を `Ctrl+クリック` するとそのファイルを開けます。
*`ls` for images — list and view images in your terminal as thumbnails.*

![gls のデモ動画: パブリックドメインの画像6枚のサムネイル一覧が1行ずつ現れ、続いて -vv の詳細表示、ハーフブロック、アスキーアートで表示する](docs/images/demo.gif)

<sub>画像プロトコルに対応した端末での `gls -s m -v`（文字ではなく本物の画像）に続けて、`-vv`、`-m half`、`-m text`。サンプルの6枚はパブリックドメインです（[docs/CREDITS.md](docs/CREDITS.md)）。どのコマも `gls` の実際の出力から作ったもので、画面の録画ではありません（入力する様子は作り物です）。</sub>

## クイックスタート

```bash
cargo install --git https://github.com/equinox79/gls   # またはバイナリをダウンロード（インストールを参照）
gls                  # カレントディレクトリの画像を、サムネイル一覧で表示
gls -l --thumbs      # 1行ごとにサムネイルが付いた、詳細な一覧
```

## 特徴

- **端末の中に本物の画像を表示。** Sixel・Kitty・iTerm2 のグラフィックスを使います。使えない端末では、**ハーフブロック**（TrueColor / 256色）、さらに**アスキーアート**へ自動でフォールバック
- **多くの形式に対応:** PNG・JPEG・GIF・WebP・TIFF・BMP・**SVG**・**動画**・**PDF**・**HEIC**
- **ファイル名がリンク:** `Ctrl+クリック` で既定のアプリで開ける（OSC 8 ハイパーリンク）
- **`ls` の使い方に合わせた作り:** 正規表現での絞り込み、並び替え、再帰、EXIF の向きの反映、`-vv` の詳細表示、`-l` の詳細一覧（`--thumbs` でサムネイル付き）
- **スクリプトから使える:** 構造化した出力の `--json`、`xargs -0` 向けの `-0`
- **速い:** 並列デコード・先読み・キャッシュで、何千枚の画像でも待たない
- **Windows / macOS / Linux**（WSL を含む）。メッセージは **10言語**

**ファイル名を Ctrl+クリックすると、既定のビューワーで開きます。** ファイル名は端末のハイパーリンク（OSC 8）なので、対応している端末（Windows Terminal、iTerm2、WezTerm、Kitty、GNOME Terminal、VS Code など）で使えます（macOS は `Cmd+クリック`）。WSL では、Windows のアプリで開けるようにパスを変換します。パイプやリダイレクトのときは、リンクを付けません。

![gls の一覧でファイル名を Ctrl+クリックするイメージ図: 名前に下線が出て、「Ctrl + click to open」とツールチップが出たあと、その画像を映したビューワーのウィンドウが開く](docs/images/demo-click.gif)

<sub>これはイメージ図（作り物）で、画面の録画ではありません。一覧は `gls` の実際の出力ですが、マウスポインタ、ツールチップ、ビューワーのウィンドウは、動きを伝えるために描いたものです。実際に開くのは、その種類のファイルに OS が割り当てているアプリ（フォト、プレビュー、画像ビューアなど）です。</sub>

### ほかの画像ツールとの違い

端末で画像を見るツールは、[chafa](https://hpjansson.org/chafa/)、[viu](https://github.com/atanunq/viu)、[timg](https://github.com/hzeller/timg)、[lsix](https://github.com/hackerb9/lsix) など、すでによいものがあります。gls は、`ls` の使い方に合わせて作っています。名前とサイズの一覧、名前・日付・EXIF の時刻での絞り込みと並び替え、再帰、詳細表示、クリックできる名前、ほかのコマンドにつなげられる出力です。

## 目次

- [インストール](#インストール)
- [使い方](#使い方)
- [オプション](#オプション)
- [描画モードとフォールバック](#描画モードとフォールバック)
- [対応形式](#対応形式)
- [現状と既知の制限](#現状と既知の制限)
- [速さとキャッシュ](#速さとキャッシュ)
- [環境による注意](#環境による注意)
- [開発](#開発) ・ [言語](#言語) ・ [ライセンス](#ライセンス)

## インストール

### 方法1: ビルド済みのバイナリをダウンロードする（Rust は不要）

[Releases のページ](https://github.com/equinox79/gls/releases)から、お使いの OS 用のアーカイブをダウンロードして展開し、`gls`（Windows は `gls.exe`）を `PATH` の通ったフォルダに置きます。

> **ご注意:** 最初のリリースは、まだ公開していません。Releases のページに出るまでは、方法2を使ってください。

| OS | ファイル（バージョン `v0.1.0` の場合） |
| --- | --- |
| Windows（x64） | `gls-v0.1.0-x86_64-pc-windows-msvc.zip` |
| macOS（Apple シリコン） | `gls-v0.1.0-aarch64-apple-darwin.tar.gz` |
| macOS（Intel） | `gls-v0.1.0-x86_64-apple-darwin.tar.gz` |
| Linux / WSL（x64） | `gls-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` |
| Linux（ARM64） | `gls-v0.1.0-aarch64-unknown-linux-gnu.tar.gz` |

コピーして貼り付けられる例です（Linux の x64）。ほかの環境では、`VERSION` とターゲット名を変えてください。

```bash
VERSION=0.1.0
TARGET=x86_64-unknown-linux-gnu
curl -L -o gls.tar.gz "https://github.com/equinox79/gls/releases/download/v$VERSION/gls-v$VERSION-$TARGET.tar.gz"
tar xzf gls.tar.gz
sudo install "gls-v$VERSION-$TARGET/gls" /usr/local/bin/
```

Windows（PowerShell）の場合:

```powershell
$v = "0.1.0"
$t = "x86_64-pc-windows-msvc"
Invoke-WebRequest "https://github.com/equinox79/gls/releases/download/v$v/gls-v$v-$t.zip" -OutFile gls.zip
Expand-Archive gls.zip -DestinationPath .
# gls-v$v-$t\gls.exe を、PATH の通ったフォルダにコピーします
```

同じページの `SHA256SUMS` に、チェックサムがあります。macOS のバイナリは署名していないので、開けないと言われたら、`xattr -d com.apple.quarantine gls` を一度実行してください。Linux 版には glibc 2.35 以上（Ubuntu 22.04 以降）が必要です。

動画・PDF・HEIC のサムネイルを見るには、追加のツールが必要な場合があります。下の[必要なら: 動画・PDF・HEIC 用のツール](#必要なら-動画pdfheic-用のツール)を参照してください。

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

Windows の Rust には、Visual Studio の C++ Build Tools が必要です。無ければ `rustup` が入れるか尋ねるので、「C++ によるデスクトップ開発」を選んでください。

入れたあとは、新しいターミナルを開いて、`cargo` が `PATH` に通るようにします。

#### 2. gls をインストールする

```bash
cargo install --git https://github.com/equinox79/gls
```

ソースから入れる場合:

```bash
git clone https://github.com/equinox79/gls
cd gls
cargo install --path .
```

動画・PDF・HEIC のサムネイルを見るには、追加のツールが必要な場合があります。次の節[必要なら: 動画・PDF・HEIC 用のツール](#必要なら-動画pdfheic-用のツール)を参照してください。

### 必要なら: 動画・PDF・HEIC 用のツール

画像と SVG は追加のツールなしで表示できます。動画・PDF・HEIC/AVIF にはサムネイルを作る手段が必要で、gls は**まず OS のサムネイル、次に `PATH` にある外部ツール**の順で試します（[対応形式](#対応形式)を参照）。そのため、入れるべきものは OS によって違います。

| | 動画 | PDF | HEIC / AVIF | 動画の詳細（`-vv`、`-l`、`--json`） |
| --- | --- | --- | --- | --- |
| **macOS** | Quick Look | Quick Look | Quick Look | `ffprobe`（ffmpeg に付属） |
| **Windows** | エクスプローラーのサムネイル | エクスプローラーのサムネイル。**ただし PDF のサムネイルを作る仕組みが入っている場合だけ**（Adobe Acrobat や PDF-XChange などは作れますが、素の Windows は作れません）。なければ `mutool`、`pdftoppm`、ImageMagick のどれか | Microsoft Store の「HEIF 画像拡張機能」、または ImageMagick / `ffmpeg` | `ffprobe` |
| **Linux / WSL** | `ffmpeg` | `mutool`、`pdftoppm`、ImageMagick のどれか | ImageMagick 7（`magick`）か、HEIF を読める `ffmpeg` | `ffprobe` |

ツールが足りないと、そのセルには「(読み込み失敗)」と出て、**足りないツールの名前を挙げた警告が1回だけ**出ます。全部を入れる必要はありません。使うものだけ入れてください。

**インストールのコマンド**（コピーして貼り付けられます。ブロックごとに、その環境で必要なものだけを入れています）

macOS（Homebrew）。サムネイルは Quick Look が作るので、どれも任意です:

```bash
brew install ffmpeg mupdf poppler imagemagick
```

Windows（winget）:

```powershell
winget install Gyan.FFmpeg                # 動画のサムネイルと動画の詳細（ffmpeg、ffprobe）
winget install oschwartz10612.Poppler     # PDF のサムネイル（pdftoppm）
winget install ImageMagick.ImageMagick    # HEIC / AVIF、PDF の最後の手段
```

Windows（Scoop）:

```powershell
scoop install ffmpeg mupdf                # mupdf に mutool（PDF）が入っています
```

Debian / Ubuntu / WSL:

```bash
sudo apt install ffmpeg mupdf-tools poppler-utils
```

Fedora（動画のコーデックをすべて使うなら、`ffmpeg-free` ではなく RPM Fusion の `ffmpeg`）:

```bash
sudo dnf install ffmpeg-free mupdf poppler-utils ImageMagick
```

Arch:

```bash
sudo pacman -S ffmpeg mupdf-tools poppler imagemagick
```

入れたあとは、新しいターミナルを開いて `PATH` に通ったことを確認します。

```bash
ffmpeg -version     # 動画のサムネイル
ffprobe -version    # 動画の詳細
mutool -v           # PDF（または pdftoppm -v）
magick -version     # HEIC / AVIF（ImageMagick 7）
```

> Debian / Ubuntu の `imagemagick` はバージョン 6 で、`magick` コマンドがないため、gls からは使われません。これらの OS で HEIC/AVIF を扱うには、HEIF を読める `ffmpeg` を使うか、ImageMagick 7 を自分で入れてください。

> **名前について:** macOS で Homebrew の `coreutils` を入れていると、GNU の `ls` が `gls` という名前で入っています。
> 衝突する場合は、`cargo install` のあとに実行ファイルの名前を変えるか、シェルでエイリアスを使ってください。

## 使い方

```bash
gls                                # 引数なし: カレントディレクトリの画像を、サムネイル一覧で表示
gls photo.jpg                      # 1枚を表示
gls ./*                            # カレントディレクトリの画像を、サムネイル一覧で表示
gls photos/ -s m                   # サムネイルを大きく
gls . -R -e '\.png$' --sort date -n 20   # サブフォルダも含めて、PNG を新しい順に20枚
gls ./* -vv                        # ファイル名の下に、形式・EXIF などの詳細も表示
gls -l --thumbs                    # 1ファイル1枚のカード: サムネイル、名前、サイズ、日時、EXIF
gls --json -R photos | jq '.[].name'   # スクリプト向けの構造化した出力
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
| `-l`, `--long` | `ls -l` のような詳細表示。1ファイル1行で、サムネイルは出さない（`--thumbs` を参照） |
| `--thumbs` | 各画像のサムネイルを、情報の左に表示する。`-l` を含む |

`-l` は次の列を出し、表示するものがない列は省きます（ほかの列で、値がない項目は `-` にします）。

- サイズ、縦横、画素数、縦横比、形式と色、更新日時
- EXIF の撮影日時、カメラ、撮影設定（絞り・シャッター速度・ISO・焦点距離）、GPS の有無
- 動画は、撮影設定の代わりに、長さとコーデック

絞り込み・並び替え・`-R`・`-n` もそのまま使えます。見出しとリンクは、端末に出すときだけ付きます。

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

`--thumbs` を付けると、1ファイルが小さなカードになります。左にサムネイル、右に名前、続けてサイズ・縦横・形式、さらに日時と EXIF（あるものだけ。1グループ1行）を並べます。`-s xs|s|m|l|xl` でカードの高さ（3/4/5/7/9 行）を変えられます。サムネイル一覧と同じ描画なので、ハーフブロックやアスキーアートへのフォールバックも同じです。

![gls -l --thumbs -s m: パブリックドメインの画像6枚。それぞれ左にサムネイル、右に名前・サイズ・縦横・形式・日付が並ぶ](docs/images/demo-thumbs.jpg)

<sub>`gls -l --thumbs -s m`。`gls` の実際の出力から作った画像です。</sub>

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

### 出力の動作

| オプション | 説明 |
| --- | --- |
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

- **画像:** PNG・JPEG・GIF・BMP・WebP・TIFF・ICO・TGA・QOI。JPEG などの EXIF の向きは反映します。アニメーション GIF は最初のコマを表示します。
- **SVG:** そのまま描画します（外部ツール不要。白背景。`<text>` の文字は描画されません）。
- **動画（mp4, mov, mkv, webm, avi, m4v, wmv, flv, mpg, 3gp）・PDF（1ページ目）・HEIC / HEIF / AVIF:** サムネイルを作れる環境が必要です。次の順に試します。
  1. **OS のサムネイル** — Windows はエクスプローラーと同じもの（動画は標準で、PDF は Adobe Acrobat や PDF-XChange のような PDF のサムネイルを作る仕組みが入っている場合だけ、HEIC は Microsoft Store の「HEIF 画像拡張機能」を入れると対応）。入れるツールは[動画・PDF・HEIC 用のツール](#必要なら-動画pdfheic-用のツール)を参照、macOS は Quick Look
  2. **外部ツール**（PATH に通っていれば使います） — 動画は `ffmpeg`、PDF は `mutool` → `pdftoppm` → ImageMagick、HEIC/AVIF は ImageMagick（`magick`）→ `ffmpeg`

  どれも使えないときは、そのセルを「(読み込み失敗)」にして、必要なツールを案内する警告を1回だけ出します。

## 現状と既知の制限

gls はまだ若いツール（バージョン 0.1）です。確認できていることと、できていないことを書きます。

**確認できていること**
- Windows 11 の Windows Terminal（PowerShell と WSL の Ubuntu）: 画像の表示（Sixel）、ページャ、ファイル名の Ctrl+クリック、キャッシュ。
- `-l`、`--json`、`-0` は Windows で動かしていて、自動テストの対象です。
- 継続的インテグレーションで、Linux・macOS・Windows でビルドとテストを実行しています。テストは、実際の端末には描画しません。

**まだ実際の端末で確認できていないこと**（報告をとても歓迎します。[issue を立てて](https://github.com/equinox79/gls/issues)ください）
- `--thumbs` を、実際の画像プロトコルで表示したときの位置（端末の動きを再現するスクリプトでしか確認していません）
- iTerm2、WezTerm、Kitty、Ghostty、GNOME Terminal、foot、mlterm、VS Code のターミナル
- macOS の Terminal.app（画像プロトコルがないので `half` になります）と、macOS の Quick Look のサムネイル
- 動画と HEIC のサムネイル、ビルド済みのバイナリ（まだリリースを公開していません）

**既知の制限**
- RAW 写真（CR2、NEF、ARW など）には対応していません。
- 色のプロファイル（ICC）は見ていないので、広い色域の画像は、色が違って見えることがあります。
- SVG の文字は描画しません。PDF は1ページ目だけです。アニメーション GIF は最初のコマだけです。
- 画像プロトコルは、tmux や SSH 越しでは動かないことがあります。端末は環境変数から判定するので、外れたときは `--protocol` で指定してください。
- `-l` は、全ファイルを読み終えてから出力するので、WSL の `/mnt/c` のように遅い場所で長い一覧を出すと、表示までに少し待ちます。
- 1枚だけの指定（`gls photo.jpg`）は、キャッシュしません。
- 英語と日本語以外の翻訳は、機械翻訳です。

## 速さとキャッシュ

- 一覧は複数枚を並列にデコード・描画します。最初の1行ができたらすぐ表示を始め、数行先まで先読みします。
- JPEG は 1/2・1/4・1/8 に縮小しながらデコードします。一覧では、足りる大きさなら EXIF サムネイル（約160px）を使い、本体を読まずに済ませます。
- 描画結果をキャッシュします。画像のパス・更新日時・サイズと、表示設定が同じなら、2回目以降はデコードしません。`image` モードでは、時間のかかる動画・PDF・HEIC/AVIF・SVG の縮小サムネイルもキャッシュするので、ffmpeg などを再び起動しません。キャッシュするのは一覧のときだけで、1枚だけの指定（`gls video.mp4`）は毎回作り直します。何がキャッシュされているかは `--cache-info` で見られます。

<details>
<summary>キャッシュの場所と整理</summary>

- 保存先: Windows は `%LOCALAPPDATA%\gls\cache`、Linux / macOS は `~/.cache/gls/cache`（`XDG_CACHE_HOME` 優先）。
- キャッシュには、画像の小さなサムネイルが残ります。書き込みたくないときは `--no-cache`、消すときは `--clear-cache` を使ってください。
- 古いキャッシュは、1日に1回、起動時にバックグラウンドで整理します。最後に使われてから 30 日を過ぎたものと、合計 256 MB を超えた分（古い順）を削除します。オプション `--cache-days` と `--cache-max-mb`（すぐ反映）、または環境変数 `GLS_CACHE_DAYS` と `GLS_CACHE_MAX_MB`（次の1日1回の整理で反映）で変えられます。オプションが環境変数より優先されます。

</details>

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
| `src/main.rs` | 引数、入力の展開、並び替え、一覧とカードの描画、ページャ |
| `src/gfx.rs` | Sixel / Kitty / iTerm2 の出力と端末の判定 |
| `src/ext.rs` | SVG・動画・PDF・HEIC の取得（OS のサムネイル、外部ツール） |
| `src/info.rs` | `-v` / `-vv` / `-l` の画像情報 |
| `src/json.rs` | `--json` の出力 |
| `src/orient.rs`, `src/thumb.rs` | EXIF の向き、EXIF サムネイル |
| `src/link.rs` | ファイル名のハイパーリンク（OSC 8） |
| `src/i18n.rs`, `locales/*.txt` | 言語の選択とメッセージの辞書 |
| `tests/cli.rs`, `tests/data/` | 小さな合成画像で、実際のバイナリを動かすテスト |
| `.github/workflows/` | `ci.yml`（整形・lint・3つの OS でのテスト）と `release.yml`（バイナリと GitHub のリリース） |
| `scripts/package.sh` | リリースビルドをアーカイブにまとめる（`release.yml` が使う） |

### リリースの手順（メンテナ向け）

1. `Cargo.toml` のバージョンを上げます（`cargo build` を実行して `Cargo.lock` も追従させます）。コミットして push します。
2. 任意の予行演習: Actions のタブで **Release** → **Run workflow** を選びます。5つの環境でビルドして、アーカイブを成果物として残します。公開はしません。
3. バージョンに合うタグを push します: `git tag v0.1.0 && git push origin v0.1.0`。ワークフローが、タグと `Cargo.toml` のバージョンが同じことを確かめ、テストを実行し、Linux（x64、ARM64）、macOS（Apple シリコン、Intel）、Windows（x64）をビルドして、アーカイブと `SHA256SUMS` を付けた GitHub のリリースを公開します。

## 言語

<details>
<summary>10言語: English・日本語・简体中文・繁體中文・한국어・Español・Français・Deutsch・Português (Brasil)・Русский</summary>

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

</details>

## ライセンス

[MIT](LICENSE)
