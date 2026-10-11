//! RAW 写真に埋め込まれた、JPEG のプレビューの取り出し。現像はしない。
//!
//! 対応する入れ物は3種類:
//! - **TIFF 系**（ARW・DNG・CR2・NEF・ORF・PEF・RW2 など）: IFD をたどって JPEG の候補を集め、
//!   いちばん大きな「ふつうの JPEG」を選ぶ。メーカー独自のデータの中にしかないときは、
//!   ファイルの先頭を走査して探す
//! - **RAF**（Fujifilm）: 先頭のヘッダに JPEG の位置と長さがある
//! - **CR3**（Canon）: ISO のメディア形式。独自の箱 `PRVW` に JPEG がある
//!
//! ファイル全体は読まず、構造とプレビューの部分だけを読む（数十 MB の RAW でも速い）。
//!
//! 気をつける点: DNG の本体は、無損失 JPEG（SOF3）のタイルに分かれていることがある。
//! これはプレビューではないので、JPEG の中の方式を見て、除く。

use std::collections::HashSet;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use image::metadata::Orientation;
use image::RgbImage;

/// 対応する拡張子。サンプルで確かめた形式は README にある
pub const EXTS: &[&str] = &[
    "arw", "sr2", "srf", "dng", "cr2", "cr3", "nef", "nrw", "pef", "orf", "rw2", "raf", "3fr",
    "erf", "mef",
];

pub fn is_raw_ext(ext: &str) -> bool {
    EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// たどる IFD の最大数。壊れたファイルや、ループするファイルで止まらなくなるのを防ぐ
const MAX_IFDS: usize = 64;
/// 1つの IFD の項目数の上限
const MAX_ENTRIES: usize = 2048;
/// JPEG の寸法を調べるために読む先頭の大きさ（ICC プロファイルなどで先頭が長いことがある）
const PROBE_BYTES: u64 = 1 << 20;
/// これより大きなプレビューは読まない
const MAX_PREVIEW_BYTES: u64 = 64 << 20;
/// 構造から JPEG が見つからないとき、JPEG を探して走査するファイルの先頭の大きさ
const SCAN_BYTES: u64 = 8 << 20;
/// 構造から見つかったプレビューの長辺がこれより小さいときは、走査でもっと大きなものを探す
const SMALL_PREVIEW: u32 = 1000;
/// CR3 の先頭から読む大きさ（moov とプレビューの箱は、ここに収まる）
const CR3_HEAD_BYTES: u64 = 8 << 20;

const CANON_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];
const CANON_PRVW_UUID: [u8; 16] = [
    0xea, 0xf4, 0x2b, 0x5e, 0x1c, 0x98, 0x4b, 0x88, 0xb9, 0xfb, 0xb7, 0xdc, 0x40, 0x6e, 0x4d, 0x16,
];

fn not_raw() -> String {
    t!("raw.not_tiff").to_string()
}

fn no_preview() -> String {
    t!("raw.no_preview").to_string()
}

// ---------------------------------------------------------------- 読み込み

enum Src {
    File(File),
    Mem(Vec<u8>),
}

/// ファイル（またはメモリ上のデータ）の、指定した範囲を読む
struct Reader {
    src: Src,
    len: u64,
}

impl Reader {
    fn open(path: &Path) -> Result<Reader, String> {
        let f = File::open(path).map_err(|e| e.to_string())?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        Ok(Reader {
            src: Src::File(f),
            len,
        })
    }

    fn bytes(v: Vec<u8>) -> Reader {
        Reader {
            len: v.len() as u64,
            src: Src::Mem(v),
        }
    }

    fn read_at(&mut self, off: u64, n: usize) -> Option<Vec<u8>> {
        if off.checked_add(n as u64)? > self.len {
            return None;
        }
        match &mut self.src {
            Src::Mem(v) => v.get(off as usize..off as usize + n).map(<[u8]>::to_vec),
            Src::File(f) => {
                let mut b = vec![0u8; n];
                if n > 0 {
                    f.seek(SeekFrom::Start(off)).ok()?;
                    f.read_exact(&mut b).ok()?;
                }
                Some(b)
            }
        }
    }
}

// ---------------------------------------------------------------- TIFF

#[derive(Debug, Clone, Copy)]
struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    /// 値そのもの（4 バイト以下のとき）か、値のあるオフセット
    field: [u8; 4],
}

struct Tiff {
    r: Reader,
    le: bool,
}

impl Tiff {
    /// ヘッダを確かめて、TIFF として開く。最初の IFD のオフセットも返す
    fn new(mut r: Reader) -> Option<(Tiff, u64)> {
        let h = r.read_at(0, 8)?;
        let le = match &h[..2] {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let t = Tiff { r, le };
        // 42 は ふつうの TIFF。0x4F52 と 0x5352 は Olympus の ORF、0x55 は Panasonic の RW2
        if !matches!(t.u16(&h[2..4]), 42 | 0x4F52 | 0x5352 | 0x55) {
            return None;
        }
        let first = u64::from(t.u32(&h[4..8]));
        Some((t, first))
    }

    fn u16(&self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        if self.le {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        }
    }

    fn u32(&self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    }

    /// IFD を読む。項目と、次の IFD のオフセット（なければ 0）を返す
    fn ifd(&mut self, off: u64) -> Option<(Vec<Entry>, u64)> {
        let head = self.r.read_at(off, 2)?;
        let n = usize::from(self.u16(&head));
        if n == 0 || n > MAX_ENTRIES {
            return None;
        }
        let b = self.r.read_at(off + 2, n * 12 + 4)?;
        let entries = (0..n)
            .map(|i| {
                let e = &b[i * 12..i * 12 + 12];
                Entry {
                    tag: self.u16(&e[0..2]),
                    typ: self.u16(&e[2..4]),
                    count: self.u32(&e[4..8]),
                    field: [e[8], e[9], e[10], e[11]],
                }
            })
            .collect();
        let next = u64::from(self.u32(&b[n * 12..n * 12 + 4]));
        Some((entries, next))
    }

    /// 値の入っているバイト列（4 バイト以下なら項目の中、それより大きければオフセット先）
    fn data(&mut self, e: &Entry, size: usize, limit: usize) -> Option<Vec<u8>> {
        let total = size.checked_mul(e.count as usize)?;
        let want = size * (e.count as usize).min(limit);
        if total <= 4 {
            Some(e.field[..total].to_vec())
        } else {
            let off = u64::from(self.u32(&e.field));
            self.r.read_at(off, want)
        }
    }

    /// 項目の値を整数の列として読む（BYTE・SHORT・LONG・IFD のみ）。最大 limit 個
    fn values(&mut self, e: &Entry, limit: usize) -> Vec<u32> {
        let size = match e.typ {
            1 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 13 => 4,
            _ => return Vec::new(),
        };
        let count = (e.count as usize).min(limit);
        let Some(data) = self.data(e, size, limit) else {
            return Vec::new();
        };
        (0..count)
            .filter_map(|i| {
                let s = data.get(i * size..(i + 1) * size)?;
                Some(match size {
                    1 => u32::from(s[0]),
                    2 => u32::from(self.u16(s)),
                    _ => self.u32(s),
                })
            })
            .collect()
    }

    /// 文字列（ASCII）の項目を読む。末尾の NUL と空白は除く
    fn ascii(&mut self, e: &Entry) -> Option<String> {
        if e.typ != 2 || e.count == 0 || e.count > 256 {
            return None;
        }
        let d = self.data(e, 1, 256)?;
        let s = String::from_utf8_lossy(&d);
        let s = s.split('\0').next().unwrap_or("").trim();
        (!s.is_empty()).then(|| s.to_string())
    }

    /// 分数（RATIONAL）の項目を読む。最初の1つ
    fn rational(&mut self, e: &Entry) -> Option<(u32, u32)> {
        if e.typ != 5 || e.count == 0 {
            return None;
        }
        let d = self.data(e, 8, 1)?;
        Some((self.u32(d.get(0..4)?), self.u32(d.get(4..8)?)))
    }
}

#[derive(Default)]
struct Scan {
    /// JPEG の候補（ファイル内のオフセット, 長さ）
    jpegs: Vec<(u64, u64)>,
    /// EXIF の PixelXDimension / PixelYDimension
    exif_dims: Option<(u32, u32)>,
    /// 本体（NewSubfileType が 0）の IFD のうち、いちばん大きなものの大きさ
    main_dims: Option<(u32, u32)>,
    /// 先頭の IFD の Orientation
    orientation: Option<u16>,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    /// 先頭の IFD（IFD0）
    First,
    /// EXIF の IFD
    Exif,
    Other,
}

/// IFD をたどって、JPEG の候補・大きさ・向きを集める（JPEG の中身はまだ見ない）。
/// first が EXIF の IFD そのものなら（CR3 の CMT2 など）、role に Role::Exif を渡す
fn scan(t: &mut Tiff, first: u64, role: Role) -> Scan {
    let mut s = Scan::default();
    let mut seen = HashSet::new();
    let mut queue = vec![(first, role)];
    while let Some((off, role)) = queue.pop() {
        if seen.len() >= MAX_IFDS || off == 0 || !seen.insert(off) {
            continue;
        }
        let Some((entries, next)) = t.ifd(off) else {
            continue;
        };
        let get = |tag: u16| entries.iter().find(|e| e.tag == tag).copied();
        let one = |t: &mut Tiff, tag: u16| get(tag).and_then(|e| t.values(&e, 1).first().copied());

        let (w, h) = (one(t, 0x100), one(t, 0x101));
        if role == Role::Exif {
            if let (Some(w), Some(h)) = (one(t, 0xA002), one(t, 0xA003)) {
                s.exif_dims = Some((w, h));
            }
        } else if let (Some(w), Some(h), 0) = (w, h, one(t, 0xFE).unwrap_or(0)) {
            let area = |d: (u32, u32)| u64::from(d.0) * u64::from(d.1);
            if s.main_dims.is_none_or(|m| area((w, h)) > area(m)) {
                s.main_dims = Some((w, h));
            }
        }
        if role == Role::First {
            s.orientation = one(t, 0x112).and_then(|o| u16::try_from(o).ok());
        }

        // JPEG の候補: JPEGInterchangeFormat、JPEG 圧縮で1本の strip、RW2 の JpgFromRaw（タグ 0x2E）
        if let (Some(o), Some(l)) = (one(t, 0x201), one(t, 0x202)) {
            s.jpegs.push((u64::from(o), u64::from(l)));
        }
        let tiled = get(0x144).is_some();
        let single_strip = get(0x111).is_some_and(|e| e.count == 1);
        if !tiled && single_strip && matches!(one(t, 0x103), Some(6 | 7)) {
            if let (Some(o), Some(l)) = (one(t, 0x111), one(t, 0x117)) {
                s.jpegs.push((u64::from(o), u64::from(l)));
            }
        }
        if let Some(e) = get(0x2E) {
            if matches!(e.typ, 1 | 7) && e.count > 4 {
                s.jpegs
                    .push((u64::from(t.u32(&e.field)), u64::from(e.count)));
            }
        }

        // 子の IFD と、次の IFD
        if let Some(e) = get(0x14A) {
            for o in t.values(&e, 16) {
                queue.push((u64::from(o), Role::Other));
            }
        }
        if let Some(o) = one(t, 0x8769) {
            queue.push((u64::from(o), Role::Exif));
        }
        if role != Role::Exif && next != 0 {
            queue.push((next, Role::Other));
        }
    }
    s
}

// ---------------------------------------------------------------- JPEG

/// JPEG の先頭から、ふつうの（DCT 方式の）JPEG の大きさを読む。
/// 無損失（SOF3）や算術符号などの、このあとデコードできないものは None
fn jpeg_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut p = 2;
    while p + 4 <= b.len() {
        if b[p] != 0xFF {
            return None;
        }
        let m = b[p + 1];
        if m == 0xFF {
            p += 1; // 詰め物
            continue;
        }
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            p += 2;
            continue;
        }
        if m == 0xD9 || m == 0xDA {
            return None;
        }
        let seg = usize::from(u16::from_be_bytes([b[p + 2], b[p + 3]]));
        if seg < 2 {
            return None;
        }
        match m {
            0xC0..=0xC2 => {
                if p + 9 > b.len() {
                    return None;
                }
                let h = u32::from(u16::from_be_bytes([b[p + 5], b[p + 6]]));
                let w = u32::from(u16::from_be_bytes([b[p + 7], b[p + 8]]));
                return (w > 0 && h > 0).then_some((w, h));
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
            _ => {}
        }
        p += 2 + seg;
    }
    None
}

/// JPEG の長さ（SOI から EOI まで）。構造が壊れていれば None。
/// 画像データの中の `FF D9` にだまされないよう、マーカーを順にたどる
fn jpeg_extent(b: &[u8]) -> Option<usize> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut p = 2;
    while p + 2 <= b.len() {
        if b[p] != 0xFF {
            return None;
        }
        let m = b[p + 1];
        match m {
            0xFF => p += 1,
            0xD9 => return Some(p + 2),
            0x01 | 0xD0..=0xD8 => p += 2,
            _ => {
                if p + 4 > b.len() {
                    return None;
                }
                let seg = usize::from(u16::from_be_bytes([b[p + 2], b[p + 3]]));
                if seg < 2 {
                    return None;
                }
                p += 2 + seg;
                if m == 0xDA {
                    // 画像データ: 次のマーカー（FF 00 の詰め物と、リスタートは除く）まで読み飛ばす
                    while p + 1 < b.len() {
                        if b[p] == 0xFF && b[p + 1] != 0x00 && !(0xD0..=0xD7).contains(&b[p + 1]) {
                            break;
                        }
                        p += 1;
                    }
                }
            }
        }
    }
    None
}

/// JPEG の APP1（Exif）の中の、TIFF のデータ
fn jpeg_exif_tiff(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut p = 2;
    while p + 4 <= b.len() && b[p] == 0xFF {
        let m = b[p + 1];
        if m == 0xDA || m == 0xD9 {
            return None;
        }
        let seg = usize::from(u16::from_be_bytes([b[p + 2], b[p + 3]]));
        if seg < 2 {
            return None;
        }
        if m == 0xE1 && b.get(p + 4..p + 10) == Some(b"Exif\0\0") {
            return b.get(p + 10..p + 2 + seg).map(<[u8]>::to_vec);
        }
        p += 2 + seg;
    }
    None
}

/// buf の中から、構造がきちんとした、ふつうの JPEG を探す（オフセット, 長さ）
fn find_jpegs(buf: &[u8]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(pos) = buf[i..].windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) {
        let at = i + pos;
        if jpeg_dims(&buf[at..]).is_some() {
            if let Some(l) = jpeg_extent(&buf[at..]) {
                out.push((at as u64, l as u64));
            }
        }
        i = at + 3;
    }
    out
}

/// 候補の中から、いちばん大きなふつうの JPEG を選ぶ
fn best_jpeg(r: &mut Reader, cands: &[(u64, u64)]) -> Option<((u64, u64), (u32, u32))> {
    let mut best: Option<((u64, u64), (u32, u32))> = None;
    for &(off, len) in cands {
        if len == 0 || off.saturating_add(len) > r.len {
            continue;
        }
        let Some(head) = r.read_at(off, len.min(PROBE_BYTES) as usize) else {
            continue;
        };
        let Some((w, h)) = jpeg_dims(&head) else {
            continue;
        };
        let key = |d: (u32, u32), l: u64| (u64::from(d.0) * u64::from(d.1), l);
        if best.is_none_or(|(b, d)| key((w, h), len) > key(d, b.1)) {
            best = Some(((off, len), (w, h)));
        }
    }
    best
}

// ---------------------------------------------------------------- 入れ物ごとの読み取り

/// RAW ファイルから分かったこと
#[derive(Debug, Clone)]
pub struct Info {
    /// 使うプレビュー JPEG の（オフセット, 長さ）
    pub jpeg: (u64, u64),
    /// プレビューの大きさ
    pub preview: (u32, u32),
    /// 画像の本来の大きさ（EXIF、なければ本体の IFD、それもなければプレビュー）
    pub image: (u32, u32),
    /// EXIF の向き（1〜8）。なければ 1
    pub orientation: u16,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Format {
    Tiff,
    Raf,
    Cr3,
}

fn detect(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        Some(Format::Raf)
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" && &head[8..12] == b"crx " {
        Some(Format::Cr3)
    } else if head.starts_with(b"II") || head.starts_with(b"MM") {
        Some(Format::Tiff)
    } else {
        None
    }
}

fn open(path: &Path) -> Result<(Reader, Format), String> {
    let mut r = Reader::open(path)?;
    let head = r.read_at(0, 16.min(r.len as usize)).ok_or_else(not_raw)?;
    let f = detect(&head).ok_or_else(not_raw)?;
    Ok((r, f))
}

fn tiff_info(r: Reader) -> Result<Info, String> {
    let (mut t, first) = Tiff::new(r).ok_or_else(not_raw)?;
    let s = scan(&mut t, first, Role::First);
    let mut best = best_jpeg(&mut t.r, &s.jpegs);
    // 構造の中に見つからない（または小さなサムネイルしかない）ときは、メーカー独自のデータの中を
    // 探すため、ファイルの先頭を走査する（古い Nikon の NEF、Olympus の ORF など）
    if best.is_none_or(|(_, d)| d.0.max(d.1) < SMALL_PREVIEW) {
        let n = t.r.len.min(SCAN_BYTES) as usize;
        if let Some(buf) = t.r.read_at(0, n) {
            let found = find_jpegs(&buf);
            if let Some(b) = best_jpeg(&mut t.r, &found) {
                let area = |d: (u32, u32)| u64::from(d.0) * u64::from(d.1);
                if best.is_none_or(|(_, d)| area(b.1) > area(d)) {
                    best = Some(b);
                }
            }
        }
    }
    let (jpeg, preview) = best.ok_or_else(no_preview)?;
    Ok(Info {
        jpeg,
        preview,
        image: s.exif_dims.or(s.main_dims).unwrap_or(preview),
        orientation: s.orientation.unwrap_or(1),
    })
}

/// RAF の、プレビュー JPEG の（位置, 長さ）。84 バイト目から、ビッグエンディアンの 4 バイトずつ
fn raf_jpeg_range(r: &mut Reader) -> Option<(u64, u64)> {
    let h = r.read_at(84, 8)?;
    let off = u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]]));
    let len = u64::from(u32::from_be_bytes([h[4], h[5], h[6], h[7]]));
    Some((off, len))
}

fn raf_info(mut r: Reader) -> Result<Info, String> {
    let range = raf_jpeg_range(&mut r).ok_or_else(not_raw)?;
    let (jpeg, preview) = best_jpeg(&mut r, &[range]).ok_or_else(no_preview)?;
    // 向きと大きさは、JPEG の中の EXIF から
    let head = r
        .read_at(jpeg.0, jpeg.1.min(PROBE_BYTES) as usize)
        .ok_or_else(no_preview)?;
    let s = jpeg_exif_tiff(&head)
        .and_then(|b| Tiff::new(Reader::bytes(b)))
        .map(|(mut t, first)| scan(&mut t, first, Role::First));
    Ok(Info {
        jpeg,
        preview,
        image: s.as_ref().and_then(|s| s.exif_dims).unwrap_or(preview),
        orientation: s.and_then(|s| s.orientation).unwrap_or(1),
    })
}

/// ISO のメディア形式の、メモリ上のデータの中の箱
struct BoxRef {
    start: usize,
    size: usize,
    typ: [u8; 4],
    body: usize,
    ext: Option<[u8; 16]>,
}

/// b の先頭から順に、箱を読む。はみ出した箱（大きな mdat など）は、そこで止める
fn boxes_in(b: &[u8]) -> Vec<BoxRef> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 8 <= b.len() && out.len() < 256 {
        let size32 = u32::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]);
        let typ = [b[p + 4], b[p + 5], b[p + 6], b[p + 7]];
        let mut hdr = 8usize;
        let mut size = size32 as usize;
        if size32 == 1 {
            let Some(s) = b.get(p + 8..p + 16) else { break };
            let mut a = [0u8; 8];
            a.copy_from_slice(s);
            let Ok(s) = usize::try_from(u64::from_be_bytes(a)) else {
                break;
            };
            size = s;
            hdr = 16;
        } else if size32 == 0 {
            size = b.len() - p;
        }
        let mut ext = None;
        if &typ == b"uuid" {
            let Some(u) = b.get(p + hdr..p + hdr + 16) else {
                break;
            };
            let mut a = [0u8; 16];
            a.copy_from_slice(u);
            ext = Some(a);
            hdr += 16;
        }
        if size < hdr || p.checked_add(size).is_none_or(|e| e > b.len()) {
            break;
        }
        out.push(BoxRef {
            start: p,
            size,
            typ,
            body: p + hdr,
            ext,
        });
        p += size;
    }
    out
}

/// CR3 の中の、プレビューと、メーカー・向き・撮影データの TIFF（CMT1 と CMT2）
struct Cr3 {
    /// プレビュー JPEG の（オフセット, 長さ）
    jpeg: Option<(u64, u64)>,
    cmt1: Option<Vec<u8>>,
    cmt2: Option<Vec<u8>>,
}

fn cr3_parse(r: &mut Reader) -> Option<Cr3> {
    let n = r.len.min(CR3_HEAD_BYTES) as usize;
    let b = r.read_at(0, n)?;
    let mut c = Cr3 {
        jpeg: None,
        cmt1: None,
        cmt2: None,
    };
    for top in boxes_in(&b) {
        if &top.typ == b"moov" {
            let moov = &b[top.body..top.start + top.size];
            for m in boxes_in(moov) {
                if m.ext != Some(CANON_UUID) {
                    continue;
                }
                let canon = &moov[m.body..m.start + m.size];
                for x in boxes_in(canon) {
                    let body = canon.get(x.body..x.start + x.size);
                    match (&x.typ, body) {
                        (b"CMT1", Some(d)) => c.cmt1 = Some(d.to_vec()),
                        (b"CMT2", Some(d)) => c.cmt2 = Some(d.to_vec()),
                        _ => {}
                    }
                }
            }
        } else if top.ext == Some(CANON_PRVW_UUID) {
            // 8 バイトの前置きのあとに PRVW の箱。その中身は、
            // 4 バイト（フラグ）+ 2（1）+ 幅 2 + 高さ 2 + 2（1）+ JPEG の長さ 4 + JPEG
            let inner = b.get(top.body + 8..top.start + top.size)?;
            if inner.len() > 24 && &inner[4..8] == b"PRVW" {
                let len = u64::from(u32::from_be_bytes([
                    inner[8 + 12],
                    inner[8 + 13],
                    inner[8 + 14],
                    inner[8 + 15],
                ]));
                c.jpeg = Some(((top.body + 8 + 8 + 16) as u64, len));
            }
        } else if &top.typ == b"mdat" {
            break;
        }
    }
    Some(c)
}

fn cr3_info(mut r: Reader) -> Result<Info, String> {
    let c = cr3_parse(&mut r).ok_or_else(not_raw)?;
    let cand: Vec<(u64, u64)> = c.jpeg.into_iter().collect();
    let (jpeg, preview) = best_jpeg(&mut r, &cand).ok_or_else(no_preview)?;
    let orientation = c
        .cmt1
        .and_then(|b| Tiff::new(Reader::bytes(b)))
        .and_then(|(mut t, first)| scan(&mut t, first, Role::First).orientation)
        .unwrap_or(1);
    // CMT2 は、EXIF の IFD そのもの
    let exif_dims = c
        .cmt2
        .and_then(|b| Tiff::new(Reader::bytes(b)))
        .and_then(|(mut t, first)| scan(&mut t, first, Role::Exif).exif_dims);
    Ok(Info {
        jpeg,
        preview,
        image: exif_dims.unwrap_or(preview),
        orientation,
    })
}

/// プレビューを探す。見つからなければエラー
pub fn info(path: &Path) -> Result<Info, String> {
    let (r, f) = open(path)?;
    match f {
        Format::Tiff => tiff_info(r),
        Format::Raf => raf_info(r),
        Format::Cr3 => cr3_info(r),
    }
}

/// 画像の本来の大きさ。プレビューの中身は、できるだけ開かない。取れなければ None
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
    let (r, f) = open(path).ok()?;
    if f == Format::Tiff {
        let (mut t, first) = Tiff::new(r)?;
        let s = scan(&mut t, first, Role::First);
        if let Some(d) = s.exif_dims.or(s.main_dims) {
            return Some(d);
        }
    }
    info(path).ok().map(|i| i.image)
}

/// プレビューを読み込んで、向きを反映した RGB にする。
/// JPEG は、長辺が longest ピクセル程度になるよう、縮小しながらデコードする。
pub fn load_preview(path: &Path, longest: u32) -> Result<RgbImage, String> {
    let bad = || t!("raw.bad_preview").to_string();
    let info = info(path)?;
    let (off, len) = info.jpeg;
    if len > MAX_PREVIEW_BYTES {
        return Err(bad());
    }
    let bytes = Reader::open(path)?
        .read_at(off, len as usize)
        .ok_or_else(bad)?;

    let mut d = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    d.read_info().map_err(|_| bad())?;
    let (pw, ph) = info.preview;
    let f = (f64::from(longest) / f64::from(pw.max(ph))).min(1.0);
    let want = |v: u32| ((f64::from(v) * f).ceil() as u32).clamp(1, u32::from(u16::MAX)) as u16;
    let (sw, sh) = d.scale(want(pw), want(ph)).map_err(|_| bad())?;
    let pixels = d.decode().map_err(|_| bad())?;
    let rgb = match d.info().ok_or_else(bad)?.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels,
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&v| [v, v, v]).collect(),
        _ => return Err(bad()),
    };
    let img = RgbImage::from_raw(u32::from(sw), u32::from(sh), rgb).ok_or_else(bad)?;
    let o = u8::try_from(info.orientation)
        .ok()
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms);
    Ok(crate::orient::apply(img, o))
}

// ---------------------------------------------------------------- 撮影データ

/// 撮影データ。`info.rs` の表示と同じ書き方の文字列
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Meta {
    pub camera: Option<String>,
    pub aperture: Option<String>,
    pub exposure: Option<String>,
    pub iso: Option<String>,
    pub focal: Option<String>,
    pub taken: Option<String>,
}

impl Meta {
    pub fn is_empty(&self) -> bool {
        *self == Meta::default()
    }
}

/// 小数第1位まで。`8.0` は `8` にする
fn trim_num(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").unwrap_or(&s).to_string()
}

fn ratio(r: (u32, u32)) -> Option<f64> {
    (r.1 != 0).then(|| f64::from(r.0) / f64::from(r.1))
}

fn fmt_exposure(v: f64) -> Option<String> {
    if v <= 0.0 {
        None
    } else if v >= 1.0 {
        Some(format!("{}s", trim_num(v)))
    } else {
        Some(format!("1/{}s", (1.0 / v).round() as u64))
    }
}

/// `2025:12:27 16:16:04` を `2025-12-27 16:16:04` にする
fn fmt_taken(s: &str) -> String {
    let mut parts = s.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(rest)) if y.len() == 4 => format!("{y}-{m}-{rest}"),
        _ => s.to_string(),
    }
}

/// TIFF の IFD から、メーカー・機種・撮影設定を読む。
/// first が EXIF の IFD そのものなら（CR3 の CMT2）、role に Role::Exif を渡す
fn meta_from(t: &mut Tiff, first: u64, role: Role) -> Meta {
    let mut m = Meta::default();
    let (mut make, mut model) = (None, None);
    let mut seen = HashSet::new();
    let mut queue = vec![(first, role)];
    while let Some((off, role)) = queue.pop() {
        if seen.len() >= MAX_IFDS || off == 0 || !seen.insert(off) {
            continue;
        }
        let Some((entries, _)) = t.ifd(off) else {
            continue;
        };
        let get = |tag: u16| entries.iter().find(|e| e.tag == tag).copied();
        match role {
            Role::First => {
                make = get(0x10F).and_then(|e| t.ascii(&e));
                model = get(0x110).and_then(|e| t.ascii(&e));
                if let Some(o) = get(0x8769).and_then(|e| t.values(&e, 1).first().copied()) {
                    queue.push((u64::from(o), Role::Exif));
                }
            }
            Role::Exif => {
                m.aperture = get(0x829D)
                    .and_then(|e| t.rational(&e))
                    .and_then(ratio)
                    .filter(|v| *v > 0.0)
                    .map(|v| format!("f/{}", trim_num(v)));
                m.exposure = get(0x829A)
                    .and_then(|e| t.rational(&e))
                    .and_then(ratio)
                    .and_then(fmt_exposure);
                m.iso = get(0x8827)
                    .and_then(|e| t.values(&e, 1).first().copied())
                    .filter(|v| *v > 0)
                    .map(|v| format!("ISO{v}"));
                m.focal = get(0x920A)
                    .and_then(|e| t.rational(&e))
                    .and_then(ratio)
                    .filter(|v| *v > 0.0)
                    .map(|v| format!("{}mm", trim_num(v)));
                m.taken = get(0x9003).and_then(|e| t.ascii(&e)).map(|s| fmt_taken(&s));
            }
            Role::Other => {}
        }
    }
    m.camera = match (make, model) {
        (Some(mk), Some(md)) if md.starts_with(&mk) => Some(md),
        (Some(mk), Some(md)) => Some(format!("{mk} {md}")),
        (mk, md) => mk.or(md),
    };
    m
}

/// 外部のクレートでは EXIF を読めない RAW（RAF・RW2・CR3・ORF など）の撮影データ。読めなければ None
pub fn meta(path: &Path) -> Option<Meta> {
    let (mut r, f) = open(path).ok()?;
    let m = match f {
        Format::Tiff => {
            let (mut t, first) = Tiff::new(r)?;
            meta_from(&mut t, first, Role::First)
        }
        Format::Raf => {
            let (off, len) = raf_jpeg_range(&mut r)?;
            let head = r.read_at(off, len.min(PROBE_BYTES) as usize)?;
            let (mut t, first) = Tiff::new(Reader::bytes(jpeg_exif_tiff(&head)?))?;
            meta_from(&mut t, first, Role::First)
        }
        Format::Cr3 => {
            let c = cr3_parse(&mut r)?;
            let mut m = c
                .cmt1
                .and_then(|b| Tiff::new(Reader::bytes(b)))
                .map(|(mut t, first)| meta_from(&mut t, first, Role::First))
                .unwrap_or_default();
            if let Some((mut t, first)) = c.cmt2.and_then(|b| Tiff::new(Reader::bytes(b))) {
                let e = meta_from(&mut t, first, Role::Exif);
                (m.aperture, m.exposure, m.iso, m.focal, m.taken) =
                    (e.aperture, e.exposure, e.iso, e.focal, e.taken);
            }
            m
        }
    };
    (!m.is_empty()).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ------------------------------------------------------------ 合成ファイルの組み立て

    /// 左半分と右半分が違う色の JPEG
    fn jpeg(w: u32, h: u32, left: [u8; 3], right: [u8; 3]) -> Vec<u8> {
        let mut img = RgbImage::new(w, h);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb(if x < w / 2 { left } else { right });
        }
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    /// JPEG の SOI の直後に、EXIF（APP1）として TIFF のデータを入れる
    fn with_exif(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
        let mut seg = b"Exif\0\0".to_vec();
        seg.extend_from_slice(tiff);
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&seg);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// DNG の本体のタイルのような、無損失 JPEG（SOF3）。ヘッダだけ
    fn lossless_stub() -> Vec<u8> {
        vec![
            0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B, 0x10, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11,
            0x00, 0xFF, 0xD9,
        ]
    }

    struct Tw {
        le: bool,
        buf: Vec<u8>,
    }

    impl Tw {
        fn new(le: bool) -> Tw {
            Tw::with_magic(le, 42)
        }
        fn with_magic(le: bool, magic: u16) -> Tw {
            let mut buf = Vec::new();
            buf.extend_from_slice(if le { b"II" } else { b"MM" });
            let mut t = Tw { le, buf };
            let m = t.p16(magic);
            t.buf.extend_from_slice(&m);
            t.buf.extend_from_slice(&[0; 4]); // 最初の IFD のオフセットは、あとで入れる
            t
        }
        fn p16(&self, v: u16) -> [u8; 2] {
            if self.le {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        }
        fn p32(&self, v: u32) -> [u8; 4] {
            if self.le {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        }
        /// データを置いて、そのオフセットを返す
        fn blob(&mut self, data: &[u8]) -> u32 {
            let off = self.buf.len() as u32;
            self.buf.extend_from_slice(data);
            if self.buf.len() % 2 == 1 {
                self.buf.push(0);
            }
            off
        }
        fn head(&self, tag: u16, typ: u16, count: u32) -> [u8; 12] {
            let mut e = [0u8; 12];
            e[0..2].copy_from_slice(&self.p16(tag));
            e[2..4].copy_from_slice(&self.p16(typ));
            e[4..8].copy_from_slice(&self.p32(count));
            e
        }
        fn short(&self, tag: u16, v: u16) -> [u8; 12] {
            let mut e = self.head(tag, 3, 1);
            e[8..10].copy_from_slice(&self.p16(v));
            e
        }
        fn long(&self, tag: u16, v: u32) -> [u8; 12] {
            let mut e = self.head(tag, 4, 1);
            e[8..12].copy_from_slice(&self.p32(v));
            e
        }
        /// LONG の配列（2 個以上）を置いて、その項目を返す
        fn longs(&mut self, tag: u16, vals: &[u32]) -> [u8; 12] {
            let data: Vec<u8> = vals.iter().flat_map(|v| self.p32(*v)).collect();
            let off = self.blob(&data);
            let mut e = self.head(tag, 4, vals.len() as u32);
            e[8..12].copy_from_slice(&self.p32(off));
            e
        }
        /// 文字列（ASCII）の項目
        fn ascii(&mut self, tag: u16, s: &str) -> [u8; 12] {
            let mut d = s.as_bytes().to_vec();
            d.push(0);
            let mut e = self.head(tag, 2, d.len() as u32);
            if d.len() <= 4 {
                e[8..8 + d.len()].copy_from_slice(&d);
            } else {
                let off = self.blob(&d);
                e[8..12].copy_from_slice(&self.p32(off));
            }
            e
        }
        /// 分数（RATIONAL）の項目
        fn rational(&mut self, tag: u16, n: u32, d: u32) -> [u8; 12] {
            let mut data = self.p32(n).to_vec();
            data.extend_from_slice(&self.p32(d));
            let off = self.blob(&data);
            let mut e = self.head(tag, 5, 1);
            e[8..12].copy_from_slice(&self.p32(off));
            e
        }
        /// 中身が任意のバイト列（UNDEFINED）の項目。RW2 の JpgFromRaw 用
        fn undefined(&mut self, tag: u16, data: &[u8]) -> [u8; 12] {
            let off = self.blob(data);
            let mut e = self.head(tag, 7, data.len() as u32);
            e[8..12].copy_from_slice(&self.p32(off));
            e
        }
        /// 項目を tag の順に並べて IFD を置き、そのオフセットを返す
        fn ifd(&mut self, mut entries: Vec<[u8; 12]>, next: u32) -> u32 {
            let le = self.le;
            entries.sort_by_key(|e| {
                if le {
                    u16::from_le_bytes([e[0], e[1]])
                } else {
                    u16::from_be_bytes([e[0], e[1]])
                }
            });
            let off = self.buf.len() as u32;
            let n = self.p16(entries.len() as u16);
            self.buf.extend_from_slice(&n);
            for e in &entries {
                self.buf.extend_from_slice(e);
            }
            let nx = self.p32(next);
            self.buf.extend_from_slice(&nx);
            off
        }
        fn finish(mut self, first: u32) -> Vec<u8> {
            let f = self.p32(first);
            self.buf[4..8].copy_from_slice(&f);
            self.buf
        }
    }

    /// 撮影データ（1/250 秒・f/5.6・ISO400・50mm・2025-12-27 16:16:04）の項目
    fn shot_entries(w: &mut Tw) -> Vec<[u8; 12]> {
        vec![
            w.rational(0x829A, 1, 250),
            w.rational(0x829D, 56, 10),
            w.short(0x8827, 400),
            w.ascii(0x9003, "2025:12:27 16:16:04"),
            w.rational(0x920A, 500, 10),
        ]
    }

    /// DxO などが作る DNG のような配置: 先頭は小さなサムネイル、
    /// サブ IFD0 は無損失 JPEG のタイル（プレビューではない）、サブ IFD1 が JPEG のプレビュー
    pub(crate) fn dng_like(le: bool) -> Vec<u8> {
        let mut w = Tw::new(le);
        let preview = jpeg(96, 64, [255, 0, 0], [0, 0, 255]);
        let p_off = w.blob(&preview);
        let tile_a = w.blob(&lossless_stub());
        let tile_b = w.blob(&lossless_stub());
        let tile_len = lossless_stub().len() as u32;

        let sub0 = {
            let e = vec![
                w.long(0xFE, 0),
                w.short(0x100, 160),
                w.short(0x101, 96),
                w.short(0x103, 7),
                w.short(0x142, 16),
                w.short(0x143, 16),
                w.longs(0x144, &[tile_a, tile_b]),
                w.longs(0x145, &[tile_len, tile_len]),
            ];
            w.ifd(e, 0)
        };
        let sub1 = {
            let e = vec![
                w.long(0xFE, 1),
                w.short(0x100, 96),
                w.short(0x101, 64),
                w.short(0x103, 7),
                w.long(0x111, p_off),
                w.long(0x117, preview.len() as u32),
            ];
            w.ifd(e, 0)
        };
        let ifd0 = {
            let e = vec![
                w.long(0xFE, 1),
                w.short(0x100, 4),
                w.short(0x101, 3),
                w.short(0x103, 1),
                w.short(0x112, 1),
                w.longs(0x14A, &[sub0, sub1]),
            ];
            w.ifd(e, 0)
        };
        w.finish(ifd0)
    }

    /// Sony の ARW のような配置: 先頭の IFD に JPEG のプレビュー、2つ目の IFD に小さなサムネイル、
    /// サブ IFD に本体（独自の圧縮）、EXIF に画像の大きさと撮影データ
    pub(crate) fn arw_like(le: bool, orientation: u16) -> Vec<u8> {
        let mut w = Tw::new(le);
        let preview = jpeg(64, 48, [0, 200, 0], [255, 255, 0]);
        let thumb = jpeg(16, 12, [10, 10, 10], [20, 20, 20]);
        let p_off = w.blob(&preview);
        let t_off = w.blob(&thumb);
        let raw = {
            let e = vec![
                w.long(0xFE, 0),
                w.short(0x100, 128),
                w.short(0x101, 84),
                w.short(0x103, 32767),
                w.long(0x111, 1000),
                w.long(0x117, 5000),
            ];
            w.ifd(e, 0)
        };
        let exif = {
            let mut e = shot_entries(&mut w);
            e.push(w.long(0xA002, 120));
            e.push(w.long(0xA003, 80));
            w.ifd(e, 0)
        };
        let ifd1 = {
            let e = vec![
                w.long(0xFE, 1),
                w.short(0x103, 6),
                w.long(0x201, t_off),
                w.long(0x202, thumb.len() as u32),
            ];
            w.ifd(e, 0)
        };
        let ifd0 = {
            let e = vec![
                w.long(0xFE, 1),
                w.short(0x103, 6),
                w.ascii(0x10F, "SONY"),
                w.ascii(0x110, "ILCE-6000"),
                w.short(0x112, orientation),
                w.long(0x14A, raw),
                w.long(0x201, p_off),
                w.long(0x202, preview.len() as u32),
                w.long(0x8769, exif),
            ];
            w.ifd(e, ifd1)
        };
        w.finish(ifd0)
    }

    /// Panasonic の RW2 のような配置: 識別子は 0x55、IFD0 のタグ 0x2E に JPEG
    pub(crate) fn rw2_like() -> Vec<u8> {
        let mut w = Tw::with_magic(true, 0x55);
        let preview = jpeg(80, 60, [200, 0, 200], [0, 200, 200]);
        let exif = {
            let e = shot_entries(&mut w);
            w.ifd(e, 0)
        };
        let ifd0 = {
            let e = vec![
                w.undefined(0x2E, &preview),
                w.ascii(0x10F, "Panasonic"),
                w.ascii(0x110, "DMC-LX7"),
                w.short(0x112, 1),
                w.long(0x8769, exif),
            ];
            w.ifd(e, 0)
        };
        w.finish(ifd0)
    }

    /// 古い NEF や ORF のような配置: TIFF の構造の中には JPEG がなく、
    /// 構造の外（メーカー独自のデータの中）に、小さなサムネイルと、大きめのプレビューがある
    pub(crate) fn makernote_like() -> Vec<u8> {
        let mut w = Tw::new(false);
        let thumb = jpeg(16, 12, [0, 0, 0], [30, 30, 30]);
        let preview = jpeg(200, 150, [250, 10, 10], [10, 10, 250]);
        let _pad = w.blob(&[0u8; 100]);
        let _t = w.blob(&thumb);
        let _p = w.blob(&preview);
        let ifd0 = {
            let e = vec![
                w.short(0x100, 400),
                w.short(0x101, 300),
                w.short(0x103, 1),
                w.short(0x112, 1),
            ];
            w.ifd(e, 0)
        };
        w.finish(ifd0)
    }

    /// Fujifilm の RAF のような配置: ヘッダの 84 バイト目に、JPEG の位置と長さ（ビッグエンディアン）
    pub(crate) fn raf_like(orientation: u16) -> Vec<u8> {
        let mut exif = Tw::new(true);
        let ifd0 = {
            let e = vec![
                exif.ascii(0x10F, "FUJIFILM"),
                exif.ascii(0x110, "FinePix S5000"),
                exif.short(0x112, orientation),
            ];
            exif.ifd(e, 0)
        };
        let tiff = exif.finish(ifd0);
        let j = with_exif(&jpeg(64, 48, [0, 120, 255], [255, 120, 0]), &tiff);
        let mut b = b"FUJIFILMCCD-RAW 0201FF389201FinePix S5000 ".to_vec();
        b.resize(84, 0);
        b.extend_from_slice(&148u32.to_be_bytes());
        b.extend_from_slice(&(j.len() as u32).to_be_bytes());
        b.resize(148, 0);
        b.extend_from_slice(&j);
        b.extend_from_slice(&[0u8; 64]); // この先は、センサーのデータ
        b
    }

    fn mp4_box(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(body);
        v
    }

    fn mp4_uuid(uuid: &[u8; 16], body: &[u8]) -> Vec<u8> {
        let mut b = uuid.to_vec();
        b.extend_from_slice(body);
        mp4_box(b"uuid", &b)
    }

    /// Canon の CR3 のような配置: moov の中の Canon 独自の箱に CMT1 と CMT2、
    /// その後ろの独自の箱に PRVW（JPEG）、最後に mdat
    pub(crate) fn cr3_like(orientation: u16) -> Vec<u8> {
        let mut cmt1 = Tw::new(true);
        let c1 = {
            let e = vec![
                cmt1.ascii(0x10F, "Canon"),
                cmt1.ascii(0x110, "Canon EOS R6"),
                cmt1.short(0x112, orientation),
            ];
            cmt1.ifd(e, 0)
        };
        let cmt1 = cmt1.finish(c1);
        let mut cmt2 = Tw::new(true);
        let c2 = {
            let e = vec![
                cmt2.rational(0x829A, 1, 125),
                cmt2.rational(0x829D, 40, 10),
                cmt2.short(0x8827, 100),
                cmt2.ascii(0x9003, "2021:05:14 10:20:30"),
                cmt2.rational(0x920A, 350, 10),
                cmt2.long(0xA002, 5472),
                cmt2.long(0xA003, 3648),
            ];
            cmt2.ifd(e, 0)
        };
        let cmt2 = cmt2.finish(c2);

        let canon = [
            mp4_box(b"CNCV", b"CanonCR3_001/00.10.00"),
            mp4_box(b"CMT1", &cmt1),
            mp4_box(b"CMT2", &cmt2),
            mp4_box(b"THMB", &[0u8; 20]),
        ]
        .concat();
        let moov = mp4_box(b"moov", &mp4_uuid(&CANON_UUID, &canon));

        let j = jpeg(81, 54, [255, 0, 255], [0, 255, 0]);
        let mut prvw = vec![0, 0, 0, 0, 0, 1];
        prvw.extend_from_slice(&81u16.to_be_bytes());
        prvw.extend_from_slice(&54u16.to_be_bytes());
        prvw.extend_from_slice(&[0, 1]);
        prvw.extend_from_slice(&(j.len() as u32).to_be_bytes());
        prvw.extend_from_slice(&j);
        let mut inner = vec![0, 0, 0, 0, 0, 0, 0, 1]; // 8 バイトの前置き
        inner.extend_from_slice(&mp4_box(b"PRVW", &prvw));

        let mut f = mp4_box(b"ftyp", b"crx \0\0\0\x01crx isom");
        f.extend_from_slice(&moov);
        f.extend_from_slice(&mp4_uuid(&CANON_PRVW_UUID, &inner));
        f.extend_from_slice(&mp4_box(b"mdat", &[0u8; 64]));
        f
    }

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gls-raw-test-{}-{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn red_left_blue_right(img: &RgbImage) -> bool {
        let (w, h) = img.dimensions();
        let l = img.get_pixel(w / 4, h / 2).0;
        let r = img.get_pixel(w * 3 / 4, h / 2).0;
        l[0] > 200 && l[2] < 60 && r[2] > 200 && r[0] < 60
    }

    // ------------------------------------------------------------ テスト

    #[test]
    fn extensions() {
        assert!(is_raw_ext("ARW") && is_raw_ext("dng") && is_raw_ext("Nef"));
        assert!(is_raw_ext("CR3") && is_raw_ext("raf") && is_raw_ext("RW2"));
        assert!(!is_raw_ext("jpg") && !is_raw_ext("x3f") && !is_raw_ext("txt"));
    }

    #[test]
    fn dng_like_uses_the_baseline_preview_and_not_the_lossless_tiles() {
        for le in [true, false] {
            let p = temp(&format!("dng-{le}.dng"), &dng_like(le));
            let i = info(&p).unwrap();
            assert_eq!(i.preview, (96, 64), "le={le}");
            // EXIF に大きさがないので、本体の IFD（タイルの親）の大きさ
            assert_eq!(i.image, (160, 96), "le={le}");
            assert_eq!(image_size(&p), Some((160, 96)));
            let img = load_preview(&p, 1000).unwrap();
            assert_eq!(img.dimensions(), (96, 64));
            assert!(red_left_blue_right(&img), "le={le}");
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn arw_like_reads_the_first_ifd_jpeg_and_ignores_the_thumbnail() {
        for le in [true, false] {
            let p = temp(&format!("arw-{le}.arw"), &arw_like(le, 1));
            let i = info(&p).unwrap();
            assert_eq!(i.preview, (64, 48), "le={le}");
            assert_eq!(i.image, (120, 80), "EXIF の大きさが優先される");
            assert_eq!(i.orientation, 1);
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn orientation_is_applied_to_the_preview() {
        let p = temp("rot.arw", &arw_like(true, 6));
        let img = load_preview(&p, 1000).unwrap();
        assert_eq!(img.dimensions(), (48, 64), "90° 回転で縦横が入れ替わる");
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn the_preview_is_scaled_down_while_decoding() {
        let p = temp("scale.dng", &dng_like(true));
        let img = load_preview(&p, 24).unwrap();
        assert!(
            img.width() <= 48 && img.width() >= 12,
            "{:?}",
            img.dimensions()
        );
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn rw2_like_reads_the_jpeg_from_tag_0x2e() {
        let p = temp("lx7.rw2", &rw2_like());
        let i = info(&p).unwrap();
        assert_eq!(i.preview, (80, 60));
        assert_eq!(load_preview(&p, 1000).unwrap().dimensions(), (80, 60));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn a_jpeg_outside_the_tiff_structure_is_found_by_scanning() {
        let p = temp("old.nef", &makernote_like());
        let i = info(&p).unwrap();
        assert_eq!(
            i.preview,
            (200, 150),
            "小さなサムネイルではなく、大きいほう"
        );
        assert_eq!(i.image, (400, 300), "本体の IFD の大きさ");
        let img = load_preview(&p, 1000).unwrap();
        assert!(red_left_blue_right(&img));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn raf_like_reads_the_header_and_the_exif_inside_the_jpeg() {
        for o in [1u16, 6] {
            let p = temp(&format!("fuji{o}.raf"), &raf_like(o));
            let i = info(&p).unwrap();
            assert_eq!(i.jpeg.0, 148);
            assert_eq!(i.preview, (64, 48));
            assert_eq!(i.orientation, o);
            let img = load_preview(&p, 1000).unwrap();
            let want = if o == 6 { (48, 64) } else { (64, 48) };
            assert_eq!(img.dimensions(), want, "orientation {o}");
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn cr3_like_reads_prvw_and_the_cmt_boxes() {
        for o in [1u16, 8] {
            let p = temp(&format!("canon{o}.cr3"), &cr3_like(o));
            let i = info(&p).unwrap();
            assert_eq!(i.preview, (81, 54));
            assert_eq!(i.image, (5472, 3648), "CMT2 の EXIF の大きさ");
            assert_eq!(i.orientation, o);
            assert_eq!(image_size(&p), Some((5472, 3648)));
            let img = load_preview(&p, 1000).unwrap();
            let want = if o == 8 { (54, 81) } else { (81, 54) };
            assert_eq!(img.dimensions(), want, "orientation {o}");
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn meta_is_read_from_every_container() {
        let want = |camera: &str| Meta {
            camera: Some(camera.to_string()),
            aperture: Some("f/5.6".into()),
            exposure: Some("1/250s".into()),
            iso: Some("ISO400".into()),
            focal: Some("50mm".into()),
            taken: Some("2025-12-27 16:16:04".into()),
        };
        let p = temp("m.arw", &arw_like(true, 1));
        assert_eq!(meta(&p), Some(want("SONY ILCE-6000")));
        std::fs::remove_file(p).unwrap();

        let p = temp("m.rw2", &rw2_like());
        assert_eq!(meta(&p), Some(want("Panasonic DMC-LX7")));
        std::fs::remove_file(p).unwrap();

        // CR3: メーカー・機種は CMT1 から、撮影設定は CMT2 から
        let p = temp("m.cr3", &cr3_like(1));
        let m = meta(&p).unwrap();
        assert_eq!(m.camera.as_deref(), Some("Canon EOS R6"));
        assert_eq!(m.aperture.as_deref(), Some("f/4"));
        assert_eq!(m.exposure.as_deref(), Some("1/125s"));
        assert_eq!(m.iso.as_deref(), Some("ISO100"));
        assert_eq!(m.focal.as_deref(), Some("35mm"));
        assert_eq!(m.taken.as_deref(), Some("2021-05-14 10:20:30"));
        std::fs::remove_file(p).unwrap();

        // RAF: JPEG の中の EXIF から（この合成ファイルには、撮影設定はなく、カメラだけ）
        let p = temp("m.raf", &raf_like(1));
        let m = meta(&p).unwrap();
        assert_eq!(m.camera.as_deref(), Some("FUJIFILM FinePix S5000"));
        assert_eq!(m.aperture, None);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn number_formats() {
        assert_eq!(trim_num(8.0), "8");
        assert_eq!(trim_num(5.6), "5.6");
        assert_eq!(fmt_exposure(1.0 / 250.0).as_deref(), Some("1/250s"));
        assert_eq!(fmt_exposure(1.3).as_deref(), Some("1.3s"));
        assert_eq!(fmt_exposure(30.0).as_deref(), Some("30s"));
        assert_eq!(fmt_exposure(0.0), None);
        assert_eq!(fmt_taken("2025:12:27 16:16:04"), "2025-12-27 16:16:04");
        assert_eq!(fmt_taken("garbage"), "garbage");
    }

    #[test]
    fn files_without_a_preview_or_not_raw_are_errors() {
        // JPEG のプレビューがない TIFF（構造だけ）
        let mut w = Tw::new(true);
        let e = vec![w.short(0x100, 4), w.short(0x101, 3), w.short(0x103, 1)];
        let ifd0 = w.ifd(e, 0);
        let p = temp("nopreview.dng", &w.finish(ifd0));
        assert!(info(&p).is_err());
        std::fs::remove_file(p).unwrap();

        let p = temp("text.arw", b"this is not a raw file at all");
        assert!(info(&p).is_err());
        assert!(load_preview(&p, 100).is_err());
        assert!(meta(&p).is_none());
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn damaged_files_never_panic() {
        // 先頭から切り詰めた、壊れたファイル。パニックせず、エラーか成功のどちらかになる
        let all: [(&str, Vec<u8>); 6] = [
            ("dng", dng_like(true)),
            ("arw", arw_like(false, 6)),
            ("rw2", rw2_like()),
            ("nef", makernote_like()),
            ("raf", raf_like(1)),
            ("cr3", cr3_like(1)),
        ];
        for (ext, full) in &all {
            for cut in (0..full.len()).step_by(7) {
                let p = temp(&format!("cut.{ext}"), &full[..cut]);
                let _ = info(&p);
                let _ = image_size(&p);
                let _ = meta(&p);
                let _ = load_preview(&p, 100);
                std::fs::remove_file(p).unwrap();
            }
        }
        // 自分自身を指す IFD（ループ）
        let mut w = Tw::new(true);
        let e = vec![w.short(0x100, 4)];
        let at = w.buf.len() as u32;
        let ifd0 = w.ifd(e, at); // 次の IFD が自分
        let p = temp("loop.dng", &w.finish(ifd0));
        assert!(info(&p).is_err());
        std::fs::remove_file(p).unwrap();
        // 大きさがはみ出す箱
        let mut cr3 = cr3_like(1);
        cr3[24..28].copy_from_slice(&u32::MAX.to_be_bytes()); // moov のサイズが大きすぎる
        let p = temp("big.cr3", &cr3);
        let _ = info(&p);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn jpeg_markers() {
        let j = jpeg(30, 20, [0; 3], [0; 3]);
        assert_eq!(jpeg_dims(&j), Some((30, 20)));
        assert_eq!(jpeg_extent(&j), Some(j.len()), "SOI から EOI まで");
        assert_eq!(jpeg_dims(&lossless_stub()), None, "SOF3 は除く");
        assert_eq!(jpeg_dims(b"not a jpeg"), None);
        assert_eq!(jpeg_dims(&[]), None);
        assert_eq!(jpeg_extent(&[]), None);
        // 後ろにごみが続いていても、EOI で終わる
        let mut g = j.clone();
        g.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(jpeg_extent(&g), Some(j.len()));
    }

    /// 合成ファイルをテスト用に書き出す（`cargo test write_fixtures -- --ignored`）。
    /// 出力は tests/raw/ に置いて、コミットする
    #[test]
    #[ignore]
    fn write_fixtures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("raw");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("synthetic.dng"), dng_like(true)).unwrap();
        std::fs::write(dir.join("synthetic.arw"), arw_like(true, 1)).unwrap();
        std::fs::write(dir.join("synthetic.rw2"), rw2_like()).unwrap();
        std::fs::write(dir.join("synthetic.raf"), raf_like(1)).unwrap();
        std::fs::write(dir.join("synthetic.cr3"), cr3_like(1)).unwrap();
        std::fs::write(dir.join("synthetic.nef"), makernote_like()).unwrap();
    }

    /// 本物の RAW ファイルで確かめる（`GLS_RAW_SAMPLES=フォルダ cargo test real_raw -- --ignored --nocapture`）
    #[test]
    #[ignore]
    fn real_raw_files() {
        let Some(dir) = std::env::var_os("GLS_RAW_SAMPLES") else {
            eprintln!("GLS_RAW_SAMPLES is not set");
            return;
        };
        let (mut n, mut failed) = (0, Vec::new());
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let p = e.path();
            if !p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(is_raw_ext)
            {
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let t0 = std::time::Instant::now();
            match info(&p).and_then(|i| load_preview(&p, 800).map(|img| (i, img))) {
                Ok((i, img)) => eprintln!(
                    "{name}: preview {}x{} ({} bytes), image {}x{}, orientation {}, decoded {}x{} in {:?}; meta {:?}",
                    i.preview.0,
                    i.preview.1,
                    i.jpeg.1,
                    i.image.0,
                    i.image.1,
                    i.orientation,
                    img.width(),
                    img.height(),
                    t0.elapsed(),
                    meta(&p).map(|m| m.camera),
                ),
                Err(e) => {
                    eprintln!("{name}: FAILED: {e}");
                    failed.push(name);
                }
            }
            n += 1;
        }
        assert!(n > 0, "no RAW files in the folder");
        assert!(failed.is_empty(), "failed: {failed:?}");
    }
}
