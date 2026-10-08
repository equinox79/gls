//! 画像の情報表示（-v / -vv）

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

/// 画像の情報を短い行のリストで返す。
/// level 1: `4000x3000 1.2MB`。level 2 以上: 形式・色・画素数・縦横比・更新日時・EXIF（撮影情報）も加える。
/// 取得できなかった項目は省く。ヘッダだけを読むので速い。
pub fn lines(path: &Path, level: u8) -> Vec<String> {
    let mut v = Vec::new();
    // image クレートで読めない形式（SVG・動画・PDF・HEIC）は別の方法で調べる
    let kind = crate::ext::kind(path);
    let dims = match kind {
        Some(crate::ext::Kind::Svg) => crate::ext::svg_size(path),
        Some(_) => None,
        None => image::image_dimensions(path).ok(),
    };
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| human_size(m.len()));
    match (dims, &size) {
        (Some((w, h)), Some(s)) => v.push(format!("{w}x{h} {s}")),
        (Some((w, h)), None) => v.push(format!("{w}x{h}")),
        (None, Some(s)) => v.push(s.clone()),
        _ => {}
    }
    if level < 2 {
        return v;
    }

    // 形式・色
    if let Some(k) = kind {
        v.push(crate::ext::label(k).to_string());
        if k == crate::ext::Kind::Video {
            v.extend(crate::ext::video_info(path));
        }
    } else if let Ok(reader) = image::ImageReader::open(path).and_then(|r| r.with_guessed_format())
    {
        let fmt = reader.format().map(|f| format!("{f:?}").to_uppercase());
        let color = reader
            .into_decoder()
            .ok()
            .map(|d| color_desc(d.color_type()));
        let head = [fmt, color]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        if !head.is_empty() {
            v.push(head);
        }
    }
    if let Some((w, h)) = dims {
        v.push(format!(
            "{:.1}MP {}",
            w as f64 * h as f64 / 1e6,
            aspect(w, h)
        ));
    }
    if let Some(t) = meta.and_then(|m| m.modified().ok()) {
        let t: chrono::DateTime<chrono::Local> = t.into();
        v.push(format!("更新 {}", t.format("%Y-%m-%d")));
    }
    v.extend(exif_lines(path));
    v
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

/// EXIF（カメラ・撮影設定・撮影日時など）。無ければ空
fn exif_lines(path: &Path) -> Vec<String> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(ex) = exif::Reader::new().read_from_container(&mut BufReader::new(file)) else {
        return Vec::new();
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

    let mut v = Vec::new();
    let (make, model) = (get(exif::Tag::Make), get(exif::Tag::Model));
    let camera = match (make, model) {
        (Some(mk), Some(md)) if md.starts_with(&mk) => Some(md),
        (Some(mk), Some(md)) => Some(format!("{mk} {md}")),
        (mk, md) => mk.or(md),
    };
    v.extend(camera);

    // 狭いサムネイルでも切れないよう、絞り・シャッター速度 / ISO・焦点距離の2行に分ける
    let join =
        |items: [Option<String>; 2]| items.into_iter().flatten().collect::<Vec<_>>().join(" ");
    for line in [
        join([
            get(exif::Tag::FNumber),
            get(exif::Tag::ExposureTime).map(|s| s.replace(' ', "")),
        ]),
        join([
            get(exif::Tag::PhotographicSensitivity).map(|s| format!("ISO{s}")),
            get(exif::Tag::FocalLength).map(|s| s.replace(' ', "")),
        ]),
    ] {
        if !line.is_empty() {
            v.push(line);
        }
    }

    if let Some(d) = get(exif::Tag::DateTimeOriginal) {
        v.push(format!("撮影 {}", d.get(..10).unwrap_or(&d)));
    }

    let mut flags = Vec::new();
    if ex
        .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
        .is_some()
    {
        flags.push("GPSあり".to_string());
    }
    if !flags.is_empty() {
        v.push(flags.join(" "));
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
}
