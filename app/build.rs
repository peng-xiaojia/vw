fn main() {
    let target = std::path::Path::new("ui/plugin-assets");
    std::fs::create_dir_all(target).expect("create plugin assets");
    std::fs::copy("../plugins/text/ui/formatter.mjs", target.join("text-formatter.mjs")).expect("copy text formatter");
    println!("cargo:rerun-if-changed=../plugins/text/ui/formatter.mjs");
    tauri_build::build()
}
