//! 画像の情報表示（-v / -vv / -l）

use std::io::BufReader;
use std::path::Path;

use image::ImageDecoder;

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes}B")
    } else {
        format!("{v:.1}{}", UNITS[u])
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// 縦横比。約分して小さい整数比（16:9 など）になればそれを、ならなければ `1.85:1` 形式にする
fn aspect(w: u32, h: u32) -> String {
    let g = gcd(w, h).max(1);
    let (a, b) = (w / g, h / g);
    if a <= 32 && b <= 32 {
        format!("{a}:{b}")
    } else {
        format!("{:.2}:1", w as f64 / h as f64)
    }
}

fn color_desc(ct: image::ColorType) -> String {
    let ch = ct.channel_count().max(1) as u16;
    let bits = ct.bits_per_pixel() / ch;
    let name = match ct.channel_count() {
        1 => "Gray",
        2 => "Gray+A",
        3 => "RGB",
        _ => "RGBA",
    };
    format!("{name} {bits}bit")
}

/// EXIF から取り出した撮影情報。無い項目は None
#[derive(Default, Clone)]
pub struct Exif {
    pub camera: Option<String>,
    /// 絞り（`f/2.8`）
    pub aperture: Option<String>,
    /// シャッター速度（`1/250s`）
    pub exposure: Option<String>,
    /// `ISO100`
    pub iso: Option<String>,
    /// 焦点距離（`35mm`）
    pub focal: Option<String>,
    /// 撮影日時（`2026-10-08 12:34:56`）
    pub taken: Option<String>,
    pub gps: bool,
}

/// 1つのファイルについて調べた情報
#[derive(Default)]
pub struct Info {
    pub dims: Option<(u32, u32)>,
    pub bytes: Option<u64>,
    /// 形式（`JPEG`、`PNG`、動画・PDF などは種類の名前）
    pub format: Option<String>,
    /// 色（`RGB 8bit`）
    pub color: Option<String>,
    /// 動画の長さ・コーデックなど
    pub extra: Vec<String>,
    pub modified: Option<chrono::DateTime<chrono::Local>>,
    pub exif: Exif,
}

/// ファイルの情報を集める。full が false なら、ヘッダだけ読む軽い項目（大きさとファイルサイズ）だけ
pub fn gather(path: &Path, full: bool) -> Info {
    let mut i = Info::default();
    // image クレートで読めない形式（SVG・動画・PDF・HEIC）は別の方法で調べる
    let kind = crate::ext::kind(path);
    i.dims = match kind {
        Some(crate::ext::Kind::Svg) => crate::ext::svg_size(path),
        Some(_) => None,
        None => image::image_dimensions(path).ok(),
    };
    let meta = std::fs::metadata(path).ok();
    i.bytes = meta.as_ref().map(|m| m.len());
    if !full {
        return i;
    }

    if let Some(k) = kind {
        i.format = Some(crate::ext::label(k).to_string());
        if k == crate::ext::Kind::Video {
            i.extra = crate::ext::video_info(path);
        }
    } else if let Ok(reader) = image::ImageReader::open(path).and_then(|r| r.with_guessed_format())
    {
        i.format = reader.format().map(|f| format!("{f:?}").to_uppercase());
        i.color = reader
            .into_decoder()
            .ok()
            .map(|d| color_desc(d.color_type()));
    }
    i.modified = meta.and_then(|m| m.modified().ok()).map(Into::into);
    i.exif = exif_info(path);
    i
}

/// 画像の情報を短い行のリストで返す。
/// level 1: `4000x3000 1.2MB`。level 2 以上: 形式・色・画素数・縦横比・更新日時・EXIF（撮影情報）も加える。
/// 取得できなかった項目は省く。ヘッダだけを読むので速い。
pub fn lines(path: &Path, level: u8) -> Vec<String> {
    let i = gather(path, level >= 2);
    let mut v = Vec::new();
    let size = i.bytes.map(human_size);
    match (i.dims, &size) {
        (Some((w, h)), Some(s)) => v.push(format!("{w}x{h} {s}")),
        (Some((w, h)), None) => v.push(format!("{w}x{h}")),
        (None, Some(s)) => v.push(s.clone()),
        _ => {}
    }
    if level < 2 {
        return v;
    }

    let head = [&i.format, &i.color]
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if !head.is_empty() {
        v.push(head);
    }
    v.extend(i.extra.iter().cloned());
    if let Some((w, h)) = i.dims {
        v.push(format!(
            "{:.1}MP {}",
            w as f64 * h as f64 / 1e6,
            aspect(w, h)
        ));
    }
    if let Some(t) = &i.modified {
        v.push(t!("info.modified", date = t.format("%Y-%m-%d")));
    }
    v.extend(exif_lines(&i.exif));
    v
}

/// `-l`（長い表示）の列。表のタイトルの言語キー（locales の `long.*`）と、右寄せにするか
pub const LONG_COLUMNS: [(&str, bool); 10] = [
    ("long.size", true),
    ("long.dims", true),
    ("long.mp", true),
    ("long.aspect", false),
    ("long.format", false),
    ("long.modified", false),
    ("long.taken", false),
    ("long.camera", false),
    ("long.details", false),
    ("long.gps", false),
];

/// `-l` の1行ぶんのセル（`LONG_COLUMNS` の順）。無い項目は空文字
pub fn long_cells(i: &Info) -> Vec<String> {
    let join = |items: &[&Option<String>]| {
        items
            .iter()
            .filter_map(|s| s.as_deref())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let e = &i.exif;
    // 写真は撮影設定、動画は長さやコーデックなど
    let details = if i.extra.is_empty() {
        join(&[&e.aperture, &e.exposure, &e.iso, &e.focal])
    } else {
        i.extra.join(" ")
    };
    vec![
        i.bytes.map(human_size).unwrap_or_default(),
        i.dims.map(|(w, h)| format!("{w}x{h}")).unwrap_or_default(),
        i.dims
            .map(|(w, h)| format!("{:.1}MP", w as f64 * h as f64 / 1e6))
            .unwrap_or_default(),
        i.dims.map(|(w, h)| aspect(w, h)).unwrap_or_default(),
        join(&[&i.format, &i.color]),
        i.modified
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default(),
        e.taken.clone().unwrap_or_default(),
        e.camera.clone().unwrap_or_default(),
        details,
        if e.gps {
            "GPS".to_string()
        } else {
            String::new()
        },
    ]
}

/// EXIF の撮影日時（なければ日時）を UNIX 秒で返す。並び替え用。タイムゾーンは不明なので UTC として扱う
pub fn exif_timestamp(path: &Path) -> Option<i64> {
    let file = std::fs::File::open(path).ok()?;
    let ex = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    [exif::Tag::DateTimeOriginal, exif::Tag::DateTime]
        .into_iter()
        .find_map(|tag| {
            let f = ex.get_field(tag, exif::In::PRIMARY)?;
            let s = f.display_value().to_string();
            chrono::NaiveDateTime::parse_from_str(s.trim_matches('"'), "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|d| d.and_utc().timestamp())
        })
}

/// EXIF（カメラ・撮影設定・撮影日時など）を読む。読めなければ空
fn exif_info(path: &Path) -> Exif {
    let Ok(file) = std::fs::File::open(path) else {
        return Exif::default();
    };
    let Ok(ex) = exif::Reader::new().read_from_container(&mut BufReader::new(file)) else {
        return Exif::default();
    };
    let get = |tag: exif::Tag| {
        ex.get_field(tag, exif::In::PRIMARY)
            .map(|f| {
                f.display_value()
                    .with_unit(&ex)
                    .to_string()
                    .trim_matches('"')
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty())
    };
    let (make, model) = (get(exif::Tag::Make), get(exif::Tag::Model));
    let camera = match (make, model) {
        (Some(mk), Some(md)) if md.starts_with(&mk) => Some(md),
        (Some(mk), Some(md)) => Some(format!("{mk} {md}")),
        (mk, md) => mk.or(md),
    };
    Exif {
        camera,
        aperture: get(exif::Tag::FNumber),
        exposure: get(exif::Tag::ExposureTime).map(|s| s.replace(' ', "")),
        iso: get(exif::Tag::PhotographicSensitivity).map(|s| format!("ISO{s}")),
        focal: get(exif::Tag::FocalLength).map(|s| s.replace(' ', "")),
        taken: get(exif::Tag::DateTimeOriginal),
        gps: ex
            .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
            .is_some(),
    }
}

/// -vv 用の行。狭いサムネイルでも切れないよう、絞り・シャッター速度 / ISO・焦点距離の2行に分ける
fn exif_lines(e: &Exif) -> Vec<String> {
    let mut v = Vec::new();
    v.extend(e.camera.clone());
    let join = |items: [&Option<String>; 2]| {
        items
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    };
    for line in [join([&e.aperture, &e.exposure]), join([&e.iso, &e.focal])] {
        if !line.is_empty() {
            v.push(line);
        }
    }
    if let Some(d) = &e.taken {
        v.push(t!("info.taken", date = d.get(..10).unwrap_or(d)));
    }
    if e.gps {
        v.push(t!("info.gps").to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratios() {
        assert_eq!(aspect(1920, 1080), "16:9");
        assert_eq!(aspect(4000, 3000), "4:3");
        assert_eq!(aspect(1000, 1000), "1:1");
        assert_eq!(aspect(1115, 600), "1.86:1");
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1536), "1.5KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0MB");
    }

    #[test]
    fn long_cells_for_a_photo_and_for_nothing() {
        let i = Info {
            dims: Some((4000, 3000)),
            bytes: Some(2 * 1024 * 1024),
            format: Some("JPEG".into()),
            color: Some("RGB 8bit".into()),
            exif: Exif {
                camera: Some("Canon EOS".into()),
                aperture: Some("f/2.8".into()),
                exposure: Some("1/250s".into()),
                iso: Some("ISO100".into()),
                focal: Some("35mm".into()),
                taken: Some("2026-10-08 12:34:56".into()),
                gps: true,
            },
            ..Default::default()
        };
        let c = long_cells(&i);
        assert_eq!(c.len(), LONG_COLUMNS.len());
        assert_eq!(c[0], "2.0MB");
        assert_eq!(c[1], "4000x3000");
        assert_eq!(c[2], "12.0MP");
        assert_eq!(c[3], "4:3");
        assert_eq!(c[4], "JPEG RGB 8bit");
        assert_eq!(c[6], "2026-10-08 12:34:56");
        assert_eq!(c[7], "Canon EOS");
        assert_eq!(c[8], "f/2.8 1/250s ISO100 35mm");
        assert_eq!(c[9], "GPS");
        assert!(long_cells(&Info::default()).iter().all(String::is_empty));
    }

    #[test]
    fn every_long_column_has_a_title() {
        for (key, _) in LONG_COLUMNS {
            assert!(crate::i18n::find(key).is_some(), "{key}");
        }
        assert!(crate::i18n::find("long.name").is_some());
    }
}
