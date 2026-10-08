//! gls - Image viewer / thumbnail lister for the terminal
//! Supports 24-bit TrueColor and custom character palettes.

mod ext;
mod gfx;
mod info;
mod link;
mod orient;
mod thumb;

use std::hash::{Hash, Hasher};
use std::io::{self, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use clap::{Parser, ValueEnum};
use image::imageops::FilterType;
use rayon::prelude::*;

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "ico", "tga", "qoi", "avif",
];
/// キャッシュの内容が変わる変更（向きの反映など）をしたときに上げる。古いキャッシュを使わないための番号
const CACHE_FORMAT: u32 = 2;
const GRID_GAP: u32 = 2;
/// ファイル名を折り返して表示する最大行数
const NAME_LINES: usize = 3;
const DEFAULT_GRID: Grid = Grid { cols: 3, rows: 2 };

/// プリセットパレット（暗い -> 明るい）
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Palette {
    Standard,
    Block,
    Simple,
    Binary,
    Retro,
    Dots,
}

impl Palette {
    fn chars(self) -> &'static str {
        match self {
            Palette::Standard => " .:-=+*#%@",
            Palette::Block => " ░▒▓█",
            Palette::Simple => " .:+*#@",
            Palette::Binary => " 01",
            Palette::Retro => " .oO8@",
            Palette::Dots => " .··::***",
        }
    }
}

/// 出力サイズのプリセット
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Size {
    Xs,
    S,
    M,
    L,
    Xl,
}

impl Size {
    /// 単体表示の横幅（文字数）
    fn single_width(self) -> u32 {
        match self {
            Size::Xs => 24,
            Size::S => 40,
            Size::M => 80,
            Size::L => 120,
            Size::Xl => 160,
        }
    }

    /// 一覧表示でのサムネイル1枚の横幅（文字数）
    fn cell_width(self) -> u32 {
        match self {
            Size::Xs => 12,
            Size::S => 20,
            Size::M => 40,
            Size::L => 60,
            Size::Xl => 80,
        }
    }
}

/// 並び替えの基準
#[derive(Clone, Copy, Debug, ValueEnum)]
enum SortKey {
    /// 指定された順（ワイルドカードの展開順）のまま
    None,
    /// ファイル名（数字は数値として比較: img2 < img10）。昇順
    Name,
    /// 更新日時。新しい順
    Date,
    /// ファイルサイズ。大きい順
    Size,
    /// EXIF の撮影日時（なければ更新日時）。新しい順
    ExifDate,
}

/// 描画モード
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    /// 文字の濃淡で表現（パレット使用）
    Text,
    /// ハーフブロック `▀` で上下2ピクセルを1文字に描画（縦解像度2倍・要カラー）
    Half,
    /// 端末の画像プロトコル（Sixel / Kitty / iTerm2）で本物の画像を表示。非対応なら half
    Image,
}

/// 画像プロトコルの指定（--mode image 用）
#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProtoArg {
    /// 環境変数から自動判定
    Auto,
    Sixel,
    Kitty,
    Iterm2,
}

fn parse_cell_size(s: &str) -> Result<(u32, u32), String> {
    let err = || format!("`{s}` は WxH の形式（例: 10x20）で指定してください");
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(err)?;
    let (w, h): (u32, u32) = (w.parse().map_err(|_| err())?, h.parse().map_err(|_| err())?);
    if w == 0 || h == 0 {
        return Err(err());
    }
    Ok((w, h))
}

/// 描画設定
#[derive(Clone)]
struct Opts {
    palette: Vec<char>,
    color: bool,
    half: bool,
    /// 色を TrueColor ではなく 256色パレット（38;5;N）で出力する
    ansi256: bool,
    /// ガンマ・コントラスト補正のルックアップテーブル
    lut: [u8; 256],
    /// 縮小フィルタ（一覧は速さ重視、単体は品質重視）
    filter: FilterType,
    /// 描画キャッシュの保存先（None なら無効）
    cache_dir: Option<PathBuf>,
    /// 見た目に影響する設定すべてのハッシュ（キャッシュキーに使う）
    fingerprint: u64,
    /// 画像プロトコルで表示する場合の設定（None なら文字で描画）
    gfx: Option<gfx::Gfx>,
    /// ファイル名の下に付ける画像情報の詳しさ（0: なし, 1: -v, 2: -vv）
    verbose: u8,
    /// ファイル名にハイパーリンク（OSC 8）を付ける
    links: bool,
    /// ラベルに親ディレクトリ名も付ける（再帰表示で、同名ファイルを見分けるため）
    show_parent: bool,
}

impl Opts {
    #[allow(clippy::too_many_arguments)]
    fn new(
        palette: Vec<char>,
        color: bool,
        half: bool,
        ansi256: bool,
        lut: [u8; 256],
        filter: FilterType,
        cache_dir: Option<PathBuf>,
        gfx: Option<gfx::Gfx>,
    ) -> Self {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        env!("CARGO_PKG_VERSION").hash(&mut h);
        CACHE_FORMAT.hash(&mut h);
        palette.hash(&mut h);
        (color, half, ansi256).hash(&mut h);
        lut.hash(&mut h);
        format!("{filter:?}").hash(&mut h);
        let fingerprint = h.finish();
        Opts {
            palette,
            color,
            half,
            ansi256,
            lut,
            filter,
            cache_dir,
            fingerprint,
            gfx,
            verbose: 0,
            links: false,
            show_parent: false,
        }
    }
}

/// ガンマ（>1 で明るく）とコントラスト（>1 で強く、1.0 で変化なし）の補正テーブルを作る
fn build_lut(gamma: f32, contrast: f32) -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (i, o) in lut.iter_mut().enumerate() {
        let v = (i as f32 / 255.0).powf(1.0 / gamma);
        let v = (v - 0.5) * contrast + 0.5;
        *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    lut
}

#[derive(Clone, Copy, Debug)]
struct Grid {
    cols: u32,
    rows: u32,
}

fn parse_grid(s: &str) -> Result<Grid, String> {
    let err = || format!("`{s}` は COLSxROWS の形式（例: 3x2）で指定してください");
    let (c, r) = s.split_once(['x', 'X']).ok_or_else(err)?;
    let cols: u32 = c.parse().map_err(|_| err())?;
    let rows: u32 = r.parse().map_err(|_| err())?;
    if cols == 0 || rows == 0 {
        return Err(err());
    }
    Ok(Grid { cols, rows })
}

#[derive(Parser, Debug)]
#[command(
    name = "gls",
    version,
    about = "画像をターミナルに表示する `ls`。画像プロトコル（Sixel / Kitty / iTerm2）・ハーフブロック・アスキーアートで描画します。\n複数の画像（ワイルドカード・ディレクトリ可）を指定すると、ファイル名つきのサムネイル一覧で表示します。"
)]
struct Args {
    /// 入力画像ファイル・ディレクトリ・ワイルドカード（例: ./*）
    #[arg(required_unless_present = "clear_cache")]
    images: Vec<String>,

    /// ファイル名（ディレクトリ部分を除く）を正規表現で絞り込む。複数指定すると、どれかに一致すれば表示する。
    /// 例: -e "\.png$"  -e "(?i)^img_\d+"（(?i) で大文字小文字を無視）
    #[arg(short = 'e', long = "regexp", value_name = "REGEX")]
    regexp: Vec<String>,

    /// サブディレクトリの中の画像も探す（隠しディレクトリは除く）。ファイル名の前に、親ディレクトリ名も表示する
    #[arg(short = 'R', long)]
    recursive: bool,

    /// サブディレクトリに入る深さ（0: 直下のみ、1: 1階層下まで）。指定すると -R がなくても再帰する
    #[arg(long, value_name = "N")]
    max_depth: Option<usize>,

    /// 並び替えの基準。name は昇順、date / size / exif-date は新しい順・大きい順（-r で逆順）
    #[arg(long, value_enum, default_value_t = SortKey::Name)]
    sort: SortKey,

    /// 並び順を逆にする
    #[arg(short, long)]
    reverse: bool,

    /// 並び替えのあと、先頭から N 枚だけ表示する（例: --sort date -n 20 で新しい順に20枚）
    #[arg(short = 'n', long, value_name = "N")]
    head: Option<usize>,

    /// 出力の横幅（文字数）。単体表示ではその画像の幅、一覧表示では全体の幅。--size と同時には指定できない
    #[arg(short, long)]
    width: Option<u32>,

    /// 出力サイズのプリセット。デフォルトは s
    /// （単体: 24/40/80/120/160文字幅、一覧: サムネイル1枚が12/20/40/60/80文字幅。xs/s/m/l/xl の順）
    #[arg(short, long, value_enum, conflicts_with = "width")]
    size: Option<Size>,

    /// 文字セットプリセットの指定
    #[arg(short, long, value_enum, default_value_t = Palette::Standard)]
    palette: Palette,

    /// 任意の文字セットを指定（--palette より優先）
    #[arg(long)]
    custom_palette: Option<String>,

    /// 文字の明暗を反転
    #[arg(short, long)]
    invert: bool,

    /// ANSIカラー出力を無効化（モノクロ表示）
    #[arg(long)]
    no_color: bool,

    /// ファイル名にハイパーリンク（Ctrl+クリックで開ける）を付けない
    #[arg(long)]
    no_links: bool,

    /// 画面に収まらない出力でも一時停止（more 風）しない
    #[arg(long)]
    no_pager: bool,

    /// 描画キャッシュを使わない
    #[arg(long)]
    no_cache: bool,

    /// 描画キャッシュをすべて削除して終了
    #[arg(long)]
    clear_cache: bool,

    /// コントラスト（1.0 で変化なし、大きいほどメリハリが強く）
    #[arg(long, default_value_t = 1.0)]
    contrast: f32,

    /// ガンマ（1.0 で変化なし、1より大きいと明るく、小さいと暗く）
    #[arg(long, default_value_t = 1.0)]
    gamma: f32,

    /// 描画モード。デフォルトは image（端末の画像プロトコル）で、使えなければ端末の対応に合わせて
    /// half（TrueColor → 256色）、さらに text（モノクロ）へ自動でフォールバックする。
    /// half は --no-color と併用不可（併用時は text）。-p/--palette は text 時のみ有効
    #[arg(short, long, value_enum)]
    mode: Option<Mode>,

    /// 画像プロトコルの指定（--mode image のとき）。auto は環境変数から判定
    #[arg(long, value_enum, default_value_t = ProtoArg::Auto)]
    protocol: ProtoArg,

    /// 文字セル1つのピクセルサイズ（例: 10x20）。Sixel で画像と文字の位置がずれるときに調整する
    #[arg(long, value_parser = parse_cell_size)]
    cell_size: Option<(u32, u32)>,

    /// 一覧表示のグリッド（COLSxROWS、1ページあたり）。画像が多い場合は複数ページに分けて出力。
    /// 未指定なら、端末の横幅に収まるだけ列数を増やす（行数は2）。--width だけを指定したときは 3x2
    #[arg(short, long, value_parser = parse_grid)]
    grid: Option<Grid>,

    /// ファイル名の下に画像情報を表示する。-v: ピクセルの縦横とファイルサイズ、
    /// -vv: 形式・色・画素数・縦横比・更新日時・EXIF（カメラや撮影設定）も
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

/// ITU-R BT.601 に基づく輝度計算 (0 - 255)
fn calculate_brightness(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

/// 輝度値をパレット内の文字へマッピング（palette は反転済みのものを渡す）
fn get_mapped_char(brightness: u8, palette: &[char]) -> char {
    let index = (brightness as f32 / 255.0 * (palette.len() - 1) as f32) as usize;
    palette[index]
}

/// 縦横比（文字セルは約1:2）を保って width 文字幅に収めた (幅, 高さ) を返す。
/// max_h があれば高さがそれを超えないよう幅も縮める。
fn fit(orig_w: u32, orig_h: u32, width: u32, max_h: Option<u32>) -> (u32, u32) {
    let ratio = orig_h as f64 / orig_w as f64 * 0.5;
    let mut w = width.max(1);
    let mut h = ((w as f64 * ratio) as u32).max(1);
    if let Some(m) = max_h {
        if h > m {
            h = m;
            w = ((m as f64 / ratio) as u32).clamp(1, width);
        }
    }
    (w, h)
}

fn push_u8(s: &mut String, v: u8) {
    if v >= 100 {
        s.push((b'0' + v / 100) as char);
    }
    if v >= 10 {
        s.push((b'0' + (v / 10) % 10) as char);
    }
    s.push((b'0' + v % 10) as char);
}

/// RGB を xterm 256色パレットの番号に変換する（6x6x6 の色立方体 + 24段階のグレー）
fn to_ansi256(c: [u8; 3]) -> u8 {
    let [r, g, b] = c;
    if r.abs_diff(g) < 10 && g.abs_diff(b) < 10 {
        return match r {
            0..=7 => 16,
            249..=255 => 231,
            _ => 232 + ((r as u32 - 8) * 24 / 241) as u8,
        };
    }
    let q = |v: u8| ((v as u32 * 5 + 127) / 255) as u8;
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

/// 色指定のパラメータを追加する。kind は 38（前景）か 48（背景）。
/// ansi256 なら `38;5;N`、そうでなければ TrueColor の `38;2;R;G;B`。
fn push_col(s: &mut String, kind: u8, c: [u8; 3], ansi256: bool) {
    push_u8(s, kind);
    if ansi256 {
        s.push_str(";5;");
        push_u8(s, to_ansi256(c));
    } else {
        s.push_str(";2;");
        push_u8(s, c[0]);
        s.push(';');
        push_u8(s, c[1]);
        s.push(';');
        push_u8(s, c[2]);
    }
}

/// 画像を w×h 文字に描画し、1行ずつの文字列（ANSI付き）にする。
/// 直前と同じ色の指定は省いて出力量を減らす。
fn render_lines(img: &image::RgbImage, o: &Opts, w: u32, h: u32) -> Vec<String> {
    // half モードは1文字に上下2ピクセルを使うので縦に2倍の解像度で縮小する
    let py = if o.half { h * 2 } else { h };
    let img = image::imageops::resize(img, w, py, o.filter);
    let px = |x: u32, y: u32| img.get_pixel(x, y).0.map(|c| o.lut[c as usize]);
    let mut lines = Vec::with_capacity(h as usize);
    for y in 0..h {
        let mut line = String::with_capacity(w as usize * 8);
        let mut last_fg: Option<[u8; 3]> = None;
        let mut last_bg: Option<[u8; 3]> = None;
        for x in 0..w {
            if o.half {
                // 前景＝上のピクセル、背景＝下のピクセル
                let (fg, bg) = (px(x, y * 2), px(x, y * 2 + 1));
                let (fg_new, bg_new) = (last_fg != Some(fg), last_bg != Some(bg));
                if fg_new || bg_new {
                    line.push_str("\x1b[");
                    if fg_new {
                        push_col(&mut line, 38, fg, o.ansi256);
                    }
                    if fg_new && bg_new {
                        line.push(';');
                    }
                    if bg_new {
                        push_col(&mut line, 48, bg, o.ansi256);
                    }
                    line.push('m');
                    (last_fg, last_bg) = (Some(fg), Some(bg));
                }
                line.push('▀');
                continue;
            }
            let c = px(x, y);
            if o.color && last_fg != Some(c) {
                // 24-bit TrueColor 文字色シーケンス
                line.push_str("\x1b[");
                push_col(&mut line, 38, c, o.ansi256);
                line.push('m');
                last_fg = Some(c);
            }
            line.push(get_mapped_char(
                calculate_brightness(c[0], c[1], c[2]),
                &o.palette,
            ));
        }
        if o.color {
            line.push_str("\x1b[0m"); // 行末でリセット
        }
        lines.push(line);
    }
    lines
}

/// 画像を読み込み、表示に使う文字サイズ (w, h) を決める。
/// JPEG は DCT スケーリングで縮小しながらデコードして高速化する
/// （非対応の形式やエラー時は通常のデコードにフォールバック）。
fn load_fit(
    path: &Path,
    o: &Opts,
    width: u32,
    max_h: Option<u32>,
) -> Result<(image::RgbImage, u32, u32), String> {
    let is_jpeg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if is_jpeg {
        if let Some(r) = load_jpeg_scaled(path, o, width, max_h) {
            return Ok(r);
        }
    }
    let img = match ext::kind(path) {
        // SVG・動画・PDF・HEIC/AVIF は専用の取得方法を使う
        Some(kind) => {
            let longest = match max_h {
                Some(m) => (width.max(m * 2) * 2).clamp(128, 1024),
                None => (width * 4).clamp(256, 1024),
            };
            ext::load(path, kind, longest)?
        }
        None => orient::open(path)?,
    };
    let (ow, oh) = img.dimensions();
    let (w, h) = fit(ow, oh, width, max_h);
    Ok((img, w, h))
}

fn load_jpeg_scaled(
    path: &Path,
    o: &Opts,
    width: u32,
    max_h: Option<u32>,
) -> Option<(image::RgbImage, u32, u32)> {
    let mut d = jpeg_decoder::Decoder::new(BufReader::new(std::fs::File::open(path).ok()?));
    d.read_info().ok()?;
    let info = d.info()?;
    // EXIF の向き。90°/270° 回転なら、表示上の縦横は本体の縦横を入れ替えたもの
    let orientation = orient::exif_orientation(path);
    let swap = orient::swaps_axes(orientation);
    let (dw, dh) = if swap {
        (info.height, info.width)
    } else {
        (info.width, info.height)
    };
    let (w, h) = fit(dw as u32, dh as u32, width, max_h);
    let py = if o.half { h * 2 } else { h };
    // 本体（回転前）の向きで必要な大きさ
    let (rw, rh) = if swap { (py, w) } else { (w, py) };
    // 一覧（max_h あり）では、足りるなら EXIF サムネイルを使って本体の読み込みを省く
    if max_h.is_some() {
        if let Some(t) = thumb::lookup(path, info.width as u32, info.height as u32, rw, rh) {
            return Some((orient::apply(t, orientation), w, h));
        }
    }
    // 出力解像度以上を保てる最大の縮小率（1/1, 1/2, 1/4, 1/8）でデコードする
    let (sw, sh) = d
        .scale(
            rw.min(u16::MAX as u32) as u16,
            rh.min(u16::MAX as u32) as u16,
        )
        .ok()?;
    let pixels = d.decode().ok()?;
    let (sw, sh) = (sw as u32, sh as u32);
    let rgb = match d.info()?.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels,
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&v| [v, v, v]).collect(),
        _ => return None, // CMYK などは image クレートに任せる
    };
    let img = image::RgbImage::from_raw(sw, sh, rgb)?;
    Some((orient::apply(img, orientation), w, h))
}

fn is_image_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        || ext::kind(p).is_some()
}

/// dir の中の画像を out に集める。depth が 1 以上ならサブディレクトリにも入る（depth - 1 で再帰）。
/// 隠しディレクトリ（`.git` など）と、シンボリックリンクのディレクトリ（ループ防止）には入らない。
fn walk_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if depth > 0 && !hidden {
                // 読めないディレクトリ（権限なし等）は黙って飛ばす
                let _ = walk_dir(&path, depth - 1, out);
            }
        } else if is_image_path(&path) {
            // 画像の拡張子なら stat を省く（WSL の /mnt など遅いファイルシステムで効く）
            out.push(path);
        }
    }
    Ok(())
}

/// 引数（ファイル / ディレクトリ / ワイルドカード）を画像パスの一覧に展開する。
/// PowerShell や cmd はワイルドカードを展開しないため、ここで自前で展開する。
/// depth: ディレクトリの中に入る深さ（0 なら直下のファイルだけ）
fn collect_inputs(inputs: &[String], depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for arg in inputs {
        let path = Path::new(arg);
        if !path.exists() && arg.contains(['*', '?', '[']) {
            // シェルと同じく、`*` は先頭が `.` のファイル・ディレクトリに一致させない
            let opts = glob::MatchOptions {
                require_literal_leading_dot: true,
                ..Default::default()
            };
            match glob::glob_with(arg, opts) {
                Ok(paths) => {
                    for p in paths.flatten() {
                        if is_image_path(&p) {
                            out.push(p); // 画像の拡張子なら is_file() の stat を省く
                        } else if p.is_dir() {
                            // ワイルドカードがディレクトリに一致したとき、再帰指定があれば中も探す
                            if depth > 0 {
                                let _ = walk_dir(&p, depth - 1, &mut out);
                            }
                        } else if p.is_file() {
                            out.push(p);
                        }
                    }
                }
                Err(e) => die(&format!("ワイルドカードが不正です `{arg}`: {e}")),
            }
        } else if path.is_dir() {
            if let Err(e) = walk_dir(path, depth, &mut out) {
                die(&format!("ディレクトリを読めません `{arg}`: {e}"));
            }
        } else {
            out.push(path.to_path_buf());
        }
    }
    // 複数候補がある場合は画像以外（Cargo.toml 等）を黙って除外する
    if out.len() > 1 {
        out.retain(|p| is_image_path(p));
    }
    out
}
/// 数字の並びを数値として比べる自然順の比較（大文字小文字は区別しない）: img2 < img10
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let num = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (na, nb) = (num(&mut a), num(&mut b));
                // 桁数が多いほうが大きい。同じ桁数なら文字列比較で数値比較になる
                let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                a.next();
                b.next();
            }
        }
    }
}

fn file_name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 並び替え。date / size / exif-date はキーを並列に取得してから、大きい（新しい）順に並べる。
/// 同じキーのものは名前順。reverse なら全体を逆順にする。
fn sort_files(files: &mut Vec<PathBuf>, key: SortKey, reverse: bool) {
    let mtime = |p: &Path| -> i64 {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64)
    };
    // 名前順は「ディレクトリ → ファイル名」。再帰したときにディレクトリごとにまとまる
    let by_name = |a: &PathBuf, b: &PathBuf| {
        let dir = |p: &PathBuf| {
            p.parent()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        natural_cmp(&dir(a), &dir(b))
            .then_with(|| natural_cmp(&file_name_of(a), &file_name_of(b)))
            .then_with(|| a.cmp(b))
    };
    match key {
        SortKey::None => {}
        SortKey::Name => files.sort_by(by_name),
        SortKey::Date | SortKey::Size | SortKey::ExifDate => {
            let mut keyed: Vec<(i64, PathBuf)> = files
                .par_iter()
                .map(|p| {
                    let k = match key {
                        SortKey::Size => std::fs::metadata(p).map_or(0, |m| m.len() as i64),
                        SortKey::ExifDate => info::exif_timestamp(p).unwrap_or_else(|| mtime(p)),
                        _ => mtime(p),
                    };
                    (k, p.clone())
                })
                .collect();
            keyed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| by_name(&a.1, &b.1)));
            *files = keyed.into_iter().map(|(_, p)| p).collect();
        }
    }
    if reverse {
        files.reverse();
    }
}

fn terminal_width() -> u32 {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as u32)
        .unwrap_or(80)
}

/// ファイル名が n 桁（表示幅）に収まらないとき、拡張子を残して `foobar...png` のように省略する
fn shorten_name(name: &str, n: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if name.width() <= n {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i), // ext は先頭の "." を含む
        _ => (name, ""),
    };
    // 省略記号は ".." + ext（ext の "." と合わせて "..."）
    let keep = n.saturating_sub(ext.width() + 2);
    if ext.is_empty() || keep == 0 {
        return name.to_string(); // 後段の center_fit が単純に切り詰める
    }
    format!("{}..{}", center_fit(stem, keep).trim_end(), ext)
}

/// 表示幅（全角=2）で n 桁に切り詰め、中央寄せにするための (左の空白, 文字列, 右の空白) を返す
fn center_parts(s: &str, n: usize) -> (String, String, String) {
    use unicode_width::UnicodeWidthChar;
    let mut text = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > n {
            break;
        }
        text.push(c);
        used += cw;
    }
    let left = (n - used) / 2;
    (" ".repeat(left), text, " ".repeat(n - used - left))
}

/// 表示幅（全角=2）で n 桁に切り詰め、n 桁に中央寄せで空白埋めする
fn center_fit(s: &str, n: usize) -> String {
    let (l, t, r) = center_parts(s, n);
    format!("{l}{t}{r}")
}

/// 一覧の1行（cols 個以下のセル）を、表示する行リスト（画像/文字 + ラベル + 空行）にする。
/// 行内のセルは並列に読み込む。
fn render_row(paths: &[PathBuf], o: &Opts, cell_w: u32, cell_h: u32, gap: &str) -> Vec<String> {
    if let Some(g) = o.gfx {
        let imgs: Vec<Option<image::RgbImage>> = paths
            .par_iter()
            .map(|p| load_cell_image(p, o, g, cell_w, cell_h))
            .collect();
        return gfx_row(&imgs, paths, o, g, cell_w, cell_h, gap);
    }
    let cells: Vec<Vec<String>> = paths
        .par_iter()
        .map(|p| render_cell(p, o, cell_w, cell_h))
        .collect();
    // 各セルは cell_w 幅の行リスト（画像 + ラベル）。ラベルの行数はセルごとに違うことがあるので、
    // 足りない分は空白で埋める
    let blank = " ".repeat(cell_w as usize);
    let height = cells.iter().map(Vec::len).max().unwrap_or(0);
    let mut lines: Vec<String> = (0..height)
        .map(|n| {
            let parts: Vec<&str> = cells
                .iter()
                .map(|c| c.get(n).map_or(blank.as_str(), String::as_str))
                .collect();
            parts.join(gap).trim_end().to_string()
        })
        .collect();
    lines.push(String::new());
    lines
}

/// サムネイル一覧を別スレッドで描画し、できた行を順に受け取るレシーバを返す。
/// - 1行（cols 枚）ずつ並列に読み込み、先頭の行ができたらすぐ送る。
///   最初の画面が全部そろうのを待たないので、読み込みが遅くても早く表示が始まる
/// - 数行先まで先読みして、後続の行を裏で処理しておく（先読みの数は CPU のスレッド数に応じる）
/// - チャンネルが上限に達すると描画を止める。表示側が more で止まっている間も、読み過ぎはしない
/// - 受信側が閉じられたら（q で終了など）そこで描画を打ち切る
fn spawn_grid(
    files: Vec<PathBuf>,
    o: Opts,
    grid: Grid,
    total_width: u32,
) -> mpsc::Receiver<String> {
    use std::collections::VecDeque;
    use std::sync::Arc;

    let (tx, rx) = mpsc::sync_channel::<String>(256);
    std::thread::spawn(move || {
        let cols = grid.cols as usize;
        let gaps = GRID_GAP * (grid.cols - 1);
        let cell_w = (total_width.saturating_sub(gaps) / grid.cols).max(4);
        // 正方形に近い画像が収まる高さ。文字セルは縦長なので、幅 × (セルの縦横ピクセル比の逆数)
        // （ピクセルサイズ不明の文字描画では 1:2 とみなして幅の半分）
        let cell_h = match o.gfx {
            Some(g) => (cell_w * g.px_w / g.px_h).max(1),
            None => (cell_w / 2).max(1),
        };
        let gap = " ".repeat(GRID_GAP as usize);
        let per_page = cols * grid.rows as usize;
        let nrows = files.len().div_ceil(cols);
        let window = (rayon::current_num_threads() / cols).max(2) + 1;

        let files = Arc::new(files);
        let o = Arc::new(o);
        // i 行目の処理を裏で始め、結果を受け取るレシーバを返す。spawn_fifo なので投入順に着手される
        let spawn_row = |i: usize| {
            let (rtx, rrx) = mpsc::sync_channel::<Vec<String>>(1);
            let (files, o, gap) = (Arc::clone(&files), Arc::clone(&o), gap.clone());
            rayon::spawn_fifo(move || {
                let paths = &files[i * cols..((i + 1) * cols).min(files.len())];
                let _ = rtx.send(render_row(paths, &o, cell_w, cell_h, &gap));
            });
            rrx
        };

        let mut pending: VecDeque<mpsc::Receiver<Vec<String>>> = VecDeque::new();
        let mut next = 0;
        // 最初の1行は単独で処理する。読み込みが遅い環境（WSL の /mnt など）では、
        // 先読みと帯域を取り合って最初の表示が遅れるため。先頭の行を出してから先読みを広げる
        let mut ahead = 1;
        for i in 0..nrows {
            while next < nrows && pending.len() < ahead {
                pending.push_back(spawn_row(next));
                next += 1;
            }
            let Some(row) = pending.pop_front() else {
                return;
            };
            let Ok(lines) = row.recv() else { return };
            // ページ間は空行で区切る
            if i > 0 && (i * cols).is_multiple_of(per_page) && tx.send(String::new()).is_err() {
                return;
            }
            for line in lines {
                if tx.send(line).is_err() {
                    return;
                }
            }
            ahead = window;
        }
    });
    rx
}
/// ファイル名を表示幅 n 桁で折り返して最大 NAME_LINES 行にする。
/// それでも収まらないときは、最終行を拡張子を残して `...png` のように省略する。
fn wrap_name(name: &str, n: usize) -> Vec<String> {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if name.width() <= n {
        return vec![name.to_string()];
    }
    // 収まる行数に分けられるなら、行の長さをならして折り返す（拡張子だけの行を作らない）。
    // 収まらないなら、最後の行以外は n 桁いっぱいまで使う。
    let needed = name.width().div_ceil(n.max(1));
    let limit = if needed <= NAME_LINES {
        name.width().div_ceil(needed).max(2)
    } else {
        n
    };
    let mut lines: Vec<String> = Vec::new();
    let mut rest = name;
    while lines.len() + 1 < NAME_LINES && rest.width() > n {
        // limit 桁に収まる最長の先頭部分（全角は2桁）
        let mut used = 0;
        let mut end = 0;
        for (i, c) in rest.char_indices() {
            let cw = c.width().unwrap_or(0);
            if used + cw > limit {
                break;
            }
            used += cw;
            end = i + c.len_utf8();
        }
        if end == 0 {
            break; // 1文字も入らない（極端に狭いセル）
        }
        lines.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    lines.push(shorten_name(rest, n));
    lines
}

fn label_names(path: &Path, o: &Opts, cell_w: u32) -> Vec<String> {
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if o.show_parent {
        if let Some(parent) = path.parent().and_then(|p| p.file_name()) {
            name = format!("{}/{name}", parent.to_string_lossy());
        }
    }
    let url = if o.links { link::file_url(path) } else { None };
    wrap_name(&name, cell_w as usize)
        .iter()
        .map(|l| {
            let (left, text, right) = center_parts(l, cell_w as usize);
            // 余白はリンクに含めず、文字の部分だけを囲む
            match &url {
                Some(u) => format!("{left}{}{right}", link::wrap(&text, u)),
                None => format!("{left}{text}{right}"),
            }
        })
        .collect()
}

/// 単体表示の -v / -vv 用。`ファイル名  4000x3000 1.2MB` の行、-vv ではその下に詳細を1項目1行で並べる
fn single_info(path: &Path, o: &Opts) -> Vec<String> {
    if o.verbose == 0 {
        return Vec::new();
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut info = info::lines(path, o.verbose).into_iter();
    let first = info.next();
    let shown = match (o.links, link::file_url(path)) {
        (true, Some(u)) => link::wrap(&name, &u),
        _ => name,
    };
    let mut v = vec![match first {
        Some(f) => format!("{shown}  {f}"),
        None => shown,
    }];
    v.extend(info.map(|l| format!("  {l}")));
    v
}

/// セルのラベル行（ファイル名、-v / -vv なら続けて画像情報）。行数は画像ごとに違ってよい
fn label_lines(path: &Path, o: &Opts, cell_w: u32) -> Vec<String> {
    let mut v = label_names(path, o, cell_w);
    if o.verbose > 0 {
        v.extend(
            info::lines(path, o.verbose)
                .iter()
                .map(|l| center_fit(l, cell_w as usize)),
        );
    }
    v
}

/// 同じ行のセルでラベルの行数がそろうよう、短いものに空行を足す
fn pad_labels(labels: &mut [Vec<String>], cell_w: u32) {
    let max = labels.iter().map(Vec::len).max().unwrap_or(0);
    for l in labels {
        l.resize(max, " ".repeat(cell_w as usize));
    }
}

/// 画像を (bw × bh) ピクセルの箱に、縦横比を保って収まる大きさで読み込む。
/// JPEG は縮小しながらデコードする。
/// thumb_ok: 足りるなら JPEG の EXIF サムネイルを使う（一覧用。単体表示では画質のため使わない）
fn load_box(
    path: &Path,
    filter: FilterType,
    bw: u32,
    bh: u32,
    thumb_ok: bool,
) -> Result<image::RgbImage, String> {
    let fit_px = |ow: u32, oh: u32| {
        let s = (bw as f64 / ow as f64).min(bh as f64 / oh as f64);
        (
            ((ow as f64 * s).round() as u32).max(1),
            ((oh as f64 * s).round() as u32).max(1),
        )
    };
    let is_jpeg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    let scaled = is_jpeg
        .then(|| {
            let mut d = jpeg_decoder::Decoder::new(BufReader::new(std::fs::File::open(path).ok()?));
            d.read_info().ok()?;
            let info = d.info()?;
            let orientation = orient::exif_orientation(path);
            let swap = orient::swaps_axes(orientation);
            let (dw, dh) = if swap {
                (info.height, info.width)
            } else {
                (info.width, info.height)
            };
            let (tw, th) = fit_px(dw as u32, dh as u32);
            // 本体（回転前）の向きで必要な大きさ
            let (rw, rh) = if swap { (th, tw) } else { (tw, th) };
            if thumb_ok {
                if let Some(t) = thumb::lookup(path, info.width as u32, info.height as u32, rw, rh)
                {
                    return Some((orient::apply(t, orientation), tw, th));
                }
            }
            let (sw, sh) = d
                .scale(
                    rw.min(u16::MAX as u32) as u16,
                    rh.min(u16::MAX as u32) as u16,
                )
                .ok()?;
            let pixels = d.decode().ok()?;
            let rgb = match d.info()?.pixel_format {
                jpeg_decoder::PixelFormat::RGB24 => pixels,
                jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&v| [v, v, v]).collect(),
                _ => return None,
            };
            image::RgbImage::from_raw(sw as u32, sh as u32, rgb)
                .map(|i| (orient::apply(i, orientation), tw, th))
        })
        .flatten();
    let (img, tw, th) = match scaled {
        Some(r) => r,
        None => {
            let img = match ext::kind(path) {
                Some(kind) => ext::load(path, kind, bw.max(bh).clamp(128, 1600))?,
                None => orient::open(path)?,
            };
            let (tw, th) = fit_px(img.width(), img.height());
            (img, tw, th)
        }
    };
    Ok(if img.dimensions() == (tw, th) {
        img
    } else {
        image::imageops::resize(&img, tw, th, filter)
    })
}

/// 一覧の1セルぶんの画像を読み込む（失敗時は警告を出して None）
fn load_cell_image(
    path: &Path,
    o: &Opts,
    g: gfx::Gfx,
    cell_w: u32,
    cell_h: u32,
) -> Option<image::RgbImage> {
    let result = load_box(path, o.filter, cell_w * g.px_w, cell_h * g.px_h, true);
    DONE.fetch_add(1, Ordering::Relaxed);
    match result {
        Ok(img) => Some(img),
        Err(e) => {
            warn_load(path, &e);
            None
        }
    }
}

/// ガンマ・コントラスト補正をかけて RGBA キャンバスの (x0, y0) に貼り付ける
fn paste(
    canvas: &mut image::RgbaImage,
    img: &image::RgbImage,
    o_lut: &[u8; 256],
    x0: u32,
    y0: u32,
) {
    for (x, y, p) in img.enumerate_pixels() {
        let [r, g, b] = p.0.map(|c| o_lut[c as usize]);
        canvas.put_pixel(x0 + x, y0 + y, image::Rgba([r, g, b, 255]));
    }
}

/// 一覧の1行（cols 個以下のセル）を、画像1枚（キャンバス）にまとめて描画する行リストにする。
/// 戻り値は cell_h 行（先頭行に描画エスケープを含む）+ ラベル行 + 空行。
fn gfx_row(
    imgs: &[Option<image::RgbImage>],
    paths: &[PathBuf],
    o: &Opts,
    g: gfx::Gfx,
    cell_w: u32,
    cell_h: u32,
    gap: &str,
) -> Vec<String> {
    let n = imgs.len() as u32;
    let total_cols = n * cell_w + GRID_GAP * (n - 1);
    let mut canvas = image::RgbaImage::new(total_cols * g.px_w, cell_h * g.px_h);
    for (k, img) in imgs.iter().enumerate() {
        if let Some(img) = img {
            let x0 = k as u32 * (cell_w + GRID_GAP) * g.px_w + (cell_w * g.px_w - img.width()) / 2;
            paste(&mut canvas, img, &o.lut, x0, 0);
        }
    }
    let mut lines = vec![gfx::draw(&canvas, g.proto, total_cols, cell_h)];
    lines.extend((1..cell_h).map(|_| String::new()));
    let mut labels: Vec<Vec<String>> = paths.iter().map(|p| label_lines(p, o, cell_w)).collect();
    pad_labels(&mut labels, cell_w);
    for n in 0..labels[0].len() {
        let parts: Vec<&str> = labels.iter().map(|l| l[n].as_str()).collect();
        lines.push(parts.join(gap).trim_end().to_string());
    }
    lines.push(String::new());
    lines
}

enum PagerKey {
    Screen,
    Line,
    Quit,
}

/// more 風のプロンプトを出してキー入力を待つ
fn wait_key(out: &mut impl io::Write) -> PagerKey {
    use crossterm::event::{read, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    let _ = write!(
        out,
        "\x1b[7m--More-- (Space: 次の画面 / Enter: 1行 / q: 終了)\x1b[0m"
    );
    let _ = out.flush();
    if enable_raw_mode().is_err() {
        return PagerKey::Screen;
    }
    let key = loop {
        match read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Char('f') => {
                    break PagerKey::Screen
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Char('j') => break PagerKey::Line,
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break PagerKey::Quit,
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    break PagerKey::Quit
                }
                _ => {}
            },
            Ok(_) => {}
            Err(_) => break PagerKey::Quit,
        }
    };
    let _ = disable_raw_mode();
    let _ = write!(out, "\r\x1b[2K"); // プロンプト行を消す
    key
}

/// 行を標準出力へ出す。ページ表示が有効なら画面ごとに more 風に停止する。
/// `gls ... | head` などでパイプが閉じられても panic しない。
fn emit(lines: impl Iterator<Item = String>, pager: bool) {
    use io::IsTerminal;

    let stdout = io::stdout();
    let interactive = stdout.is_terminal() && io::stdin().is_terminal();
    let pager = pager && interactive;
    let screen = terminal_size::terminal_size()
        .map(|(_, h)| h.0 as usize)
        .unwrap_or(24)
        .saturating_sub(1)
        .max(1);

    let mut out = stdout.lock();
    let mut lines = lines.peekable();
    let mut budget = screen;
    while let Some(line) = lines.next() {
        if let Err(e) = writeln!(out, "{line}") {
            if e.kind() != io::ErrorKind::BrokenPipe {
                die(&format!("出力に失敗しました: {e}"));
            }
            return;
        }
        if pager {
            budget -= 1;
            if budget == 0 && lines.peek().is_some() {
                match wait_key(&mut out) {
                    PagerKey::Quit => return,
                    PagerKey::Line => budget = 1,
                    PagerKey::Screen => budget = screen,
                }
            }
        }
    }
    let _ = out.flush();
}

/// 描画キャッシュの保存先ディレクトリ（OS 標準のキャッシュ置き場）
fn cache_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    let dir = base.join("gls").join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// キャッシュの保存件数・期間の上限。環境変数 GLS_CACHE_MAX_MB（既定 256）、GLS_CACHE_DAYS（既定 30）で変えられる
fn cache_limits() -> (u64, std::time::Duration) {
    let env = |k: &str, default: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    (
        env("GLS_CACHE_MAX_MB", 256) * 1024 * 1024,
        std::time::Duration::from_secs(env("GLS_CACHE_DAYS", 30) * 24 * 3600),
    )
}

/// 古いキャッシュを整理する。
/// - 最後に使われてから GLS_CACHE_DAYS 日を過ぎたものを削除
/// - 合計が GLS_CACHE_MAX_MB を超えていたら、古いものから削除
/// - 書き込み途中で残った一時ファイルは、1日経ったら削除
///
/// 実行は1日1回まで（`.pruned` ファイルの更新日時で判定）。起動を遅らせないよう、呼び出し側でバックグラウンドで実行する。
fn prune_cache(dir: &Path) {
    use std::time::{Duration, SystemTime};
    let day = Duration::from_secs(24 * 3600);
    let now = SystemTime::now();
    let marker = dir.join(".pruned");
    let recently_pruned = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| now.duration_since(t).ok())
        .is_some_and(|age| age < day);
    if recently_pruned {
        return;
    }
    let (max_bytes, max_age) = cache_limits();
    prune_with(dir, max_bytes, max_age);
    let _ = std::fs::write(&marker, b"");
}

/// prune_cache の本体（上限を引数で受け取る）
fn prune_with(dir: &Path, max_bytes: u64, max_age: std::time::Duration) {
    use std::time::{Duration, SystemTime};
    let day = Duration::from_secs(24 * 3600);
    let now = SystemTime::now();

    // (パス, 最終使用日時, サイズ)
    let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if !(ext == "txt" || ext.starts_with("tmp")) {
                continue; // 自分が作ったファイルだけを対象にする
            }
            if let Ok(m) = e.metadata() {
                files.push((p, m.modified().unwrap_or(now), m.len()));
            }
        }
    }
    let age = |t: &SystemTime| now.duration_since(*t).unwrap_or_default();
    files.retain(|(p, t, _)| {
        let limit = if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.starts_with("tmp"))
        {
            day
        } else {
            max_age
        };
        if age(t) > limit {
            let _ = std::fs::remove_file(p);
            false
        } else {
            true
        }
    });
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    if total > max_bytes {
        files.sort_by_key(|f| f.1); // 古い順
        for (p, _, len) in &files {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(p).is_ok() {
                total -= len;
            }
        }
    }
}

/// キャッシュを使ったら更新日時を今にして、「最後に使われた日時」として整理の基準にする。
/// 書き込みを減らすため、1日以上前のときだけ更新する。
fn touch_cache(p: &Path) {
    use std::time::{Duration, SystemTime};
    let stale = std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age > Duration::from_secs(24 * 3600));
    if stale {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(p) {
            let _ = f.set_modified(SystemTime::now());
        }
    }
}

/// キャッシュを削除して、消した件数と容量を表示する
fn clear_cache() {
    let Some(dir) = cache_dir() else {
        println!("キャッシュはありません。");
        return;
    };
    let (mut count, mut bytes) = (0u64, 0u64);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            // 自分が作ったファイル（<hash>.txt と書き込み途中の一時ファイル）だけを対象にする
            let ours = p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "txt" || e.starts_with("tmp"));
            if !ours {
                continue;
            }
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&p).is_ok() {
                count += 1;
                bytes += len;
            }
        }
    }
    println!(
        "キャッシュを削除しました: {count} 件 ({:.1} MB) [{}]",
        bytes as f64 / 1_048_576.0,
        dir.display()
    );
}

/// キャッシュファイルのパス。画像のパス・更新日時・サイズ、セルの大きさ、描画設定で決まる
fn cache_path(path: &Path, o: &Opts, cell_w: u32, cell_h: u32) -> Option<PathBuf> {
    let dir = o.cache_dir.as_ref()?;
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let abs = std::fs::canonicalize(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        o.fingerprint,
        abs,
        mtime.as_nanos(),
        meta.len(),
        cell_w,
        cell_h,
    )
        .hash(&mut h);
    Some(dir.join(format!("{:016x}.txt", h.finish())))
}

/// 1枚分のセル。必ず cell_h + 1 行（画像 + ラベル）、各行の可視幅は cell_w
fn render_cell(path: &Path, o: &Opts, cell_w: u32, cell_h: u32) -> Vec<String> {
    let blank = " ".repeat(cell_w as usize);
    let cache = cache_path(path, o, cell_w, cell_h);

    let cached = cache
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.lines().map(String::from).collect::<Vec<_>>())
        .filter(|l| !l.is_empty() && l.len() <= cell_h as usize);

    let mut lines = match cached {
        Some(l) => {
            if let Some(p) = &cache {
                touch_cache(p);
            }
            l
        }
        None => match load_fit(path, o, cell_w, Some(cell_h)) {
            Ok((img, w, h)) => {
                let left = ((cell_w - w) / 2) as usize;
                let right = (cell_w - w) as usize - left;
                let lines: Vec<String> = render_lines(&img, o, w, h)
                    .into_iter()
                    .map(|l| format!("{}{}{}", " ".repeat(left), l, " ".repeat(right)))
                    .collect();
                if let Some(p) = &cache {
                    // 書き込み途中のファイルを読まないよう、一時ファイル経由で置き換える
                    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
                    if std::fs::write(&tmp, lines.join("\n")).is_ok() {
                        let _ = std::fs::rename(&tmp, p);
                    }
                }
                lines
            }
            Err(e) => {
                warn_load(path, &e);
                vec![center_fit("(読み込み失敗)", cell_w as usize)]
            }
        },
    };
    DONE.fetch_add(1, Ordering::Relaxed);
    lines.resize(cell_h as usize, blank);
    lines.extend(label_lines(path, o, cell_w));
    lines
}

/// 旧 Windows コンソール（conhost）で ANSI エスケープを有効化する。
/// Windows Terminal や他OSでは何もしない／不要。
#[cfg(windows)]
fn enable_ansi() {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        STD_OUTPUT_HANDLE,
    };
    // SAFETY: 標準出力ハンドルに対する Win32 コンソール API 呼び出しのみ。
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi() {}

/// 単体表示の行を作る（読み込みに失敗したらエラー終了）
fn single_lines(path: &Path, o: &Opts, width: u32) -> Vec<String> {
    let mut lines = if let Some(g) = o.gfx {
        let img = match load_box(path, o.filter, width * g.px_w, 100_000, false) {
            Ok(i) => i,
            Err(e) => die(&format!("画像の読み込みに失敗しました: {e}")),
        };
        let (cols, rows) = (img.width().div_ceil(g.px_w), img.height().div_ceil(g.px_h));
        let mut canvas = image::RgbaImage::new(cols * g.px_w, rows * g.px_h);
        paste(&mut canvas, &img, &o.lut, 0, 0);
        let mut lines = vec![gfx::draw(&canvas, g.proto, cols, rows)];
        lines.extend((1..rows).map(|_| String::new()));
        lines
    } else {
        let (img, w, h) = match load_fit(path, o, width, None) {
            Ok(r) => r,
            Err(e) => die(&format!("画像の読み込みに失敗しました: {e}")),
        };
        render_lines(&img, o, w, h)
    };
    lines.extend(single_info(path, o));
    lines
}

/// 読み込み済みの枚数（インジケータの進捗表示用）
static DONE: AtomicUsize = AtomicUsize::new(0);

/// 標準エラー出力が端末か
fn stderr_is_tty() -> bool {
    use io::IsTerminal;
    io::stderr().is_terminal()
}

/// 画像の読み込み失敗を警告する。外部ツールが無いための失敗は、同じ内容を1回だけ出す
fn warn_load(path: &Path, err: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    match err.strip_prefix(ext::MISSING_PREFIX) {
        Some(hint) => {
            let first = SEEN
                .get_or_init(Default::default)
                .lock()
                .is_ok_and(|mut s| s.insert(hint.to_string()));
            if first {
                warn(&format!("警告: {hint}"));
            }
        }
        None => warn(&format!("警告: {} を読み込めません: {err}", path.display())),
    }
}

/// 警告を出す。読み込み中インジケータの行を消してから出す。
fn warn(msg: &str) {
    if stderr_is_tty() {
        eprint!("\r\x1b[2K");
    }
    eprintln!("{msg}");
}

/// 行を受け取るイテレータ。次の行がなかなか来ないとき（150ms 以上）に、
/// 端末なら標準エラー出力に「⠋ 読み込み中 12/86」のインジケータを出す。行が届けば消す。
struct Progress {
    rx: mpsc::Receiver<String>,
    total: usize,
    enabled: bool,
    shown: bool,
    frame: usize,
}

impl Progress {
    fn new(rx: mpsc::Receiver<String>, total: usize) -> Self {
        Progress {
            rx,
            total,
            enabled: stderr_is_tty(),
            shown: false,
            frame: 0,
        }
    }

    fn clear(&mut self) {
        if self.shown {
            eprint!("\r\x1b[2K");
            let _ = io::stderr().flush();
            self.shown = false;
        }
    }
}

impl Iterator for Progress {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        use std::time::{Duration, Instant};
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let started = Instant::now();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(80)) {
                Ok(line) => {
                    self.clear();
                    return Some(line);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.clear();
                    return None;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.enabled && started.elapsed() >= Duration::from_millis(150) {
                        let f = FRAMES[self.frame % FRAMES.len()];
                        self.frame += 1;
                        if self.total > 0 {
                            let done = DONE.load(Ordering::Relaxed).min(self.total);
                            eprint!("\r\x1b[2K{f} 読み込み中 {done}/{}", self.total);
                        } else {
                            eprint!("\r\x1b[2K{f} 読み込み中...");
                        }
                        let _ = io::stderr().flush();
                        self.shown = true;
                    }
                }
            }
        }
    }
}

enum ColorDepth {
    TrueColor,
    Ansi256,
    None,
}

/// 端末の色の対応を環境変数から推定する。
/// 判定できない端末で色を落とすのは損なので、Windows と既知の端末は TrueColor とみなす。
fn color_depth() -> ColorDepth {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (term, colorterm) = (env("TERM"), env("COLORTERM"));
    let known = ["alacritty", "kitty", "wezterm", "foot", "direct", "ghostty"];
    if cfg!(windows)
        || colorterm == "truecolor"
        || colorterm == "24bit"
        || !env("WT_SESSION").is_empty()
        || !env("TERM_PROGRAM").is_empty()
        || known.iter().any(|k| term.contains(k))
    {
        ColorDepth::TrueColor
    } else if term.contains("256color") {
        ColorDepth::Ansi256
    } else {
        ColorDepth::None
    }
}

fn die(msg: &str) -> ! {
    if stderr_is_tty() {
        eprint!("\r\x1b[2K"); // 読み込み中インジケータの行を消す
    }
    eprintln!("エラー: {msg}");
    exit(1);
}

fn main() {
    let args = Args::parse();

    if args.clear_cache {
        clear_cache();
        return;
    }
    if args.gamma <= 0.0 || args.contrast < 0.0 {
        die("--gamma は0より大きく、--contrast は0以上で指定してください。");
    }
    if args.width == Some(0) {
        die("横幅は1以上を指定してください。");
    }

    let palette_src = args
        .custom_palette
        .as_deref()
        .unwrap_or_else(|| args.palette.chars());
    let mut palette: Vec<char> = palette_src.chars().collect();
    if palette.len() < 2 {
        die("パレットには少なくとも2文字以上指定してください。");
    }
    if args.invert {
        palette.reverse();
    }

    // サブディレクトリに入る深さ。-R は無制限、--max-depth でも指定できる
    let depth = match (args.recursive, args.max_depth) {
        (_, Some(d)) => d,
        (true, None) => usize::MAX,
        (false, None) => 0,
    };
    let mut files = collect_inputs(&args.images, depth);
    if files.is_empty() {
        die("画像ファイルが見つかりませんでした。");
    }
    if !args.regexp.is_empty() {
        let set = match regex::RegexSet::new(&args.regexp) {
            Ok(s) => s,
            Err(e) => die(&format!("正規表現が不正です: {e}")),
        };
        files.retain(|p| {
            p.file_name()
                .is_some_and(|n| set.is_match(&n.to_string_lossy()))
        });
        if files.is_empty() {
            die("正規表現に一致する画像がありません。");
        }
    }

    sort_files(&mut files, args.sort, args.reverse);
    if let Some(n) = args.head {
        files.truncate(n.max(1));
    }

    let mut color = !args.no_color;
    if color {
        enable_ansi();
    }
    // 画像モード（デフォルト）。画像プロトコルが使えなければ、端末の色対応に合わせてフォールバックする:
    //   image → half（TrueColor）→ half（256色）→ text（モノクロ）
    let image_requested = args.mode.is_none() || matches!(args.mode, Some(Mode::Image));
    let gfx = if image_requested && color {
        let proto = match args.protocol {
            ProtoArg::Auto => gfx::detect(),
            ProtoArg::Sixel => Some(gfx::Proto::Sixel),
            ProtoArg::Kitty => Some(gfx::Proto::Kitty),
            ProtoArg::Iterm2 => Some(gfx::Proto::Iterm2),
        };
        // 明示的に -m image を指定したのに使えないときだけ知らせる（デフォルト時は黙ってフォールバック）
        if proto.is_none() && args.mode.is_some() {
            eprintln!("警告: 画像プロトコル対応の端末を判定できませんでした。文字で表示します（--protocol sixel|kitty|iterm2 で指定可）。");
        }
        proto.map(|proto| {
            let (px_w, px_h) = args
                .cell_size
                .or_else(gfx::query_cell_px)
                .unwrap_or((10, 20));
            gfx::Gfx { proto, px_w, px_h }
        })
    } else {
        None
    };
    let mut half = matches!(args.mode, Some(Mode::Half)) && color;
    let mut ansi256 = false;
    if image_requested && gfx.is_none() && color {
        match color_depth() {
            ColorDepth::TrueColor => half = true,
            ColorDepth::Ansi256 => {
                half = true;
                ansi256 = true;
            }
            ColorDepth::None => color = false,
        }
    }
    // 一覧は速さ重視の縮小フィルタ、単体表示は品質重視
    let filter = if files.len() == 1 {
        FilterType::Lanczos3
    } else {
        FilterType::Triangle
    };
    let cache_dir = if args.no_cache { None } else { cache_dir() };
    if let Some(dir) = cache_dir.clone() {
        // 古いキャッシュの整理は、表示を遅らせないようバックグラウンドで（1日1回まで）
        std::thread::spawn(move || prune_cache(&dir));
    }
    let mut opts = Opts::new(
        palette,
        color,
        half,
        ansi256,
        build_lut(args.gamma, args.contrast),
        filter,
        cache_dir,
        gfx,
    );
    opts.verbose = args.verbose;
    opts.show_parent = depth > 0;
    // リンクの制御文字は、出力先が端末のときだけ付ける（リダイレクトやパイプには混ぜない）
    opts.links = !args.no_links && io::IsTerminal::is_terminal(&io::stdout());

    // サイズ未指定は s（標準）。--width を指定したときはそちらを優先する
    let size = args.size.unwrap_or(Size::S);
    let single_width = args.width.unwrap_or(size.single_width());

    let pager = !args.no_pager;
    if files.len() == 1 {
        // 単体表示（読み込みに時間がかかるときにインジケータを出せるよう、別スレッドで処理する）
        let path = files[0].clone();
        let (tx, rx) = mpsc::sync_channel::<String>(256);
        std::thread::spawn(move || {
            for line in single_lines(&path, &opts, single_width) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        emit(Progress::new(rx, 0), pager);
    } else {
        // サムネイル一覧。端末の右端ぴったりだと折り返す端末があるので1文字余らせる
        let term = terminal_width().saturating_sub(1);
        let cell = size.cell_width();
        let (grid, total) = match (args.grid, args.width) {
            (g, Some(w)) => (g.unwrap_or(DEFAULT_GRID), w),
            // サムネイル幅 × 列数 + 隙間。端末幅を超える場合は端末幅に収める
            (Some(g), None) => (g, (cell * g.cols + GRID_GAP * (g.cols - 1)).min(term)),
            // 列数の指定がなければ、端末の横幅に収まるだけサムネイルを並べる
            (None, None) => {
                let cols = ((term + GRID_GAP) / (cell + GRID_GAP)).max(1);
                (
                    Grid {
                        cols,
                        rows: DEFAULT_GRID.rows,
                    },
                    (cell * cols + GRID_GAP * (cols - 1)).min(term),
                )
            }
        };
        let count = files.len();
        emit(
            Progress::new(spawn_grid(files, opts, grid, total), count),
            pager,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi256_mapping() {
        assert_eq!(to_ansi256([0, 0, 0]), 16);
        assert_eq!(to_ansi256([255, 255, 255]), 231);
        assert_eq!(to_ansi256([255, 0, 0]), 196);
        assert_eq!(to_ansi256([0, 255, 0]), 46);
        assert_eq!(to_ansi256([0, 0, 255]), 21);
        assert!((232..=255).contains(&to_ansi256([128, 128, 128])));
    }

    #[test]
    fn push_col_formats() {
        let mut s = String::new();
        push_col(&mut s, 38, [1, 22, 255], false);
        assert_eq!(s, "38;2;1;22;255");
        let mut s = String::new();
        push_col(&mut s, 48, [255, 0, 0], true);
        assert_eq!(s, "48;5;196");
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn wrap_names() {
        assert_eq!(wrap_name("short.png", 20), vec!["short.png"]);
        // 2行に収まる
        assert_eq!(
            wrap_name("abcdefghijklmnopqrstuvwxy.png", 20),
            vec!["abcdefghijklmno", "pqrstuvwxy.png"]
        );
        // 3行に収まらない: 最終行は拡張子を残して省略
        let l = wrap_name(
            "0123456789012345678901234567890123456789012345678901234567890123.jpg",
            20,
        );
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], "01234567890123456789");
        assert!(l[2].ends_with(".jpg") && l[2].contains(".."), "{l:?}");
        // 全角は2桁
        let l = wrap_name("あいうえおかきくけこさしすせそ.png", 10);
        assert_eq!(l[0], "あいうえお");
        // 41桁・幅20: 3行にならして折り返し、拡張子は最終行に残る
        let l = wrap_name("Gemini_Generated_Image_hs65lkhs65lkhs65.png", 20);
        assert_eq!(l.len(), 3);
        assert!(l[2].ends_with(".png") && l[2].len() > 4, "{l:?}");
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn label_links_wrap_text_only() {
        let mut o = Opts::new(
            vec![' ', '#'],
            false,
            false,
            false,
            build_lut(1.0, 1.0),
            FilterType::Triangle,
            None,
            None,
        );
        o.links = true;
        let p = Path::new("sub dir/photo.jpg");
        let lines = label_names(p, &o, 20);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        // 余白（左右の空白）はリンクの外、文字だけがリンクの中
        assert!(l.starts_with("     \x1b]8;id="), "{l:?}"); // "photo.jpg" は9桁 → 左5・右6
        assert!(
            l.contains("sub%20dir/photo.jpg\x1b\\photo.jpg\x1b]8;;\x1b\\"),
            "{l:?}"
        );
        assert!(l.ends_with("\x1b]8;;\x1b\\      "), "{l:?}");
        // リンクなしなら制御文字を含まない
        o.links = false;
        assert!(!label_names(p, &o, 20)[0].contains('\x1b'));
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("img2.jpg", "img10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("IMG_002.jpg", "img_2.jpg"), Ordering::Equal);
        assert_eq!(natural_cmp("a.jpg", "B.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("a1b", "a1"), Ordering::Greater);
        let mut v = vec!["10.png", "9.png", "1.png", "100.png"]
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        sort_files(&mut v, SortKey::Name, false);
        assert_eq!(
            v.iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>(),
            ["1.png", "9.png", "10.png", "100.png"]
        );
        sort_files(&mut v, SortKey::Name, true);
        assert_eq!(v[0], PathBuf::from("100.png"));
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn make(dir: &Path, name: &str, bytes: usize, age_days: u64) {
        let p = dir.join(name);
        std::fs::write(&p, vec![b'x'; bytes]).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(age_days * 24 * 3600))
            .unwrap();
    }

    #[test]
    fn prune_by_age_size_and_tmp() {
        let dir = std::env::temp_dir().join(format!("gls-prune-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        make(&dir, "old.txt", 10, 40); // 30日より古い → 削除
        make(&dir, "a.txt", 100, 3);
        make(&dir, "b.txt", 100, 2);
        make(&dir, "c.txt", 100, 1);
        make(&dir, "stale.tmp123", 10, 2); // 1日より古い一時ファイル → 削除
        make(&dir, "fresh.tmp456", 10, 0); // 書き込み中かもしれない → 残す
        make(&dir, "keep.dat", 10, 90); // 自分のファイルではない → 触らない
                                        // 上限 250 バイト: 期限切れを消した後の合計 100*3 + 10(fresh tmp) = 310 → 古い a.txt を消して 210
        prune_with(&dir, 250, Duration::from_secs(30 * 24 * 3600));
        let exists = |n: &str| dir.join(n).exists();
        assert!(!exists("old.txt"));
        assert!(!exists("stale.tmp123"));
        assert!(exists("fresh.tmp456"));
        assert!(exists("keep.dat"));
        assert!(!exists("a.txt"), "最も古い a.txt から削除される");
        assert!(exists("b.txt") && exists("c.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
