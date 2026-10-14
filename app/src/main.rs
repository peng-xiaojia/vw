fn main() {
    if let Err(e) = vw_app::run_app() {
        eprintln!("vw-app 退出：{e}");
        std::process::exit(1);
    }
}
