//! Open a user-selected source in another app, or a shell at its directory.
//! File paths never become PowerShell code or command-line fragments.
use std::path::{Path, PathBuf};
use vw_protocol::PluginError;

#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAction { OtherApp, Terminal, Reveal, App }

fn terminal_command(system_directory: &Path, directory: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(system_directory.join("WindowsPowerShell/v1.0/powershell.exe"));
    command.args(["-NoLogo", "-NoProfile", "-NoExit"]).current_dir(directory);
    command
}

/// An installed application that can open documents, detected from the Windows registry.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct ExternalApp { pub id: String, pub name: String, pub exe: String }

#[cfg(windows)]
fn shell_path(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let normalized = if wide.starts_with(&unc) { [vec![92, 92], wide[unc.len()..].to_vec()].concat() }
        else if wide.starts_with(&prefix) { wide[prefix.len()..].to_vec() } else { wide };
    PathBuf::from(std::ffi::OsString::from_wide(&normalized))
}

#[cfg(windows)]
fn system_directory() -> Result<PathBuf, PluginError> {
    let mut directory = vec![0u16; 32768];
    let length = unsafe { windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(directory.as_mut_ptr(), directory.len() as u32) } as usize;
    if length == 0 || length >= directory.len() { return Err(PluginError::internal("无法定位 Windows 系统目录")); }
    use std::os::windows::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&directory[..length])))
}

/// "/select,<path>" for explorer.exe: the whole path is one quoted argument;
/// quotes cannot appear in Windows file names, so the argument stays unambiguous.
#[cfg(windows)]
fn explorer_select_argument(path: &Path) -> String {
    format!("/select,\"{}\"", path.display())
}

#[cfg(windows)]
pub fn launch(action: ExternalAction, path: PathBuf, owner: usize) -> Result<bool, PluginError> {
    match action {
        ExternalAction::OtherApp => {
            // A dedicated STA avoids reusing a blocking worker with another COM apartment.
            std::thread::spawn(move || {
                use std::os::windows::ffi::OsStrExt;
                use windows_sys::Win32::{System::Com::*, UI::Shell::*};
                let initialized = unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
                if initialized < 0 { return Err(PluginError::internal(format!("无法初始化应用选择器（0x{initialized:08X}）"))); }
                struct Apartment;
                impl Drop for Apartment { fn drop(&mut self) { unsafe { CoUninitialize() }; } }
                let _apartment = Apartment;
                let filename: Vec<u16> = shell_path(&path).as_os_str().encode_wide().chain(Some(0)).collect();
                let info = OPENASINFO { pcszFile: filename.as_ptr(), pcszClass: std::ptr::null(), oaifInFlags: OAIF_EXEC | OAIF_HIDE_REGISTRATION };
                let result = unsafe { SHOpenWithDialog(owner as _, &info) };
                if result == 0x800704C7u32 as i32 { return Ok(false); } // ERROR_CANCELLED
                if result < 0 { return Err(PluginError::internal(format!("无法用其他软件打开（0x{result:08X}）"))); }
                Ok(true)
            }).join().map_err(|_| PluginError::internal("应用选择器线程异常"))?
        }
        ExternalAction::Terminal => {
            use std::os::windows::process::CommandExt;
            let system = system_directory()?;
            let mut command = terminal_command(&system, &shell_path(&path));
            command.creation_flags(0x00000010); // CREATE_NEW_CONSOLE: interactive terminal requested by the user.
            command.spawn().map_err(|e| PluginError::internal(format!("无法启动系统 PowerShell：{e}")))?;
            Ok(true)
        }
        ExternalAction::Reveal => {
            use std::os::windows::process::CommandExt;
            let system = system_directory()?;
            let mut command = std::process::Command::new(system.join("explorer.exe"));
            if path.is_dir() { command.arg(shell_path(&path)); }
            else { command.raw_arg(explorer_select_argument(&shell_path(&path))); }
            command.spawn().map_err(|e| PluginError::internal(format!("无法打开文件资源管理器：{e}")))?;
            Ok(true)
        }
        // Applications are started through launch_app with a registry-resolved executable.
        ExternalAction::App => Err(PluginError::unsupported("请选择要使用的软件")),
    }
}

/// Start a detected application with the document path as its only argument.
#[cfg(windows)]
pub fn launch_app(exe: &str, target: &Path) -> Result<(), PluginError> {
    std::process::Command::new(shell_path(Path::new(exe)))
        .arg(shell_path(target))
        .spawn()
        .map(|_| ())
        .map_err(|e| PluginError::internal(format!("无法启动所选软件：{e}")))
}

#[cfg(not(windows))]
pub fn launch_app(_: &str, _: &Path) -> Result<(), PluginError> {
    Err(PluginError::unsupported("外部软件与终端入口当前仅支持 Windows"))
}

/// Map an installed-product display name to its menu label and launcher executable.
fn product_launcher(display_name: &str) -> Option<(&'static str, &'static str)> {
    const PRODUCTS: &[(&str, &str, &str)] = &[
        ("intellij idea", "IntelliJ IDEA", "idea64.exe"),
        ("pycharm", "PyCharm", "pycharm64.exe"),
        ("android studio", "Android Studio", "studio64.exe"),
        ("visual studio code", "VS Code", "Code.exe"),
        ("webstorm", "WebStorm", "webstorm64.exe"),
        ("phpstorm", "PhpStorm", "phpstorm64.exe"),
        ("goland", "GoLand", "goland64.exe"),
        ("clion", "CLion", "clion64.exe"),
        ("rider", "Rider", "rider64.exe"),
        ("datagrip", "DataGrip", "datagrip64.exe"),
        ("rubymine", "RubyMine", "rubymine64.exe"),
    ];
    let name = display_name.to_lowercase();
    PRODUCTS.iter().find(|(keyword, _, _)| name.contains(keyword)).map(|(_, label, exe)| (*label, *exe))
}

/// Resolve the launcher executable from an uninstall entry. DisplayIcon is only a
/// directory (or direct exe) hint; a real file under the install location wins.
fn resolve_launcher(install_location: &str, display_icon: &str, exe_name: &str) -> Option<PathBuf> {
    let icon = display_icon.split(',').next().unwrap_or("").trim().trim_matches('"');
    for base in [install_location.trim(), icon] {
        if base.is_empty() { continue; }
        let root = PathBuf::from(base);
        if root.file_name().and_then(|name| name.to_str()) == Some(exe_name) && root.is_file() { return Some(root); }
        for candidate in [root.join(exe_name), root.join("bin").join(exe_name)] {
            if candidate.is_file() { return Some(candidate); }
        }
    }
    None
}

#[cfg(not(windows))]
pub fn detect_apps() -> Vec<ExternalApp> { Vec::new() }

/// Detect common editors and IDEs for the "open in" menu. Registry-only and read-only;
/// executables are re-resolved at launch time instead of trusting cached UI input.
#[cfg(windows)]
pub fn detect_apps() -> Vec<ExternalApp> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut apps: Vec<ExternalApp> = Vec::new();
    // App Paths covers machine-wide and user-setup VS Code installations.
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(key) = RegKey::predef(hive).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\App Paths\Code.exe") else { continue };
        let Ok(exe) = key.get_value::<String, _>("") else { continue };
        if exe.trim().is_empty() || apps.iter().any(|app| app.exe.eq_ignore_ascii_case(&exe)) { continue; }
        apps.push(ExternalApp { id: exe.to_lowercase(), name: "VS Code".into(), exe });
    }
    let uninstall = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let roots = [(HKEY_LOCAL_MACHINE, uninstall.to_string()), (HKEY_CURRENT_USER, uninstall.to_string()), (HKEY_LOCAL_MACHINE, format!("Software\\WOW6432Node\\{uninstall}"))];
    for (hive, path) in roots {
        let Ok(root) = RegKey::predef(hive).open_subkey(&path) else { continue };
        for name in root.enum_keys().flatten() {
            let Ok(entry) = root.open_subkey(&name) else { continue };
            let Ok(display_name) = entry.get_value::<String, _>("DisplayName") else { continue };
            let Some((label, exe_name)) = product_launcher(&display_name) else { continue };
            let install = entry.get_value::<String, _>("InstallLocation").unwrap_or_default();
            let icon = entry.get_value::<String, _>("DisplayIcon").unwrap_or_default();
            let Some(exe) = resolve_launcher(&install, &icon, exe_name) else { continue };
            let Some(exe) = exe.to_str() else { continue };
            if apps.iter().any(|app| app.exe.eq_ignore_ascii_case(exe)) { continue; }
            apps.push(ExternalApp { id: exe.to_lowercase(), name: label.into(), exe: exe.to_string() });
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.exe.cmp(&b.exe)));
    apps
}

#[cfg(not(windows))]
pub fn launch(_: ExternalAction, _: PathBuf, _: usize) -> Result<bool, PluginError> {
    Err(PluginError::unsupported("外部软件与终端入口当前仅支持 Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_treats_special_paths_only_as_working_directories() {
        let directory = Path::new(r"C:\notes & $x; 中文\-script.ps1");
        let command = terminal_command(Path::new(r"C:\Windows\System32"), directory);
        assert_eq!(command.get_current_dir(), Some(directory));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["-NoLogo", "-NoProfile", "-NoExit"]);
        assert!(command.get_program().to_string_lossy().ends_with("powershell.exe"));
        assert!(!command.get_args().any(|arg| arg == directory.as_os_str()));
    }
    #[cfg(windows)]
    #[test]
    fn shell_paths_preserve_disk_unc_and_unicode_names() {
        assert_eq!(shell_path(Path::new(r"\\?\C:\文档\a & b.txt")), PathBuf::from(r"C:\文档\a & b.txt"));
        assert_eq!(shell_path(Path::new(r"\\?\UNC\server\share\a.txt")), PathBuf::from(r"\\server\share\a.txt"));
    }
    #[test]
    fn product_names_map_to_launchers_and_reject_unknowns() {
        assert_eq!(product_launcher("IntelliJ IDEA 2024.2 (Ultimate Edition)"), Some(("IntelliJ IDEA", "idea64.exe")));
        assert_eq!(product_launcher("Microsoft Visual Studio Code (User)"), Some(("VS Code", "Code.exe")));
        assert_eq!(product_launcher("Android Studio"), Some(("Android Studio", "studio64.exe")));
        assert_eq!(product_launcher("PyCharm Community Edition"), Some(("PyCharm", "pycharm64.exe")));
        assert_eq!(product_launcher("7-Zip 24.08"), None);
        assert_eq!(product_launcher(""), None);
    }
    #[test]
    fn launcher_resolution_needs_a_real_file_and_accepts_display_icon_hints() {
        assert_eq!(resolve_launcher(r"C:\no-such-dir", "", "idea64.exe"), None);
        assert_eq!(resolve_launcher("", format!("{},0", r"C:\no-such-dir\bin\idea64.exe").as_str(), "idea64.exe"), None);
        #[cfg(windows)]
        {
            let temp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/vw-launcher-test");
            let bin = temp.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("idea64.exe"), b"mz").unwrap();
            assert_eq!(resolve_launcher(temp.to_str().unwrap(), "", "idea64.exe"), Some(bin.join("idea64.exe")));
            assert_eq!(resolve_launcher("", &format!("{},0", bin.join("idea64.exe").display()), "idea64.exe"), Some(bin.join("idea64.exe")));
            assert_eq!(resolve_launcher(temp.to_str().unwrap(), "", "missing.exe"), None);
            std::fs::remove_dir_all(&temp).unwrap();
        }
    }
    #[cfg(windows)]
    #[test]
    fn explorer_select_argument_quotes_the_whole_path_only() {
        assert_eq!(explorer_select_argument(Path::new(r"C:\文档 a&b\文 本.txt")), r#"/select,"C:\文档 a&b\文 本.txt""#);
    }
}
