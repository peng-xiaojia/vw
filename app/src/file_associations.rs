use serde::Serialize;
use std::collections::HashSet;

const GROUPS: &[(&str, &[&str])] = &[
    ("文本与数据", &["txt", "log", "md", "markdown", "html", "htm", "json", "jsonl", "csv", "tsv", "ini", "toml", "cfg"]),
    ("代码与脚本", &["sh", "bash", "bat", "cmd", "ps1", "js", "mjs", "cjs", "jsx", "ts", "tsx", "py", "rs", "go", "c", "h", "cpp", "java", "sql", "xml", "css", "yaml", "yml"]),
    ("归档", &["zip"]),
    ("图片", &["png", "jpg", "jpeg", "gif", "webp", "bmp"]),
    ("文档", &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx"]),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationState {
    formats: Vec<Format>,
    selected: Vec<String>,
    registration: &'static str,
}

#[derive(Serialize)]
struct Format { extension: &'static str, group: &'static str }

fn formats() -> Vec<Format> {
    GROUPS.iter().flat_map(|(group, extensions)| extensions.iter().map(move |extension| Format { extension, group })).collect()
}

fn validate_selection(selected: Vec<String>) -> Result<Vec<String>, String> {
    let allowed: HashSet<&str> = GROUPS.iter().flat_map(|(_, extensions)| extensions.iter().copied()).collect();
    let mut seen = HashSet::new();
    for extension in &selected {
        if !allowed.contains(extension.as_str()) || !seen.insert(extension.as_str()) {
            return Err(format!("不支持或重复的文件格式：{extension}"));
        }
    }
    Ok(selected)
}

#[cfg(windows)]
fn command_for(exe: &std::path::Path) -> String { format!("\"{}\" \"%1\"", exe.display()) }

#[cfg(windows)]
fn registry_error(step: &str, error: std::io::Error) -> String {
    let detail = format!("{step}：{error}");
    if error.raw_os_error() == Some(5) {
        format!("{detail}。当前进程无权修改 Windows 文件关联；若从受限开发工作区运行，请将便携 ZIP 解压到下载等普通用户目录后运行，或使用安装版。")
    } else { detail }
}

#[cfg(windows)]
fn read_at(hkcu: &winreg::RegKey, base: &str, exe: &std::path::Path) -> Result<AssociationState, String> {
    use std::io::ErrorKind;
    let app = match hkcu.open_subkey(base) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(AssociationState { formats: formats(), selected: vec![], registration: "none" }),
        Err(error) => return Err(error.to_string()),
    };
    let command = match app.open_subkey("shell\\open\\command").and_then(|key| key.get_value::<String, _>("")) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(AssociationState { formats: formats(), selected: vec![], registration: "other" }),
        Err(error) => return Err(error.to_string()),
    };
    if !command.eq_ignore_ascii_case(&command_for(exe)) {
        return Ok(AssociationState { formats: formats(), selected: vec![], registration: "other" });
    }
    let types = match app.open_subkey("SupportedTypes") {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(AssociationState { formats: formats(), selected: vec![], registration: "current" }),
        Err(error) => return Err(error.to_string()),
    };
    let mut selected = Vec::new();
    for format in formats() {
        let extension = format!(".{}", format.extension);
        match types.get_value::<String, _>(&extension) {
            Ok(_) => selected.push(format.extension.to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(AssociationState { formats: formats(), selected, registration: "current" })
}

#[cfg(windows)]
fn apply_at(hkcu: &winreg::RegKey, base: &str, exe: &std::path::Path, selected: &[String]) -> Result<(), String> {
    use winreg::enums::{KEY_READ, KEY_WRITE};
    const ACCESS: u32 = KEY_READ | KEY_WRITE;
    let write_error = registry_error;
    let state = read_at(hkcu, base, exe)?;
    if state.registration == "other" { return Err("另一份 vw 已注册打开方式，请先移除旧版本的关联。".into()); }
    if selected.is_empty() {
        if state.registration == "current" { hkcu.delete_subkey_all(base).map_err(|error| write_error("移除关联注册", error))?; }
        return Ok(());
    }
    let (app, _) = hkcu.create_subkey_with_flags(base, ACCESS).map_err(|error| write_error("创建关联注册", error))?;
    app.set_value("FriendlyAppName", &"熊猫查看器").map_err(|error| write_error("写入应用名称", error))?;
    let (command, _) = app.create_subkey_with_flags("shell\\open\\command", ACCESS).map_err(|error| write_error("创建打开命令", error))?;
    command.set_value("", &command_for(exe)).map_err(|error| write_error("写入打开命令", error))?;
    if app.open_subkey("SupportedTypes").is_ok() { app.delete_subkey_all("SupportedTypes").map_err(|error| write_error("更新格式列表", error))?; }
    let (types, _) = app.create_subkey_with_flags("SupportedTypes", ACCESS).map_err(|error| write_error("创建格式列表", error))?;
    for extension in selected { types.set_value(format!(".{extension}"), &"").map_err(|error| write_error(&format!("注册 .{extension}"), error))?; }
    Ok(())
}

#[cfg(windows)]
fn apply_selection_at(hkcu: &winreg::RegKey, base: &str, settings_base: &str, exe: &std::path::Path, selected: &[String]) -> Result<AssociationState, String> {
    apply_at(hkcu, base, exe, selected)?;
    let settings_root = format!("{settings_base}\\AssociationSelection");
    if hkcu.open_subkey(&settings_root).is_ok() { hkcu.delete_subkey_all(&settings_root).map_err(|error| registry_error("更新关联选择", error))?; }
    let (selection, _) = hkcu.create_subkey_with_flags(&settings_root, winreg::enums::KEY_WRITE).map_err(|error| registry_error("创建关联选择", error))?;
    for extension in selected { selection.set_value(format!(".{extension}"), &1u32).map_err(|error| registry_error("保存关联选择", error))?; }
    let (settings, _) = hkcu.create_subkey_with_flags(settings_base, winreg::enums::KEY_WRITE).map_err(|error| registry_error("保存关联配置", error))?;
    settings.set_value("FileAssociationsConfigured", &1u32).map_err(|error| registry_error("保存关联配置", error))?;
    read_at(hkcu, base, exe)
}

#[cfg(windows)]
fn register_default_app_at(hkcu: &winreg::RegKey, exe: &std::path::Path, selected: &[String], capabilities: &str, registered: &str, prog_id: &str) -> Result<(), String> {
    use winreg::enums::{KEY_READ, KEY_WRITE};
    const ACCESS: u32 = KEY_READ | KEY_WRITE;
    if selected.is_empty() {
        if hkcu.open_subkey(capabilities).is_ok() { hkcu.delete_subkey_all(capabilities).map_err(|e| registry_error("移除默认应用能力", e))?; }
        if hkcu.open_subkey(prog_id).is_ok() { hkcu.delete_subkey_all(prog_id).map_err(|e| registry_error("移除格式处理程序", e))?; }
        if let Ok(key) = hkcu.open_subkey_with_flags(registered, ACCESS) { let _ = key.delete_value("vw"); }
        return Ok(());
    }
    let (app, _) = hkcu.create_subkey_with_flags(capabilities, ACCESS).map_err(|e| registry_error("创建默认应用能力", e))?;
    app.set_value("ApplicationName", &"熊猫查看器").map_err(|e| registry_error("注册应用名称", e))?;
    app.set_value("ApplicationDescription", &"本地文件查看器").map_err(|e| registry_error("注册应用说明", e))?;
    if app.open_subkey("FileAssociations").is_ok() { app.delete_subkey_all("FileAssociations").map_err(|e| registry_error("更新默认格式", e))?; }
    let (types, _) = app.create_subkey_with_flags("FileAssociations", ACCESS).map_err(|e| registry_error("创建默认格式", e))?;
    for extension in selected { types.set_value(format!(".{extension}"), &"vw.file").map_err(|e| registry_error("注册默认格式", e))?; }
    let (handler, _) = hkcu.create_subkey_with_flags(prog_id, ACCESS).map_err(|e| registry_error("创建格式处理程序", e))?;
    handler.set_value("", &"vw 文件").map_err(|e| registry_error("命名格式处理程序", e))?;
    let (command, _) = handler.create_subkey_with_flags("shell\\open\\command", ACCESS).map_err(|e| registry_error("创建格式打开命令", e))?;
    command.set_value("", &command_for(exe)).map_err(|e| registry_error("注册格式打开命令", e))?;
    let (list, _) = hkcu.create_subkey_with_flags(registered, ACCESS).map_err(|e| registry_error("注册默认应用", e))?;
    list.set_value("vw", &capabilities).map_err(|e| registry_error("注册默认应用", e))?;
    Ok(())
}

#[cfg(windows)]
fn register_default_app(hkcu: &winreg::RegKey, exe: &std::path::Path, selected: &[String]) -> Result<(), String> {
    register_default_app_at(hkcu, exe, selected, "Software\\vw\\Capabilities", "Software\\RegisteredApplications", "Software\\Classes\\vw.file")
}

#[tauri::command]
pub fn vw_association_state() -> Result<AssociationState, String> {
    #[cfg(windows)] {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        read_at(&hkcu, "Software\\Classes\\Applications\\vw-app.exe", &exe)
    }
    #[cfg(not(windows))] { Err("文件关联设置仅支持 Windows。".into()) }
}

#[tauri::command]
pub fn vw_association_apply(selected: Vec<String>) -> Result<AssociationState, String> {
    let selected = validate_selection(selected)?;
    #[cfg(windows)] {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let base = "Software\\Classes\\Applications\\vw-app.exe";
        let state = apply_selection_at(&hkcu, base, "Software\\vw", &exe, &selected)?;
        register_default_app(&hkcu, &exe, &selected)?;
        unsafe { windows_sys::Win32::UI::Shell::SHChangeNotify(windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32, windows_sys::Win32::UI::Shell::SHCNF_IDLIST, std::ptr::null(), std::ptr::null()); }
        Ok(state)
    }
    #[cfg(not(windows))] { let _ = selected; Err("文件关联设置仅支持 Windows。".into()) }
}

#[cfg(any(windows, test))]
fn open_default_apps_with(explorer: &str, mut launch: impl FnMut(&str, Option<&str>) -> isize) -> Result<(), String> {
    const DIRECT: &str = "ms-settings:defaultapps?registeredAppUser=vw";
    let direct = launch(DIRECT, None);
    if direct > 32 { return Ok(()); }
    let broker = launch(explorer, Some(DIRECT));
    if broker > 32 { return Ok(()); }
    let fallback = launch("ms-settings:defaultapps", None);
    if fallback > 32 { Ok(()) } else { Err(format!("无法打开 Windows 默认应用设置（直达返回 {direct}，Explorer 返回 {broker}，设置主页返回 {fallback}）。")) }
}

#[tauri::command]
pub fn vw_association_defaults() -> Result<(), String> {
    #[cfg(windows)] {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let state = read_at(&hkcu, "Software\\Classes\\Applications\\vw-app.exe", &exe)?;
        if state.registration == "current" { register_default_app(&hkcu, &exe, &state.selected)?; }
        fn launch(file: &str, parameters: Option<&str>) -> isize {
            let wide = |value: &str| value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let file = wide(file);
            let parameters = parameters.map(wide);
            let args = parameters.as_ref().map_or(std::ptr::null(), |value| value.as_ptr());
            unsafe { windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), std::ptr::null(), file.as_ptr(), args, std::ptr::null(), 1) as isize }
        }
        let mut directory = vec![0u16; 32768];
        let length = unsafe { windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW(directory.as_mut_ptr(), directory.len() as u32) } as usize;
        if length == 0 || length >= directory.len() { return Err("无法确定 Windows 系统目录。".into()); }
        let explorer = std::path::PathBuf::from(String::from_utf16_lossy(&directory[..length])).join("explorer.exe");
        open_default_apps_with(&explorer.to_string_lossy(), launch)
    }
    #[cfg(not(windows))] { Err("默认应用设置仅支持 Windows。".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_accepts_supported_formats_without_duplicates() {
        assert_eq!(validate_selection(vec!["txt".into(), "pdf".into()]).unwrap(), vec!["txt", "pdf"]);
        assert!(validate_selection(vec!["txt".into(), "txt".into()]).is_err());
        assert!(validate_selection(vec!["exe".into()]).is_err());
        assert!(validate_selection(vec![".txt".into()]).is_err());
    }

    #[test]
    fn settings_uri_uses_absolute_explorer_fallback_and_reports_both_failures() {
        let mut calls = Vec::new();
        open_default_apps_with(r"C:\Windows\explorer.exe", |file, args| {
            calls.push((file.to_string(), args.map(str::to_string)));
            if args.is_none() { 5 } else { 33 }
        }).unwrap();
        assert_eq!(calls, vec![("ms-settings:defaultapps?registeredAppUser=vw".into(), None), (r"C:\Windows\explorer.exe".into(), Some("ms-settings:defaultapps?registeredAppUser=vw".into()))]);
        let error = open_default_apps_with(r"C:\Windows\explorer.exe", |_, args| if args.is_none() { 5 } else { 2 }).unwrap_err();
        assert!(error.contains("直达返回 5，Explorer 返回 2"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires permission to write an isolated HKCU test key"]
    fn isolated_registration_only_changes_selected_types() {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let base = format!("Software\\vw\\Tests\\Association-{}", std::process::id());
        let app_base = format!("{base}\\Application");
        let settings_base = format!("{base}\\Settings");
        let exe = std::env::current_exe().unwrap();
        let selected = vec!["txt".to_string(), "pdf".to_string()];
        let state = apply_selection_at(&hkcu, &app_base, &settings_base, &exe, &selected).unwrap();
        assert_eq!(state.registration, "current");
        assert_eq!(state.selected, selected);
        apply_selection_at(&hkcu, &app_base, &settings_base, &exe, &["pdf".to_string()]).unwrap();
        assert_eq!(read_at(&hkcu, &app_base, &exe).unwrap().selected, ["pdf"]);
        let preferences = hkcu.open_subkey(format!("{settings_base}\\AssociationSelection")).unwrap();
        assert_eq!(preferences.get_value::<u32, _>(".pdf").unwrap(), 1);
        assert!(preferences.get_value::<u32, _>(".txt").is_err());
        drop(preferences);
        assert!(apply_at(&hkcu, &app_base, std::path::Path::new(r"C:\another\vw-app.exe"), &["txt".into()]).is_err());
        assert_eq!(read_at(&hkcu, &app_base, &exe).unwrap().selected, ["pdf"]);
        let capabilities = format!("{base}\\Capabilities");
        let registered = format!("{base}\\RegisteredApplications");
        let handler = format!("{base}\\ProgId");
        register_default_app_at(&hkcu, &exe, &["pdf".into()], &capabilities, &registered, &handler).unwrap();
        assert_eq!(hkcu.open_subkey(format!("{capabilities}\\FileAssociations")).unwrap().get_value::<String, _>(".pdf").unwrap(), "vw.file");
        assert_eq!(hkcu.open_subkey(&registered).unwrap().get_value::<String, _>("vw").unwrap(), capabilities);
        assert_eq!(hkcu.open_subkey(format!("{handler}\\shell\\open\\command")).unwrap().get_value::<String, _>("").unwrap(), command_for(&exe));
        register_default_app_at(&hkcu, &exe, &[], &capabilities, &registered, &handler).unwrap();
        assert!(hkcu.open_subkey(&capabilities).is_err());
        apply_selection_at(&hkcu, &app_base, &settings_base, &exe, &[]).unwrap();
        assert_eq!(read_at(&hkcu, &app_base, &exe).unwrap().registration, "none");
        assert_eq!(hkcu.open_subkey(&settings_base).unwrap().get_value::<u32, _>("FileAssociationsConfigured").unwrap(), 1);
        let _ = hkcu.delete_subkey_all(&base);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "opens the Windows Settings application"]
    fn default_apps_entry_opens_from_the_application_process() {
        vw_association_defaults().unwrap();
    }
}
