fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    for path in [
        "app/src",
        "app/updater",
        "app/assets",
        "app/build.rs",
        "app/Cargo.toml",
        "Cargo.toml",
        "Cargo.lock",
        "README.md",
        "README.en.md",
        "CHANGELOG.md",
        "CHANGELOG.en.md",
        "tools",
        ".github",
        "crates",
        "docs",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    println!("cargo:rerun-if-env-changed=GIT");
    let git_executable = std::env::var_os("GIT")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            // Cargo can replace PATH on Windows; also check standard Git installs.
            let path = std::env::var_os("PATH").unwrap_or_default();
            let name = if cfg!(windows) { "git.exe" } else { "git" };
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .chain(if cfg!(windows) {
                    [
                        ("ProgramFiles", "Git/cmd/git.exe"),
                        ("LOCALAPPDATA", "Programs/Git/cmd/git.exe"),
                    ]
                    .into_iter()
                    .filter_map(|(key, suffix)| {
                        std::env::var_os(key).map(|dir| std::path::PathBuf::from(dir).join(suffix))
                    })
                    .collect::<Vec<_>>()
                } else {
                    Vec::new()
                })
                .find(|path| path.is_file())
        })
        .unwrap_or_else(|| "git".into());
    let git = |args: &[&str]| {
        std::process::Command::new(&git_executable)
            .args(args)
            .current_dir(&root)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    // Resolve Git metadata through Git itself: a worktree's .git is a file.
    for path in ["HEAD", "refs", "packed-refs", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    println!("cargo:rerun-if-env-changed=LUCIDDESK_BUILD_REVISION");
    let revision = std::env::var("LUCIDDESK_BUILD_REVISION")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value != "unknown")
        .or_else(|| {
            let revision = git(&["rev-parse", "--short", "HEAD"])?;
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|status| !status.is_empty());
            Some(format!("{revision}{}", if dirty { "-dirty" } else { "" }))
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=LUCIDDESK_BUILD_REVISION={revision}");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let numeric_version = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| {
            std::env::var(format!("CARGO_PKG_VERSION_{part}"))
                .unwrap()
                .parse::<u16>()
                .expect("Windows version components must fit in 16 bits")
                .to_string()
        })
        .join(",");
    let resource = std::fs::read_to_string("assets/app.rc")
        .expect("failed to read application resource template")
        .replace("@VERSION_NUMERIC@", &format!("{numeric_version},0"))
        .replace("@VERSION_STRING@", &version);
    let resource_path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("app.rc");
    for (bin, description) in [("luciddesk", "LucidDesk"), ("updater", "LucidDesk Updater")] {
        let rc = resource.replace("@BINARY@", bin).replace("@DESCRIPTION@", description);
        compile_resources(&resource_path.with_file_name(format!("{bin}.rc")), &rc, bin);
    }
    // Native search controls and the backdrop fixture use Explorer's v6 controls.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("../crates/luciddesk-explorer/native-controls.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-bin=updater=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bin=updater=/MANIFESTUAC:level='asInvoker'");
        println!("cargo:rustc-link-arg-bin=luciddesk=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=luciddesk=/MANIFESTINPUT:{}",
            manifest.display()
        );
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}

fn compile_resources(path: &std::path::Path, resource: &str, bin: &str) {
    // embed-resource 3.x emits app.lib for native MSVC. Keep other hosts/targets
    // on its normal path, and only cache with the selected toolchain recorded.
    let target = std::env::var("TARGET").unwrap();
    let environment = std::env::var("LUCIDDESK_WINDOWS_BUILD_ENVIRONMENT").unwrap_or_default();
    let mut inputs = Vec::new();
    for value in [
        resource.as_bytes().to_vec(),
        std::fs::read("assets/luciddesk.ico").expect("failed to read icon"),
        std::fs::read("build.rs").expect("failed to read build script"),
        std::fs::read("../Cargo.lock").expect("failed to read Cargo.lock"),
    ] {
        inputs.extend_from_slice(&(value.len() as u64).to_le_bytes());
        inputs.extend_from_slice(&value);
    }
    let mut custom_rc = false;
    for name in [
        "LUCIDDESK_WINDOWS_BUILD_ENVIRONMENT".to_owned(),
        "INCLUDE".to_owned(),
        "RC".to_owned(),
        format!("RC_{target}"),
        format!("RC_{}", target.replace('-', "_")),
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var_os(&name).unwrap_or_default();
        custom_rc |= name.starts_with("RC") && !value.is_empty();
        let bytes = value.as_encoded_bytes();
        inputs.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        inputs.extend_from_slice(bytes);
    }
    let stamp = path.with_extension("inputs");
    let output = path.with_extension("lib");
    let cacheable = cfg!(all(windows, target_env = "msvc"))
        && std::env::var("HOST").as_deref() == Ok(target.as_str())
        && target.ends_with("-msvc")
        && !environment.is_empty()
        && !custom_rc;
    if cacheable && output.is_file() && std::fs::read(&stamp).ok().as_deref() == Some(&inputs) {
        println!("cargo:rustc-link-arg-bin={bin}={}", output.display());
        return;
    }
    // Invalidate before compiling so a failed compiler cannot leave a valid stamp.
    if stamp.exists() {
        std::fs::remove_file(&stamp).expect("failed to invalidate resource cache");
    }
    std::fs::write(path, resource).expect("failed to write application resources");
    embed_resource::compile_for(path, [bin], embed_resource::NONE)
        .manifest_required()
        .expect("failed to embed the LucidDesk application resources");
    if cacheable {
        std::fs::write(stamp, inputs).expect("failed to record resource inputs");
    }
}
