//! Message catalogs and language selection.
//!
//! Every user-facing message lives in `locales/<code>.txt` (`key = message`, `#` starts a comment,
//! `\n` is a line break, `{name}` is a placeholder). English is the reference: a missing key falls
//! back to English. To add a language, create `locales/<code>.txt` and add one line to `CATALOGS`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// (language code, catalog). Codes are lower case, e.g. `ja`, `zh-cn`.
const CATALOGS: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en.txt")),
    ("ja", include_str!("../locales/ja.txt")),
    ("zh-cn", include_str!("../locales/zh-cn.txt")),
    ("zh-tw", include_str!("../locales/zh-tw.txt")),
    ("ko", include_str!("../locales/ko.txt")),
    ("es", include_str!("../locales/es.txt")),
    ("fr", include_str!("../locales/fr.txt")),
    ("de", include_str!("../locales/de.txt")),
    ("pt-br", include_str!("../locales/pt-br.txt")),
    ("ru", include_str!("../locales/ru.txt")),
];
/// Other names for a language that has a catalog (lower case): script and region variants.
const ALIASES: &[(&str, &str)] = &[
    ("zh", "zh-cn"),
    ("zh-sg", "zh-cn"),
    ("zh-hans", "zh-cn"),
    ("zh-hk", "zh-tw"),
    ("zh-mo", "zh-tw"),
    ("zh-hant", "zh-tw"),
    ("pt", "pt-br"),
    ("pt-pt", "pt-br"),
];
const DEFAULT: &str = "en";

type Catalog = HashMap<String, String>;

struct State {
    lang: &'static str,
    cur: Catalog,
    en: Catalog,
}

static STATE: OnceLock<State> = OnceLock::new();

/// `t!("key")` gives the message as `&'static str`;
/// `t!("key", name = value, ...)` fills the `{name}` placeholders and gives a `String`.
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::fmt($key, &[$((stringify!($k), ($v).to_string())),+])
    };
}

fn parse(src: &str) -> Catalog {
    src.lines()
        .filter_map(|l| {
            let l = l.trim_start_matches('\u{feff}').trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().replace("\\n", "\n")))
        })
        .collect()
}

fn catalog(code: &str) -> Catalog {
    CATALOGS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, src)| parse(src))
        .unwrap_or_default()
}

/// A locale name (`ja_JP.UTF-8`, `ja-JP`, `zh_CN`, ...) to an available language code.
/// `zh-cn` is tried first, then `zh`.
fn resolve(tag: &str) -> Option<&'static str> {
    let t = tag
        .split(['.', '@'])
        .next()?
        .replace('_', "-")
        .to_ascii_lowercase();
    let mut cur = t.as_str();
    loop {
        let name = ALIASES
            .iter()
            .find(|(a, _)| *a == cur)
            .map_or(cur, |(_, c)| *c);
        if let Some((code, _)) = CATALOGS.iter().find(|(c, _)| *c == name) {
            return Some(code);
        }
        cur = cur.rsplit_once('-')?.0;
    }
}

/// Whether `--lang` / `GLS_LANG` names a language we have a catalog for.
pub fn is_available(tag: &str) -> bool {
    resolve(tag).is_some()
}

/// Codes of the available languages.
pub fn available() -> Vec<&'static str> {
    CATALOGS.iter().map(|(c, _)| *c).collect()
}

/// The language in use (after `init`).
pub fn current() -> &'static str {
    state().lang
}

/// Locale names from the environment, most specific first. `C` / `POSIX` mean English.
fn env_candidates() -> Vec<String> {
    ["GLS_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| match v.split('.').next() {
            Some("C" | "POSIX") => DEFAULT.to_string(),
            _ => v,
        })
        .collect()
}

/// The user's preferred display languages on Windows (`ja-JP`, `en-US`, ...).
#[cfg(windows)]
fn os_candidates() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut num = 0u32;
    let mut len = 0u32;
    // SAFETY: the buffer is sized by the first call, which asks only for the required length.
    unsafe {
        let ok = GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut num,
            std::ptr::null_mut(),
            &mut len,
        );
        if ok == 0 || len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut num, buf.as_mut_ptr(), &mut len) == 0
        {
            return Vec::new();
        }
        buf.split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

#[cfg(not(windows))]
fn os_candidates() -> Vec<String> {
    Vec::new()
}

fn build(flag: Option<&str>) -> State {
    let lang = flag
        .map(String::from)
        .into_iter()
        .chain(env_candidates())
        .chain(os_candidates())
        .find_map(|c| resolve(&c))
        .unwrap_or(DEFAULT);
    State {
        lang,
        cur: catalog(lang),
        en: catalog(DEFAULT),
    }
}

/// Choose the language: `--lang` first, then `GLS_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG`,
/// then the OS display language (Windows), then English. Call once, before any message is used.
pub fn init(flag: Option<&str>) {
    STATE.get_or_init(|| build(flag));
}

fn state() -> &'static State {
    STATE.get_or_init(|| build(None))
}

/// The message for a key, or the key itself if it is in no catalog.
pub fn tr(key: &'static str) -> &'static str {
    find(key).unwrap_or(key)
}

/// The message for a key that is built at run time (e.g. `help.<arg>`).
pub fn find(key: &str) -> Option<&'static str> {
    let s = state();
    s.cur.get(key).or_else(|| s.en.get(key)).map(String::as_str)
}

/// `tr` with `{name}` placeholders filled in.
pub fn fmt(key: &'static str, args: &[(&str, String)]) -> String {
    let mut s = tr(key).to_string();
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            let Some(j) = rest[i..].find('}') else { break };
            out.insert(rest[i + 1..i + j].to_string());
            rest = &rest[i + j + 1..];
        }
        out
    }

    #[test]
    fn resolve_locale_names() {
        assert_eq!(resolve("ja_JP.UTF-8"), Some("ja"));
        assert_eq!(resolve("ja-JP"), Some("ja"));
        assert_eq!(resolve("JA"), Some("ja"));
        assert_eq!(resolve("en_US.UTF-8"), Some("en"));
        assert_eq!(resolve("fr_FR"), Some("fr"));
        assert_eq!(resolve("pt_BR.UTF-8"), Some("pt-br"));
        assert_eq!(resolve("pt-PT"), Some("pt-br"));
        assert_eq!(resolve("zh_CN.UTF-8"), Some("zh-cn"));
        assert_eq!(resolve("zh-Hans-CN"), Some("zh-cn"));
        assert_eq!(resolve("zh_TW"), Some("zh-tw"));
        assert_eq!(resolve("zh-Hant-TW"), Some("zh-tw"));
        assert_eq!(resolve("zh-HK"), Some("zh-tw"));
        assert_eq!(resolve("zh"), Some("zh-cn"));
        assert_eq!(resolve("ru_RU@euro"), Some("ru"));
        assert_eq!(resolve("sv_SE"), None);
        assert_eq!(resolve(""), None);
    }

    #[test]
    fn catalogs_match_english() {
        let en = catalog(DEFAULT);
        assert!(!en.is_empty());
        for (code, _) in CATALOGS {
            let cat = catalog(code);
            for key in cat.keys() {
                assert!(en.contains_key(key), "{code}: unknown key `{key}`");
            }
            for (key, msg) in &en {
                let Some(m) = cat.get(key) else {
                    panic!("{code}: missing key `{key}`");
                };
                assert!(!m.is_empty(), "{code}: empty message for `{key}`");
                assert_eq!(
                    placeholders(m),
                    placeholders(msg),
                    "{code}: placeholders of `{key}` differ from English"
                );
            }
        }
    }

    #[test]
    fn every_locale_file_is_registered() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "txt") {
                let code = p.file_stem().unwrap().to_string_lossy().to_string();
                assert!(
                    CATALOGS.iter().any(|(c, _)| *c == code),
                    "locales/{code}.txt is not listed in CATALOGS"
                );
            }
        }
    }

    #[test]
    fn keys_used_in_source_exist() {
        let en = catalog(DEFAULT);
        let re = regex::Regex::new(r#"\b(?:t!|tr|fmt)\(\s*"([^"]+)""#).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.extension().is_none_or(|e| e != "rs") || p.ends_with("i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&p).unwrap();
            for cap in re.captures_iter(&src) {
                assert!(
                    en.contains_key(&cap[1]),
                    "{}: key `{}` is not in locales/en.txt",
                    p.display(),
                    &cap[1]
                );
            }
        }
    }
}
