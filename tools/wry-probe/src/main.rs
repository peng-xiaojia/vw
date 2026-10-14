//! 最小 WebView2 探针：创建 tao 窗口 + wry WebView，打印创建结果与错误链。
//! 环境变量：PROBE_UDF（自定义数据目录）、PROBE_ARGS（浏览器附加参数）。

use std::error::Error;

use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{WebViewBuilder, WebViewBuilderExtWindows};

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().with_title("wry-probe").build(&event_loop).unwrap();

    let udf = std::env::var("PROBE_UDF").ok();
    let mut ctx = wry::WebContext::new(udf.map(std::path::PathBuf::from));
    let mut wb = WebViewBuilder::new_with_web_context(&mut ctx).with_url("data:text/html,<h1>probe ok</h1>");
    if let Ok(a) = std::env::var("PROBE_ARGS") {
        wb = wb.with_additional_browser_args(a);
    }

    match wb.build(&window) {
        Ok(_) => {
            println!("PROBE_WEBVIEW_OK");
            event_loop.run(move |_, _, cf| {
                *cf = ControlFlow::Exit;
            });
        }
        Err(e) => {
            println!("PROBE_WEBVIEW_ERR: {e:?}");
            let mut src: Option<&(dyn Error + 'static)> = Some(&e);
            while let Some(s) = src {
                println!("  cause: {s:?} / {s}");
                src = s.source();
            }
            std::process::exit(1);
        }
    }
}
