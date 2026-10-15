#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    std::panic::set_hook(Box::new(|panic| {
        vw_app::show_startup_error(&format!("未处理错误：{panic}"))
    }));
    if let Err(e) = vw_app::run_app() {
        vw_app::show_startup_error(&format!("无法启动文件查看器：{e}"));
        std::process::exit(1);
    }
}
