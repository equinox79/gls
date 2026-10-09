//! `--json` の出力。必要なのは文字列のエスケープと、決まった形のオブジェクトだけなので、外部クレートは使わない

use std::path::Path;

use crate::ext::Kind;
use crate::info::Info;

/// JSON の文字列リテラルにする（`"` `\` と制御文字をエスケープ。それ以外の Unicode はそのまま）
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt_str(s: &Option<String>) -> String {
    s.as_deref().map_or("null".to_string(), quote)
}

fn opt_num<T: ToString>(n: Option<T>) -> String {
    n.map_or("null".to_string(), |v| v.to_string())
}

fn kind_name(path: &Path) -> &'static str {
    match crate::ext::kind(path) {
        None => "image",
        Some(Kind::Svg) => "svg",
        Some(Kind::Video) => "video",
        Some(Kind::Pdf) => "pdf",
        Some(Kind::Heif) => "heif",
    }
}

/// 1ファイルぶんの JSON オブジェクト（1行）。値がないものは `null`（項目は常に同じ）
pub fn record(path: &Path, i: &Info) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (w, h) = (i.dims.map(|d| d.0), i.dims.map(|d| d.1));
    let megapixels = i
        .dims
        .map(|(w, h)| format!("{:.2}", w as f64 * h as f64 / 1e6));
    let aspect = i.dims.map(|(w, h)| crate::info::aspect(w, h));
    let modified = i.modified.map(|t| t.to_rfc3339());
    let details: Vec<String> = i.extra.iter().map(|s| quote(s)).collect();
    let e = &i.exif;
    let has_exif = e.camera.is_some()
        || e.aperture.is_some()
        || e.exposure.is_some()
        || e.iso.is_some()
        || e.focal.is_some()
        || e.taken.is_some()
        || e.gps;
    let exif = if has_exif {
        format!(
            "{{\"camera\":{},\"aperture\":{},\"exposure\":{},\"iso\":{},\"focal_length\":{},\"taken\":{},\"gps\":{}}}",
            opt_str(&e.camera),
            opt_str(&e.aperture),
            opt_str(&e.exposure),
            opt_str(&e.iso),
            opt_str(&e.focal),
            opt_str(&e.taken),
            e.gps
        )
    } else {
        "null".to_string()
    };
    format!(
        "{{\"path\":{},\"name\":{},\"type\":{},\"bytes\":{},\"width\":{},\"height\":{},\"megapixels\":{},\"aspect\":{},\"format\":{},\"color\":{},\"modified\":{},\"details\":[{}],\"exif\":{}}}",
        quote(&path.to_string_lossy()),
        quote(&name),
        quote(kind_name(path)),
        opt_num(i.bytes),
        opt_num(w),
        opt_num(h),
        opt_num(megapixels),
        opt_str(&aspect),
        opt_str(&i.format),
        opt_str(&i.color),
        opt_str(&modified),
        details.join(","),
        exif
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(quote("line\nbreak\ttab"), "\"line\\nbreak\\ttab\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
        // 日本語などはそのまま
        assert_eq!(quote("写真 (1).jpg"), "\"写真 (1).jpg\"");
    }

    #[test]
    fn empty_info_is_all_null() {
        let s = record(Path::new("x/none.png"), &Info::default());
        assert!(
            s.contains("\"bytes\":null") && s.contains("\"width\":null"),
            "{s}"
        );
        assert!(
            s.contains("\"details\":[]") && s.contains("\"exif\":null"),
            "{s}"
        );
        assert!(
            s.contains("\"name\":\"none.png\"") && s.contains("\"type\":\"image\""),
            "{s}"
        );
    }
}
