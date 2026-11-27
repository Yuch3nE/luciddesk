use std::path::PathBuf;
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let check = std::env::args().any(|arg| arg == "--check");
    let output = root.join("app/src/pane/native_graphics/bindings");
    let temporary = root.join("target/bindings-check");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::create_dir_all(&temporary).unwrap();
    for (name, style) in [("dwm", "--sys"), ("dcomp", "--minimal")] {
        let path = temporary.join(format!("{name}.rs"));
        let filters = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("{name}.txt")),
        )
        .unwrap();
        let mut args = vec!["--out", path.to_str().unwrap(), "--flat", style, "--filter"];
        args.extend(filters.lines().filter(|line| !line.is_empty()));
        windows_bindgen::bindgen(args);
        let generated = std::fs::read_to_string(&path).unwrap();
        // DComp macros resolve windows_core in the consuming app. In 0.62,
        // Type is exported at the root, while Bindgen 0.100 emits it under imp.
        let generated = if name == "dcomp" {
            generated.replace("windows_core::imp::Type::", "windows_core::Type::")
        } else {
            generated
        };
        let destination = output.join(format!("{name}.rs"));
        if check {
            assert_eq!(
                std::fs::read(&destination).unwrap(),
                generated.as_bytes(),
                "stale {name} bindings"
            );
        } else {
            std::fs::write(&destination, &generated).unwrap();
        }
        println!(
            "{name}: {} bytes ({})",
            generated.len(),
            if check { "verified" } else { "generated" }
        );
    }
}
