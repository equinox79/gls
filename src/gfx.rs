//! 端末の画像表示プロトコル（Sixel / Kitty / iTerm2）への出力

use std::io::Cursor;

use base64::Engine as _;
use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Sixel,
    Kitty,
    Iterm2,
}

/// 画像出力の設定（プロトコルと、文字セル1つぶんのピクセルサイズ）
#[derive(Clone, Copy, Debug)]
pub struct Gfx {
    pub proto: Proto,
    pub px_w: u32,
    pub px_h: u32,
}

/// 環境変数から端末の対応プロトコルを推定する
pub fn detect() -> Option<Proto> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (term, prog) = (env("TERM"), env("TERM_PROGRAM"));
    if !env("KITTY_WINDOW_ID").is_empty() || term == "xterm-kitty" || prog == "ghostty" {
        Some(Proto::Kitty)
    } else if prog == "iTerm.app" || prog == "WezTerm" {
        Some(Proto::Iterm2)
    } else if !env("WT_SESSION").is_empty() // Windows Terminal（WSL 内でも引き継がれる）。Sixel は 1.22 以降
        || term.contains("sixel")
        || term == "foot"
        || term == "mlterm"
    {
        Some(Proto::Sixel)
    } else {
        None
    }
}

/// 端末から文字セルのピクセルサイズを取得する（取れなければ None）
pub fn query_cell_px() -> Option<(u32, u32)> {
    let ws = crossterm::terminal::window_size().ok()?;
    if ws.width == 0 || ws.height == 0 || ws.columns == 0 || ws.rows == 0 {
        return None;
    }
    Some((
        ws.width as u32 / ws.columns as u32,
        ws.height as u32 / ws.rows as u32,
    ))
}

/// canvas（透明部分は何も描かない）を、現在のカーソル位置に cols×rows 文字ぶんの領域として描画する
/// 文字列にする。出力後のカーソル位置は呼び出し前と同じ（描画位置の確保・復元込み）。
///
/// 手順: 先に rows 回改行して領域を確保（画面末尾ならここでスクロールする）→ rows 行戻る →
/// カーソルを保存して描画 → 保存位置へ復帰。描画中にスクロールが起きないので位置がずれない。
pub fn draw(canvas: &RgbaImage, proto: Proto, cols: u32, rows: u32) -> String {
    let body = match proto {
        Proto::Sixel => sixel(canvas),
        Proto::Kitty => kitty(canvas, cols, rows),
        Proto::Iterm2 => iterm2(canvas, cols, rows),
    };
    format!(
        "{}\x1b[{rows}A\r\x1b7{body}\x1b8",
        "\n".repeat(rows as usize)
    )
}

fn png_base64(canvas: &RgbaImage) -> (String, usize) {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(canvas.clone())
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("PNG encoding to memory cannot fail");
    (
        base64::engine::general_purpose::STANDARD.encode(&png),
        png.len(),
    )
}

fn kitty(canvas: &RgbaImage, cols: u32, rows: u32) -> String {
    let (b64, _) = png_base64(canvas);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(4096).collect();
    let mut out = String::new();
    for (i, c) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        let chunk = std::str::from_utf8(c).unwrap_or_default();
        if i == 0 {
            // a=T: 転送して表示, f=100: PNG, c/r: 表示するセル数, C=1: カーソルを動かさない
            out.push_str(&format!(
                "\x1b_Ga=T,f=100,c={cols},r={rows},C=1,q=2,m={more};{chunk}\x1b\\"
            ));
        } else {
            out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
        }
    }
    out
}

fn iterm2(canvas: &RgbaImage, cols: u32, rows: u32) -> String {
    let (b64, len) = png_base64(canvas);
    format!(
        "\x1b]1337;File=inline=1;size={len};width={cols};height={rows};preserveAspectRatio=0:{b64}\x07"
    )
}

const NONE: u16 = u16::MAX;

/// Sixel 文字列を作る。256色に減色（NeuQuant）し、透明ピクセル（alpha=0）は描かない。
fn sixel(canvas: &RgbaImage) -> String {
    let (w, h) = canvas.dimensions();
    let opaque: Vec<u8> = canvas
        .pixels()
        .filter(|p| p.0[3] > 0)
        .flat_map(|p| [p.0[0], p.0[1], p.0[2], 255])
        .collect();
    if opaque.is_empty() {
        return String::new();
    }
    let nq = color_quant::NeuQuant::new(10, 256, &opaque);
    let palette = nq.color_map_rgb();
    let ncolors = palette.len() / 3;

    let indices: Vec<u16> = canvas
        .pixels()
        .map(|p| {
            if p.0[3] == 0 {
                NONE
            } else {
                nq.index_of(&[p.0[0], p.0[1], p.0[2], 255]) as u16
            }
        })
        .collect();

    // DCS P2=1: 値0のピクセルは背景を残す。"1;1;W;H: ラスター属性（1:1, 画像サイズ）
    let mut out = format!("\x1bP0;1;0q\"1;1;{w};{h}");
    for i in 0..ncolors {
        let (r, g, b) = (
            palette[i * 3] as u32,
            palette[i * 3 + 1] as u32,
            palette[i * 3 + 2] as u32,
        );
        out.push_str(&format!(
            "#{i};2;{};{};{}",
            r * 100 / 255,
            g * 100 / 255,
            b * 100 / 255
        ));
    }

    let mut bands: Vec<Option<Vec<u8>>> = vec![None; ncolors];
    for y0 in (0..h).step_by(6) {
        let mut used: Vec<usize> = Vec::new();
        for dy in 0..6.min(h - y0) {
            let row = &indices[((y0 + dy) * w) as usize..((y0 + dy + 1) * w) as usize];
            for (x, &idx) in row.iter().enumerate() {
                if idx == NONE {
                    continue;
                }
                let bits = bands[idx as usize].get_or_insert_with(|| {
                    used.push(idx as usize);
                    vec![0u8; w as usize]
                });
                bits[x] |= 1 << dy;
            }
        }
        used.sort_unstable();
        for &c in &used {
            let bits = bands[c].take().unwrap_or_default();
            out.push_str(&format!("#{c}"));
            push_rle(&mut out, &bits);
            out.push('$'); // 行頭へ戻って次の色を重ねる
        }
        out.push('-'); // 次のバンド（6ピクセル下）へ
    }
    out.push_str("\x1b\\");
    out
}

/// sixel のデータ文字（bits + 63）を連長圧縮（!n）して追加する
fn push_rle(out: &mut String, bits: &[u8]) {
    let mut i = 0;
    while i < bits.len() {
        let b = bits[i];
        let mut n = 1;
        while i + n < bits.len() && bits[i + n] == b {
            n += 1;
        }
        let ch = (b + 63) as char;
        if n > 3 {
            out.push_str(&format!("!{n}{ch}"));
        } else {
            for _ in 0..n {
                out.push(ch);
            }
        }
        i += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// 最小限の Sixel デコーダ（エンコーダの検証用）。(幅, 高さ, RGBA) を返す
    fn decode(s: &str) -> (u32, u32, Vec<[u8; 4]>) {
        let q = s.find('q').unwrap();
        let body = s[q + 1..].trim_end_matches("\x1b\\");
        let b = body.as_bytes();
        let (mut w, mut h) = (0usize, 0usize);
        let mut pal: Vec<[u8; 3]> = vec![[0; 3]; 256];
        let mut cur = 0usize;
        let (mut x, mut y) = (0usize, 0usize);
        let mut px: Vec<[u8; 4]> = Vec::new();
        let num = |b: &[u8], i: &mut usize| -> usize {
            let st = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            std::str::from_utf8(&b[st..*i])
                .unwrap()
                .parse()
                .unwrap_or(0)
        };
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'"' => {
                    i += 1;
                    let _ = num(b, &mut i);
                    i += 1;
                    let _ = num(b, &mut i);
                    i += 1;
                    w = num(b, &mut i);
                    i += 1;
                    h = num(b, &mut i);
                    px = vec![[0; 4]; w * h];
                }
                b'#' => {
                    i += 1;
                    cur = num(b, &mut i);
                    if i < b.len() && b[i] == b';' {
                        i += 1;
                        let _ = num(b, &mut i); // 2 = RGB
                        let mut c = [0u8; 3];
                        for v in c.iter_mut() {
                            i += 1;
                            *v = (num(b, &mut i) * 255 / 100) as u8;
                        }
                        pal[cur] = c;
                    }
                }
                b'$' => {
                    x = 0;
                    i += 1;
                }
                b'-' => {
                    x = 0;
                    y += 6;
                    i += 1;
                }
                b'!' => {
                    i += 1;
                    let n = num(b, &mut i);
                    let bits = b[i] - 63;
                    for _ in 0..n {
                        put(&mut px, w, h, x, y, bits, pal[cur]);
                        x += 1;
                    }
                    i += 1;
                }
                c @ 63..=126 => {
                    put(&mut px, w, h, x, y, c - 63, pal[cur]);
                    x += 1;
                    i += 1;
                }
                _ => i += 1,
            }
        }
        (w as u32, h as u32, px)
    }

    fn put(px: &mut [[u8; 4]], w: usize, h: usize, x: usize, y: usize, bits: u8, c: [u8; 3]) {
        for dy in 0..6 {
            if bits & (1 << dy) != 0 && y + dy < h && x < w {
                px[(y + dy) * w + x] = [c[0], c[1], c[2], 255];
            }
        }
    }

    #[test]
    fn sixel_round_trip() {
        // 左半分: 赤〜青のグラデーション、右半分: 透明。高さ13（6の倍数でない）
        let (w, h) = (20u32, 13u32);
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..10 {
                img.put_pixel(x, y, Rgba([(255 - x * 25) as u8, 0, (x * 25) as u8, 255]));
            }
        }
        let (dw, dh, px) = decode(&sixel(&img));
        assert_eq!((dw, dh), (w, h));
        for y in 0..h {
            for x in 0..w {
                let got = px[(y * w + x) as usize];
                let want = img.get_pixel(x, y).0;
                if want[3] == 0 {
                    assert_eq!(got[3], 0, "({x},{y}) は透明のまま");
                } else {
                    assert_eq!(got[3], 255, "({x},{y}) は描画される");
                    for k in 0..3 {
                        assert!(
                            (got[k] as i32 - want[k] as i32).abs() < 30,
                            "({x},{y}) 色が近い: {got:?} vs {want:?}"
                        );
                    }
                }
            }
        }
    }

    /// 目視確認用: GLS_SIXEL_IN の出力から最初の Sixel を取り出し、GLS_PNG_OUT に PNG で保存する
    #[test]
    #[ignore]
    fn dump_png() {
        let (Ok(inp), Ok(out)) = (std::env::var("GLS_SIXEL_IN"), std::env::var("GLS_PNG_OUT"))
        else {
            return;
        };
        let text = std::fs::read_to_string(inp).unwrap();
        let start = text.find("\x1bP").unwrap();
        let end = start + text[start..].find("\x1b\\").unwrap() + 2;
        let (w, h, px) = decode(&text[start..end]);
        let mut img = RgbaImage::from_pixel(w, h, Rgba([30, 30, 30, 255]));
        for (i, p) in px.iter().enumerate() {
            if p[3] > 0 {
                img.put_pixel(i as u32 % w, i as u32 / w, Rgba(*p));
            }
        }
        img.save(out).unwrap();
    }

    #[test]
    fn draw_wraps_reserve_and_restore() {
        let img = RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255]));
        for p in [Proto::Sixel, Proto::Kitty, Proto::Iterm2] {
            let s = draw(&img, p, 2, 3);
            assert!(s.starts_with("\n\n\n\x1b[3A\r\x1b7"));
            assert!(s.ends_with("\x1b8"));
        }
    }
}
