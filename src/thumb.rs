//! JPEG に埋め込まれた EXIF サムネイルの利用。
//! 本体を全部読まずに、ファイル先頭の数十 KB だけで小さい画像を得られる。
//! 読み込みの遅いファイルシステム（WSL の /mnt など）で一覧表示を速くする。

use std::io::BufReader;
use std::path::Path;

/// EXIF サムネイルを取り出す。無い・壊れている場合は None
fn exif_thumbnail(path: &Path) -> Option<image::RgbImage> {
    let file = std::fs::File::open(path).ok()?;
    let ex = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    let get = |tag| ex.get_field(tag, exif::In::THUMBNAIL)?.value.get_uint(0);
    let off = get(exif::Tag::JPEGInterchangeFormat)? as usize;
    let len = get(exif::Tag::JPEGInterchangeFormatLength)? as usize;
    let data = ex.buf().get(off..off.checked_add(len)?)?;
    image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
        .ok()
        .map(|i| i.to_rgb8())
}

/// 縦横比が（本体と）ほぼ同じか。編集ソフトで本体だけ切り抜かれた画像のサムネイルは使わない
fn same_aspect(aw: u32, ah: u32, bw: u32, bh: u32) -> bool {
    let (a, b) = (aw as f64 / ah as f64, bw as f64 / bh as f64);
    (a - b).abs() / a < 0.02
}

/// 本体（orig_w × orig_h）を need_w × need_h ピクセルに縮小して表示したいとき、
/// EXIF サムネイルで足りるならそれを返す。解像度が足りない・縦横比が合わない場合は None。
pub fn lookup(
    path: &Path,
    orig_w: u32,
    orig_h: u32,
    need_w: u32,
    need_h: u32,
) -> Option<image::RgbImage> {
    let t = exif_thumbnail(path)?;
    let ok = t.width() >= need_w
        && t.height() >= need_h
        && same_aspect(t.width(), t.height(), orig_w, orig_h);
    ok.then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_tolerance() {
        assert!(same_aspect(160, 120, 4000, 3000));
        assert!(!same_aspect(160, 120, 6000, 4000)); // 4:3 と 3:2
        assert!(!same_aspect(160, 120, 3000, 4000)); // 向きが違う
    }
}
