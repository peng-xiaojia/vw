//! Open a user-selected source in another app, or a shell at its directory.
//! File paths never become PowerShell code or command-line fragments.
use std::path::{Path, PathBuf};
use vw_protocol::PluginError;

#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAction { OtherApp, Terminal, Reveal }

pub(crate) fn terminal_command(system_directory: &Path, directory: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(system_directory.join("WindowsPowerShell/v1.0/powershell.exe"));
    command.args(["-NoLogo", "-NoProfile", "-NoExit"]).current_dir(directory);
    command
}

#[cfg(windows)]
pub(crate) fn shell_path(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let normalized = if wide.starts_with(&unc) { [vec![92, 92], wide[unc.len()..].to_vec()].concat() }
        else if wide.starts_with(&prefix) { wide[prefix.len()..].to_vec() } else { wide };
    PathBuf::from(std::ffi::OsString::from_wide(&normalized))
}

#[cfg(windows)]
pub(crate) fn system_directory() -> Result<PathBuf, PluginError> {
    let mut directory = vec![0u16; 32768];
    let length = unsafe { windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(directory.as_mut_ptr(), directory.len() as u32) } as usize;
    if length == 0 || length >= directory.len() { return Err(PluginError::internal("无法定位 Windows 系统目录")); }
    use std::os::windows::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&directory[..length])))
}

/// explorer.exe lives in the Windows directory, not System32; launching from the
/// system directory was the "os error 2" reported for 资源管理器.
#[cfg(windows)]
fn windows_directory() -> Result<PathBuf, PluginError> {
    let mut directory = vec![0u16; 32768];
    let length = unsafe { windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW(directory.as_mut_ptr(), directory.len() as u32) } as usize;
    if length == 0 || length >= directory.len() { return Err(PluginError::internal("无法定位 Windows 目录")); }
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
            let windows = windows_directory()?;
            let mut command = std::process::Command::new(windows.join("explorer.exe"));
            if path.is_dir() { command.arg(shell_path(&path)); }
            else { command.raw_arg(explorer_select_argument(&shell_path(&path))); }
            command.spawn().map_err(|e| PluginError::internal(format!("无法打开文件资源管理器：{e}")))?;
            Ok(true)
        }
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
    #[cfg(windows)]
    #[test]
    fn explorer_select_argument_quotes_the_whole_path_only() {
        assert_eq!(explorer_select_argument(Path::new(r"C:\文档 a&b\文 本.txt")), r#"/select,"C:\文档 a&b\文 本.txt""#);
    }
}
