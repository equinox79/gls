//! `image` クレートで読めない形式（SVG・動画・PDF・HEIC/AVIF）のサムネイル取得
//!
//! - SVG: resvg で直接ラスタライズ（外部ツール不要）
//! - 動画・PDF・HEIC/AVIF:
//!   - Windows: エクスプローラーと同じサムネイル（シェルのサムネイルプロバイダ）を使う
//!   - macOS: Quick Look（qlmanage）を使う
//!   - どの OS でも、外部ツール（ffmpeg / mutool / pdftoppm / ImageMagick）があれば使う

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::RgbImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Video,
    Pdf,
    Svg,
    Heif,
}

const VIDEO_EXTS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "wmv", "flv", "mpg", "mpeg", "3gp",
];
const HEIF_EXTS: &[&str] = &["heic", "heif", "avif"];

/// 必要なツールが見つからないときのエラー文の先頭（呼び出し側で1回だけ警告するための印）
pub const MISSING_PREFIX: &str = "\u{1}MISSING:";

pub fn kind(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let is = |list: &[&str]| list.contains(&ext.as_str());
    if is(VIDEO_EXTS) {
        Some(Kind::Video)
    } else if is(HEIF_EXTS) {
        Some(Kind::Heif)
    } else if ext == "pdf" {
        Some(Kind::Pdf)
    } else if ext == "svg" {
        Some(Kind::Svg)
    } else {
        None
    }
}

pub fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Video => "VIDEO",
        Kind::Pdf => "PDF",
        Kind::Svg => "SVG",
        Kind::Heif => "HEIF",
    }
}

// ---------------------------------------------------------------- SVG

fn parse_svg(path: &Path) -> Result<resvg::usvg::Tree, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).map_err(|e| e.to_string())
}

/// SVG の本来の大きさ（ピクセル）
pub fn svg_size(path: &Path) -> Option<(u32, u32)> {
    let s = parse_svg(path).ok()?.size();
    Some((
        (s.width().round() as u32).max(1),
        (s.height().round() as u32).max(1),
    ))
}

/// SVG を縦横比を保って、長辺が longest ピクセルになるようにラスタライズする（白背景）
pub fn render_svg(path: &Path, longest: u32) -> Result<RgbImage, String> {
    let tree = parse_svg(path)?;
    let size = tree.size();
    let scale = longest as f32 / size.width().max(size.height()).max(1.0);
    let (w, h) = (
        ((size.width() * scale).round() as u32).max(1),
        ((size.height() * scale).round() as u32).max(1),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("SVG のサイズが不正です")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // 背景を白で塗っているので、不透明（straight alpha と同じ）として RGB だけ取り出す
    let rgb: Vec<u8> = pixmap
        .data()
        .chunks(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    RgbImage::from_raw(w, h, rgb).ok_or_else(|| "SVG の変換に失敗しました".to_string())
}

// ---------------------------------------------------------------- 外部ツール

enum Run {
    /// 起動できなかった（ツールが無い）
    Missing,
    /// 起動はできたが、画像が得られなかった
    Failed,
    Image(RgbImage),
}

/// 標準出力に PNG を出すコマンドを実行する
fn run_png(prog: &str, args: &[String]) -> Run {
    let child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => return Run::Missing,
    };
    let mut buf = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut buf);
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    if !ok || buf.is_empty() {
        return Run::Failed;
    }
    match image::load_from_memory(&buf) {
        Ok(i) => Run::Image(i.to_rgb8()),
        Err(_) => Run::Failed,
    }
}

fn s(v: &str) -> String {
    v.to_string()
}

fn unique_temp(stem: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "gls-{}-{}-{stem}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// 外部ツールで1枚の画像（動画なら1コマ、PDF なら1ページ目）を取り出す
fn via_tools(path: &Path, kind: Kind, longest: u32) -> Result<RgbImage, String> {
    let p = path.to_string_lossy().into_owned();
    let n = longest.to_string();
    // 動画・HEIC 用: ffmpeg で1コマ（長辺を n ピクセルに縮小）
    let scale = format!("scale='if(gt(iw,ih),min(iw,{n}),-2)':'if(gt(iw,ih),-2,min(ih,{n}))'");
    let ffmpeg = |seek: Option<&str>| {
        let mut a = vec![s("-nostdin"), s("-v"), s("error")];
        if let Some(t) = seek {
            a.extend([s("-ss"), s(t)]);
        }
        a.extend([
            s("-i"),
            p.clone(),
            s("-frames:v"),
            s("1"),
            s("-vf"),
            scale.clone(),
        ]);
        a.extend([s("-f"), s("image2pipe"), s("-vcodec"), s("png"), s("-")]);
        a
    };
    let magick = |first_page: bool| {
        let src = if first_page {
            format!("{p}[0]")
        } else {
            p.clone()
        };
        let mut a = vec![];
        if kind == Kind::Pdf {
            a.extend([s("-density"), s("144")]);
        }
        a.extend([
            src,
            s("-auto-orient"),
            s("-background"),
            s("white"),
            s("-alpha"),
            s("remove"),
        ]);
        a.extend([s("-resize"), format!("{n}x{n}>"), s("png:-")]);
        a
    };

    let mut missing: Vec<&str> = Vec::new();
    let tried = |name: &'static str, r: Run, missing: &mut Vec<&str>| -> Option<RgbImage> {
        match r {
            Run::Image(i) => Some(i),
            Run::Missing => {
                missing.push(name);
                None
            }
            Run::Failed => None,
        }
    };

    let result = match kind {
        Kind::Video => {
            // 先頭ぴったりは黒コマのことがあるので 1 秒付近を狙い、短い動画なら先頭にする
            tried(
                "ffmpeg",
                run_png("ffmpeg", &ffmpeg(Some("1"))),
                &mut missing,
            )
            .or_else(|| tried("ffmpeg", run_png("ffmpeg", &ffmpeg(None)), &mut missing))
        }
        Kind::Heif => tried("magick", run_png("magick", &magick(true)), &mut missing)
            .or_else(|| tried("ffmpeg", run_png("ffmpeg", &ffmpeg(None)), &mut missing)),
        Kind::Pdf => {
            let mutool = vec![
                s("draw"),
                s("-q"),
                s("-F"),
                s("png"),
                s("-o"),
                s("-"),
                s("-w"),
                n.clone(),
                p.clone(),
                s("1"),
            ];
            tried("mutool", run_png("mutool", &mutool), &mut missing)
                .or_else(|| pdftoppm(&p, &n, &mut missing))
                .or_else(|| tried("magick", run_png("magick", &magick(true)), &mut missing))
        }
        Kind::Svg => None,
    };
    if let Some(img) = result {
        return Ok(img);
    }
    // 1つでも「起動はできたが失敗」したなら、ツールの有無ではなく変換の失敗
    let all_missing = match kind {
        Kind::Video => 1,
        Kind::Heif => 2,
        Kind::Pdf => 3,
        Kind::Svg => 0,
    };
    if missing.len() >= all_missing {
        Err(format!("{MISSING_PREFIX}{}", tool_hint(kind)))
    } else {
        Err("サムネイルを作れませんでした".to_string())
    }
}

/// pdftoppm は標準出力への PNG 出力が確実でないので、一時ファイル経由で受け取る
fn pdftoppm(p: &str, n: &str, missing: &mut Vec<&str>) -> Option<RgbImage> {
    let root = unique_temp("pdf");
    let out = root.with_extension("png");
    let status = Command::new("pdftoppm")
        .args([
            "-png",
            "-scale-to",
            n,
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            p,
        ])
        .arg(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let img = match status {
        Err(_) => {
            missing.push("pdftoppm");
            None
        }
        Ok(st) if st.success() => image::open(&out).ok().map(|i| i.to_rgb8()),
        Ok(_) => None,
    };
    let _ = std::fs::remove_file(&out);
    img
}

fn tool_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Video => "動画のサムネイルを作るには ffmpeg が必要です（インストールして PATH に通してください）",
        Kind::Pdf => "PDF のサムネイルを作るには mutool（MuPDF）か pdftoppm（poppler）か ImageMagick が必要です",
        Kind::Heif => "HEIC/AVIF のサムネイルを作るには ImageMagick（magick）か ffmpeg が必要です",
        Kind::Svg => "",
    }
}

// ---------------------------------------------------------------- OS のサムネイル

/// Windows: エクスプローラーが使うサムネイル（動画・PDF・HEIC など、OS が対応している形式すべて）
#[cfg(windows)]
fn os_thumbnail(path: &Path, longest: u32) -> Option<RgbImage> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_THUMBNAILONLY,
    };

    // SAFETY: COM / GDI の呼び出し。取得したハンドルは必ず解放する。
    unsafe {
        // スレッドごとに初期化が必要（既に初期化済みなら S_FALSE が返るだけ）
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let side = longest.clamp(32, 1024) as i32;
        // THUMBNAILONLY: サムネイルが無い形式で、汎用アイコンが返ってくるのを避ける
        let hbm = factory
            .GetImage(
                SIZE { cx: side, cy: side },
                SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY,
            )
            .ok()?;

        let mut bm = BITMAP::default();
        let obj = HGDIOBJ(hbm.0);
        let got = GetObjectW(
            obj,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if got == 0 || w <= 0 || h <= 0 {
            let _ = DeleteObject(obj);
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 負: 上から下の順
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            hbm,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        let _ = DeleteObject(obj);
        if lines == 0 {
            return None;
        }
        // BGRA（アルファは事前乗算のことがある）→ 白背景に合成した RGB
        let rgb: Vec<u8> = buf
            .chunks(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                let over = |c: u8| (c as u32 + (255 - a)).min(255) as u8; // 事前乗算 + 白背景
                [over(p[2]), over(p[1]), over(p[0])]
            })
            .collect();
        RgbImage::from_raw(w as u32, h as u32, rgb)
    }
}

/// macOS: Quick Look のサムネイル
#[cfg(target_os = "macos")]
fn os_thumbnail(path: &Path, longest: u32) -> Option<RgbImage> {
    let dir = unique_temp("ql");
    std::fs::create_dir_all(&dir).ok()?;
    let status = Command::new("qlmanage")
        .args(["-t", "-s", &longest.clamp(32, 1024).to_string(), "-o"])
        .arg(&dir)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    let out = dir.join(format!("{}.png", path.file_name()?.to_string_lossy()));
    let img = status
        .success()
        .then(|| image::open(&out).ok())
        .flatten()
        .map(|i| i.to_rgb8());
    let _ = std::fs::remove_dir_all(&dir);
    img
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_thumbnail(_path: &Path, _longest: u32) -> Option<RgbImage> {
    None
}

/// 動画・PDF・HEIC/AVIF の代表画像（動画は1コマ、PDF は1ページ目）を、長辺 longest ピクセル程度で取得する。
/// OS のサムネイル → 外部ツールの順に試す。
pub fn load(path: &Path, kind: Kind, longest: u32) -> Result<RgbImage, String> {
    match kind {
        Kind::Svg => render_svg(path, longest),
        _ => match os_thumbnail(path, longest) {
            Some(img) => Ok(img),
            None => via_tools(path, kind, longest),
        },
    }
}

// ---------------------------------------------------------------- 情報（-vv）

/// ffprobe があれば、動画の長さ・コーデック・解像度を返す。無ければ空
pub fn video_info(path: &Path) -> Vec<String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=codec_name,width,height:format=duration",
        ])
        .args(["-of", "default=nw=1"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(str::to_string)
    };
    let mut v = Vec::new();
    if let (Some(w), Some(h)) = (get("width="), get("height=")) {
        v.push(format!("{w}x{h}"));
    }
    if let Some(c) = get("codec_name=") {
        v.push(c);
    }
    if let Some(d) = get("duration=").and_then(|d| d.parse::<f64>().ok()) {
        let t = d.round() as u64;
        v.push(if t >= 3600 {
            format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
        } else {
            format!("{}:{:02}", t / 60, t % 60)
        });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind(Path::new("a.MP4")), Some(Kind::Video));
        assert_eq!(kind(Path::new("a.heic")), Some(Kind::Heif));
        assert_eq!(kind(Path::new("a.PDF")), Some(Kind::Pdf));
        assert_eq!(kind(Path::new("a.svg")), Some(Kind::Svg));
        assert_eq!(kind(Path::new("a.png")), None);
        assert_eq!(kind(Path::new("noext")), None);
    }

    #[test]
    fn svg_renders_with_white_background() {
        let dir = std::env::temp_dir().join(format!("gls-svg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.svg");
        std::fs::write(
            &p,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect x="0" y="0" width="20" height="20" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        assert_eq!(svg_size(&p), Some((40, 20)));
        let img = render_svg(&p, 80).unwrap();
        assert_eq!(img.dimensions(), (80, 40)); // 縦横比 2:1 を保つ
        assert_eq!(img.get_pixel(10, 20).0, [255, 0, 0]); // 左半分は赤
        assert_eq!(img.get_pixel(70, 20).0, [255, 255, 255]); // 右半分は白背景
        let _ = std::fs::remove_dir_all(&dir);
    }
}
