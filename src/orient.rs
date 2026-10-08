//! EXIF の向き（Orientation）の反映。スマホで縦に撮った写真などを正しい向きで表示する

use std::io::BufReader;
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, RgbImage};

/// JPEG などの EXIF から向きを読む（無い・読めない場合は補正なし）。ヘッダだけ読むので速い
pub fn exif_orientation(path: &Path) -> Orientation {
    let read = || -> Option<Orientation> {
        let file = std::fs::File::open(path).ok()?;
        let ex = exif::Reader::new()
            .read_from_container(&mut BufReader::new(file))
            .ok()?;
        let n = ex
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
            .value
            .get_uint(0)?;
        Orientation::from_exif(u8::try_from(n).ok()?)
    };
    read().unwrap_or(Orientation::NoTransforms)
}

/// 90°/270° 回転を含む向きか（縦横が入れ替わる）
pub fn swaps_axes(o: Orientation) -> bool {
    matches!(
        o,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

pub fn apply(img: RgbImage, o: Orientation) -> RgbImage {
    if o == Orientation::NoTransforms {
        return img;
    }
    let mut d = DynamicImage::ImageRgb8(img);
    d.apply_orientation(o);
    d.to_rgb8()
}

/// 画像を開いて、デコーダが持つ向き情報（JPEG・PNG・WebP・TIFF の EXIF）を反映した RGB にする
pub fn open(path: &Path) -> Result<RgbImage, String> {
    let reader = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let o = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(o);
    Ok(img.to_rgb8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn rotate90_swaps_dimensions_and_moves_pixels() {
        // 2x1: 左が赤、右が青
        let mut img = RgbImage::new(2, 1);
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        img.put_pixel(1, 0, Rgb([0, 0, 255]));
        assert!(swaps_axes(Orientation::Rotate90));
        assert!(!swaps_axes(Orientation::Rotate180));
        // EXIF 6 = 時計回りに 90° 回転して表示 → 1x2、上が赤・下が青
        let o = Orientation::from_exif(6).unwrap();
        let r = apply(img, o);
        assert_eq!(r.dimensions(), (1, 2));
        assert_eq!(r.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(r.get_pixel(0, 1).0, [0, 0, 255]);
    }
}
