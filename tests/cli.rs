//! コマンドライン全体の動作テスト。`tests/data` の小さな合成画像を、実際のバイナリで表示する。

use std::path::PathBuf;
use std::process::{Command, Output};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// 端末ではない出力（パイプ）なので、ページャ・インジケータ・リンクは自動で無効になる。
/// キャッシュは使わず、文字描画（モノクロ）にして、出力を固定する。
fn gls(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gls"))
        .env("GLS_LANG", "en")
        .args(["--no-cache", "--no-color", "-m", "text"])
        .args(args)
        .output()
        .expect("gls を起動できる")
}

/// 言語の指定だけを変えて実行する（環境の言語設定に左右されないよう、他の指定は外す）
fn gls_lang(env_lang: Option<&str>, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gls"));
    for k in ["GLS_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        c.env_remove(k);
    }
    if let Some(l) = env_lang {
        c.env("GLS_LANG", l);
    }
    c.args(args).output().expect("gls を起動できる")
}

#[test]
fn help_follows_the_language() {
    let en = stdout(&gls_lang(None, &["--lang", "en", "--help"]));
    assert!(en.contains("Filter by file name"), "{en}");
    let ja = stdout(&gls_lang(None, &["--lang=ja", "--help"]));
    assert!(ja.contains("ファイル名"), "{ja}");
    // 環境変数でも切り替わる。--lang が優先
    let ja = stdout(&gls_lang(Some("ja_JP.UTF-8"), &["--help"]));
    assert!(ja.contains("ファイル名"), "{ja}");
    let en = stdout(&gls_lang(Some("ja"), &["--lang", "en", "--help"]));
    assert!(en.contains("Filter by file name"), "{en}");
}

#[test]
fn messages_follow_the_language() {
    let none = data_dir().join("no-such-dir-*.png");
    let none = none.to_str().unwrap();
    let err = |o: &Output| String::from_utf8_lossy(&o.stderr).into_owned();
    let en = err(&gls_lang(Some("en"), &[none]));
    assert!(en.contains("No image files found"), "{en}");
    let ja = err(&gls_lang(Some("ja"), &[none]));
    assert!(ja.contains("画像ファイルが見つかりません"), "{ja}");
}

#[test]
fn unknown_language_warns_and_falls_back() {
    let none = data_dir().join("no-such-dir-*.png");
    let o = gls_lang(Some("en"), &["--lang", "xx", none.to_str().unwrap()]);
    let e = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(e.contains("`xx` is not available"), "{e}");
    assert!(e.contains("No image files found"), "{e}");
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn lists_all_images_with_names() {
    let o = gls(&[data_dir().to_str().unwrap(), "-w", "60"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = stdout(&o);
    for name in ["gradient.png", "photo.jpg", "portrait.png", "logo.svg"] {
        assert!(out.contains(name), "{name} が一覧に出る:\n{out}");
    }
}

#[test]
fn regexp_filters_by_file_name() {
    let dir = data_dir();
    let o = gls(&[dir.to_str().unwrap(), "-e", r"\.png$", "-v"]);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(out.contains("gradient.png") && out.contains("portrait.png"));
    assert!(!out.contains("photo.jpg") && !out.contains("logo.svg"));
}

#[test]
fn no_match_and_bad_regexp_fail() {
    let dir = data_dir();
    let o = gls(&[dir.to_str().unwrap(), "-e", "no-such-file"]);
    assert!(!o.status.success());
    let o = gls(&[dir.to_str().unwrap(), "-e", "("]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("regular expression"));
}

#[test]
fn head_limits_count_and_sort_orders() {
    let dir = data_dir();
    // 名前順の先頭は gradient.png、逆順の先頭は portrait.png。1枚だけなら単体表示（-v で名前を出す）
    let first = stdout(&gls(&[dir.to_str().unwrap(), "-n", "1", "-v"]));
    assert!(first.contains("gradient.png"), "{first}");
    let last = stdout(&gls(&[dir.to_str().unwrap(), "-r", "-n", "1", "-v"]));
    assert!(last.contains("portrait.png"), "{last}");
    // 大きい順の先頭は、いちばんファイルサイズの大きい画像
    let biggest = std::fs::read_dir(dir.clone())
        .unwrap()
        .flatten()
        .max_by_key(|e| e.metadata().unwrap().len())
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    let by_size = stdout(&gls(&[
        dir.to_str().unwrap(),
        "--sort",
        "size",
        "-n",
        "1",
        "-v",
    ]));
    assert!(by_size.contains(&biggest), "{biggest} が先頭:\n{by_size}");
}

#[test]
fn single_image_and_info_lines() {
    let png = data_dir().join("gradient.png");
    let o = gls(&[png.to_str().unwrap(), "-vv", "-w", "30"]);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(out.contains("96x64"), "{out}");
    assert!(out.contains("PNG"), "{out}");
}

#[test]
fn recursive_adds_parent_directory() {
    // tests 直下には画像がなく、tests/data の中にだけある
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let flat = gls(&[tests.to_str().unwrap()]);
    assert!(!flat.status.success(), "再帰なしでは画像が見つからない");
    let o = gls(&[tests.to_str().unwrap(), "-R", "-v"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout(&o).contains("data/"), "親ディレクトリ名が付く");
}

#[test]
fn missing_file_is_an_error() {
    let o = gls(&["no-such-file.png"]);
    assert!(!o.status.success());
}
