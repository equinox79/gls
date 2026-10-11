use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    // `gls --version` shows the build: "0.1.1 (e54d775 2026-10-11)". The hash is
    // "-dirty" when built from uncommitted changes, and omitted when there is no git.
    let version = env!("CARGO_PKG_VERSION");
    let build = git(&["rev-parse", "--short=7", "HEAD"]).map(|hash| {
        let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some();
        let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_default();
        format!(
            "{hash}{}{}{date}",
            if dirty { "-dirty" } else { "" },
            if date.is_empty() { "" } else { " " }
        )
    });
    let full = match build {
        Some(b) => format!("{version} ({b})"),
        None => version.to_string(),
    };
    println!("cargo:rustc-env=GLS_VERSION={full}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed=.git/{r}");
    }
}
