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

/// image モードでも、SVG のように作るのに時間のかかるものは縮小画像をキャッシュし、
/// 2回目は同じ出力になる
#[test]
fn image_mode_caches_svg_thumbnails() {
    let cache = std::env::temp_dir().join(format!("gls-cli-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let svg = data_dir().join("logo.svg");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_gls"))
            .env("GLS_LANG", "en")
            .env("XDG_CACHE_HOME", &cache)
            .env("LOCALAPPDATA", &cache)
            .args(["-m", "image", "--protocol", "sixel", "--cell-size", "10x20"])
            .arg(&svg)
            .arg(data_dir().join("gradient.png"))
            .output()
            .expect("gls を起動できる")
    };
    let count = |ext: &str| {
        let mut n = 0;
        let mut stack = vec![cache.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == ext) {
                    n += 1;
                }
            }
        }
        n
    };
    let first = run();
    assert!(first.status.success());
    assert_eq!(count("img"), 1, "SVG だけがキャッシュされる");
    let second = run();
    assert_eq!(
        first.stdout, second.stdout,
        "キャッシュから読んでも同じ出力"
    );
    assert_eq!(count("img"), 1);

    // 本当にキャッシュから読んでいる（毎回作り直していない）ことを、中身を差し替えて確かめる
    let img = std::fs::read_dir(cache.join("gls").join("cache"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "img"))
        .expect("キャッシュのファイルがある");
    image::RgbImage::from_pixel(30, 30, image::Rgb([255, 0, 0]))
        .save_with_format(&img, image::ImageFormat::Png)
        .unwrap();
    let third = run();
    assert_ne!(
        first.stdout, third.stdout,
        "差し替えたキャッシュが表示に使われる"
    );
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn cache_info_reports_without_creating_the_directory() {
    let cache = std::env::temp_dir().join(format!("gls-cli-info-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let run = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_gls"))
            .env("GLS_LANG", "en")
            .env("XDG_CACHE_HOME", &cache)
            .env("LOCALAPPDATA", &cache)
            .args(args)
            .output()
            .expect("gls を起動できる");
        assert!(o.status.success());
        stdout(&o)
    };
    let empty = run(&["--cache-info"]);
    assert!(empty.contains("Cache directory:"), "{empty}");
    assert!(empty.contains("Total: 0 files"), "{empty}");
    assert!(!cache.exists(), "情報の表示でディレクトリは作らない");

    run(&[
        "-m",
        "half",
        data_dir().join("gradient.png").to_str().unwrap(),
        data_dir().join("portrait.png").to_str().unwrap(),
    ]); // 2枚以上なら一覧表示になり、キャッシュされる（1枚だけの単体表示はされない）
    let after = run(&["--cache-info"]);
    assert!(
        after.contains("Rendered cells (text / half): 2 files"),
        "{after}"
    );
    assert!(after.contains("Last used:"), "{after}");
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn cache_limit_options_apply_now() {
    use std::time::{Duration, SystemTime};
    let cache = std::env::temp_dir().join(format!("gls-cli-limit-{}", std::process::id()));
    let dir = cache.join("gls").join("cache");
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::create_dir_all(&dir).unwrap();
    // 600KB のファイルを、古い順に3つ（合計 1.8MB）
    for (name, age_days) in [("a.txt", 3), ("b.txt", 2), ("c.txt", 1)] {
        let p = dir.join(name);
        std::fs::write(&p, vec![b'x'; 600 * 1024]).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(age_days * 24 * 3600))
            .unwrap();
    }
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_gls"))
            .env("GLS_LANG", "en")
            .env("XDG_CACHE_HOME", &cache)
            .env("LOCALAPPDATA", &cache)
            .args(args)
            .output()
            .expect("gls を起動できる")
    };

    // 表示だけなら、指定した上限が出る（整理はしない）
    let info = stdout(&run(&["--cache-info", "--cache-max-mb", "7"]));
    assert!(info.contains("limit 7.0MB"), "{info}");
    assert!(dir.join("a.txt").exists());

    // 0 は指定できない
    assert!(!run(&["--cache-max-mb", "0"]).status.success());

    // 1MB に収まるまで、古いものから消える
    let o = run(&[
        "--cache-max-mb",
        "1",
        "-m",
        "half",
        data_dir().join("gradient.png").to_str().unwrap(),
        data_dir().join("portrait.png").to_str().unwrap(),
    ]);
    assert!(o.status.success());
    assert!(!dir.join("a.txt").exists(), "最も古いものから消える");
    assert!(dir.join("c.txt").exists(), "新しいものは残る");
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn long_listing_has_one_line_per_file() {
    let dir = data_dir();
    let o = gls(&["-l", dir.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = stdout(&o);
    // 端末ではないので見出しは付かず、1ファイル1行
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "{out}");
    let row = lines.iter().find(|l| l.ends_with("gradient.png")).unwrap();
    for part in ["96x64", "3:2", "PNG RGBA 8bit"] {
        assert!(row.contains(part), "{part}: {row}");
    }
    assert!(lines
        .iter()
        .any(|l| l.contains("SVG") && l.ends_with("logo.svg")));
    // EXIF のない画像だけなので、カメラなどの列は出ない
    assert!(!out.contains("GPS"), "{out}");

    // 並び替え・件数の指定も効く
    let o = gls(&["-l", "--sort", "size", "-n", "1", dir.to_str().unwrap()]);
    let out = stdout(&o);
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("gradient.png"), "最も大きいファイル: {out}");
}

#[test]
fn missing_file_is_an_error() {
    let o = gls(&["no-such-file.png"]);
    assert!(!o.status.success());
}
