//! 使用 Tauri/muda 原生菜单，键盘导航与助记键由系统处理。
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let item = |id: &str, label: &str, key: &str| MenuItemBuilder::with_id(id, label).accelerator(key).build(app);
    let file = SubmenuBuilder::new(app, "文件(&F)")
        .item(&item("file", "打开文件…", "CmdOrCtrl+O")?)
        .item(&item("folder", "打开文件夹…", "CmdOrCtrl+Shift+O")?)
        .text("associations", "关联格式…")
        .separator().item(&item("save", "保存", "CmdOrCtrl+S")?)
        .text("export", "导出副本…")
        .text("otherApp", "用其他软件打开…").text("terminal", "在终端打开").separator()
        .item(&item("close", "关闭文档", "CmdOrCtrl+W")?)
        .text("cancel", "取消打开").separator().text("quit", "退出").build()?;
    let edit = SubmenuBuilder::new(app, "编辑(&E)").text("edit", "编辑文档").text("preview", "预览文档")
        .separator().item(&item("format", "格式化文档", "Shift+Alt+F")?)
        .item(&item("replace", "查找 / 替换…", "CmdOrCtrl+H")?).build()?;
    let view = SubmenuBuilder::new(app, "视图(&V)")
        .item(&item("sidebar", "显示 / 隐藏侧栏", "CmdOrCtrl+B")?)
        .text("files", "文件导航").text("outline", "文档目录")
        .item(&item("search", "文档内搜索", "CmdOrCtrl+F")?)
        .separator().text("lineNumbers", "显示 / 隐藏行号")
        .item(&item("lineWrapping", "自动换行", "Alt+Z")?)
        .text("binary", "以二进制查看").text("normal", "默认格式查看").text("text", "以文本查看")
        .separator().text("appearance", "外观设置…").build()?;
    let plugins = SubmenuBuilder::new(app, "插件(&P)").text("plugins", "插件中心…").build()?;
    let help = SubmenuBuilder::new(app, "帮助(&H)").text("about", "关于熊猫查看器…").build()?;
    app.set_menu(MenuBuilder::new(app).items(&[&file, &edit, &view, &plugins, &help]).build()?)?;
    update(app.handle(), &[ ("save".into(), false), ("export".into(), false), ("close".into(), false), ("edit".into(), false), ("preview".into(), false), ("cancel".into(), false), ("otherApp".into(), false), ("terminal".into(), false)])?;
    Ok(())
}

fn update(app: &tauri::AppHandle, states: &[(String, bool)]) -> tauri::Result<()> {
    if let Some(menu) = app.menu() {
        for top in menu.items()? {
            if let Some(submenu) = top.as_submenu() {
                for (id, enabled) in states {
                    if let Some(item) = submenu.get(id.as_str()) {
                        if let Some(normal) = item.as_menuitem() { normal.set_enabled(*enabled)?; }
                    }
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn vw_menu_state(app: tauri::AppHandle, states: Vec<(String, bool)>) -> Result<(), String> {
    update(&app, &states).map_err(|e| e.to_string())
}
