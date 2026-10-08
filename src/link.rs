//! ファイル名のハイパーリンク（OSC 8）。対応端末では Ctrl+クリックで既定のアプリで開ける

use std::path::Path;

/// 英数字と `-._~/:` 以外を %XX にする。
/// keep_unicode なら、ASCII 以外の文字（日本語など）はそのまま残す。
///
/// Windows のシェル（ShellExecute）は、`file://` URL の %XX を UTF-8 ではなく ANSI（日本語環境では cp932）として
/// 解釈するため、日本語を UTF-8 で %エンコードするとファイルが見つからない。生の Unicode のまま渡せば開ける。
fn percent_encode(s: &str, keep_unicode: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-._~/:".contains(c) || (keep_unicode && !c.is_ascii()) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// 区切りを `/` にそろえたパス文字列から file URL を作る。
/// WSL 内なら、Windows の既定アプリで開けるよう `/mnt/c/...` は `C:/...` に、
/// それ以外は `\\wsl.localhost\<ディストリビューション>\...` 形式に変換する。
fn build(p: &str, wsl_distro: Option<&str>, windows_opener: bool) -> String {
    let enc = |s: &str| percent_encode(s, windows_opener);
    if let Some(distro) = wsl_distro {
        if let Some(rest) = p.strip_prefix("/mnt/") {
            let mut it = rest.chars();
            if let Some(drive) = it.next().filter(|c| c.is_ascii_alphabetic()) {
                let tail = it.as_str();
                if tail.is_empty() || tail.starts_with('/') {
                    return format!("file:///{}:{}", drive.to_ascii_uppercase(), enc(tail));
                }
            }
        }
        return format!("file://wsl.localhost/{}{}", enc(distro), enc(p));
    }
    if p.starts_with('/') {
        format!("file://{}", enc(p))
    } else {
        // Windows のドライブ付きパス（C:/...）
        format!("file:///{}", enc(p))
    }
}

pub fn file_url(path: &Path) -> Option<String> {
    let abs = std::path::absolute(path).ok()?;
    let s = abs.to_string_lossy().replace('\\', "/");
    let distro = std::env::var("WSL_DISTRO_NAME")
        .ok()
        .filter(|d| !d.is_empty());
    // Windows（ネイティブ・WSL どちらも）では、開くのは Windows のシェルになる
    let windows_opener = cfg!(windows) || distro.is_some();
    Some(build(&s, distro.as_deref(), windows_opener))
}

/// text を url へのリンクで囲む。同じ url のリンクは id でひとまとまりに扱われる
/// （折り返した複数行のファイル名が、ホバー時に同時にハイライトされる）。
pub fn wrap(text: &str, url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    format!("\x1b]8;id={:x};{url}\x1b\\{text}\x1b]8;;\x1b\\", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_unix_and_windows() {
        assert_eq!(
            build("/home/user/a b.jpg", None, false),
            "file:///home/user/a%20b.jpg"
        );
        // Linux / macOS: 日本語は UTF-8 の %エンコード
        assert_eq!(
            build("/home/user/写真 (1).jpg", None, false),
            "file:///home/user/%E5%86%99%E7%9C%9F%20%281%29.jpg"
        );
        // Windows: 日本語は生の Unicode のまま（ShellExecute が %XX を ANSI で解釈するため）。ASCII の記号は %エンコード
        assert_eq!(
            build("C:/Users/user/写真 (1).jpg", None, true),
            "file:///C:/Users/user/写真%20%281%29.jpg"
        );
    }

    #[test]
    fn wsl_paths() {
        assert_eq!(
            build("/mnt/f/tools/gls/a.png", Some("Ubuntu"), true),
            "file:///F:/tools/gls/a.png"
        );
        assert_eq!(
            build("/mnt/c/Users/user/無題 1.png", Some("Ubuntu"), true),
            "file:///C:/Users/user/無題%201.png"
        );
        assert_eq!(build("/mnt/c", Some("Ubuntu"), true), "file:///C:");
        assert_eq!(
            build("/home/user/a.png", Some("Ubuntu-22.04"), true),
            "file://wsl.localhost/Ubuntu-22.04/home/user/a.png"
        );
        // /mnt/wsl/... はドライブではない
        assert_eq!(
            build("/mnt/wsl/x", Some("Ubuntu"), true),
            "file://wsl.localhost/Ubuntu/mnt/wsl/x"
        );
    }

    #[test]
    fn wrap_has_osc8() {
        let s = wrap("a.png", "file:///a.png");
        assert!(
            s.starts_with("\x1b]8;id=") && s.contains(";file:///a.png\x1b\\a.png\x1b]8;;\x1b\\")
        );
    }
}
