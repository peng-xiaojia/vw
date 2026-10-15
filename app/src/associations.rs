//! Registered "open with" targets for a document's file type, with native icons.
//! Everything comes from the registry the way Explorer builds its Open With menu:
//! no hardcoded product lists, and executables are re-resolved before each launch.
use std::path::Path;

/// One launchable entry of the Open With menu for the current file type.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct AssociatedApp {
    pub id: String,
    pub name: String,
    pub exe: String,
    /// blake3 key into the icon cache; absent when no icon could be extracted.
    pub icon_key: Option<String>,
    pub is_default: bool,
}

/// A terminal emulator the user can open the document's directory in.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct TerminalChoice {
    pub id: String,
    pub name: String,
}

/// Pull the executable out of a registry shell-open command such as
/// `"C:\Program Files\x\app.exe" "%1"` or `C:\Tools\app.exe /flag`.
fn command_executable(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        return rest.split('"').next().filter(|part| !part.is_empty()).map(str::to_string);
    }
    let lower = trimmed.to_lowercase();
    let end = lower.find(".exe")? + 4;
    Some(trimmed[..end].to_string())
}

/// Parse a DefaultIcon value such as `"C:\x\app.exe,-5"`, `C:\x\app.exe,3` or a bare path.
fn icon_spec(value: &str) -> Option<(String, i32)> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rsplit_once(',') {
        // A negative number is a resource id; anything non-numeric after the comma
        // (a path containing a comma) stays part of the file name.
        Some((path, index)) => match index.trim().parse::<i32>() {
            Ok(parsed) => Some((path.trim_matches('"').to_string(), parsed)),
            Err(_) => Some((trimmed.to_string(), 0)),
        },
        None => Some((trimmed.to_string(), 0)),
    }
}

#[cfg(windows)]
mod windows {
    use super::{icon_spec, AssociatedApp, TerminalChoice};
    use std::collections::BTreeSet;
    use std::path::Path;
    use winreg::enums::*;
    use winreg::RegKey;

    const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

    fn read_string(key: &RegKey, name: &str) -> Option<String> {
        key.get_value::<String, _>(name).ok().filter(|value| !value.trim().is_empty())
    }

    fn open_subkey<'a>(key: &'a RegKey, path: &str) -> Option<RegKey> {
        key.open_subkey(path).ok()
    }

    fn value_names(key: &RegKey) -> Vec<String> {
        key.enum_values().flatten().map(|(name, _)| name).collect()
    }

    fn user_choice_progid(extension: &str) -> Option<String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = open_subkey(&hkcu, &format!("{FILE_EXTS}\\{extension}\\UserChoice"))?;
        read_string(&key, "ProgId")
    }

    /// ProgIds suggested for this extension, default first, without duplicates.
    fn progid_candidates(extension: &str) -> Vec<(String, bool)> {
        let mut ordered: Vec<(String, bool)> = Vec::new();
        let mut push = |progid: Option<String>, is_default: bool| {
            if let Some(progid) = progid {
                if !progid.is_empty() && !ordered.iter().any(|(existing, _)| existing == &progid) {
                    ordered.push((progid, is_default));
                }
            }
        };
        push(user_choice_progid(extension), true);
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Some(key) = open_subkey(&hkcu, &format!("{FILE_EXTS}\\{extension}\\OpenWithProgids")) {
            for name in value_names(&key) {
                if name != "MRUList" {
                    push(Some(name), false);
                }
            }
        }
        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        if let Some(key) = open_subkey(&hkcr, &format!("{extension}\\OpenWithProgids")) {
            for name in value_names(&key) {
                if name != "MRUList" {
                    push(Some(name), false);
                }
            }
        }
        ordered
    }

    /// Executable file names registered under OpenWithList, MRU order first.
    fn open_with_list_exes(extension: &str) -> Vec<String> {
        let mut exes: Vec<String> = Vec::new();
        let mut collect = |root: &RegKey, path: String| {
            if let Some(key) = open_subkey(root, &path) {
                let mru = read_string(&key, "MRUList").unwrap_or_default();
                for letter in mru.chars() {
                    if let Some(exe) = read_string(&key, &letter.to_string()) {
                        if !exes.iter().any(|existing| existing.eq_ignore_ascii_case(&exe)) {
                            exes.push(exe);
                        }
                    }
                }
                for name in value_names(&key) {
                    if name.len() == 1 {
                        if let Some(exe) = read_string(&key, &name) {
                            if !exes.iter().any(|existing| existing.eq_ignore_ascii_case(&exe)) {
                                exes.push(exe);
                            }
                        }
                    }
                }
            }
        };
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        collect(&hkcu, format!("{FILE_EXTS}\\{extension}\\OpenWithList"));
        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        collect(&hkcr, format!("{extension}\\OpenWithList"));
        exes
    }

    /// Resolve an OpenWithList exe name to a real executable path.
    fn exe_path_for(exe: &str) -> Option<String> {
        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        if let Some(key) = open_subkey(&hkcr, &format!("Applications\\{exe}\\shell\\open\\command")) {
            if let Some(command) = read_string(&key, "") {
                if let Some(path) = super::command_executable(&command) {
                    return Some(path);
                }
            }
        }
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for hive in [&hklm, &hkcu] {
            if let Some(key) = open_subkey(hive, &format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}")) {
                if let Some(path) = read_string(&key, "") {
                    return Some(path);
                }
            }
        }
        None
    }

    /// Display name for a ProgId: FriendlyTypeName (indirect strings resolved),
    /// then the ProgId description, then the executable's file description.
    fn progid_display_name(progid: &str, exe: &str) -> Option<String> {
        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        let key = open_subkey(&hkcr, progid)?;
        if let Some(friendly) = read_string(&key, "FriendlyTypeName") {
            if let Some(resolved) = super::resolve_indirect_string(&friendly) {
                return Some(resolved);
            }
        }
        if let Some(description) = read_string(&key, "") {
            return Some(description);
        }
        super::file_description(exe)
    }

    /// The DefaultIcon registry value for a ProgId, falling back to its executable.
    fn icon_source_for_progid(hkcr: &RegKey, progid: &str, exe: &str) -> Option<(String, i32)> {
        if let Some(key) = open_subkey(hkcr, &format!("{progid}\\DefaultIcon")) {
            if let Some(value) = read_string(&key, "") {
                if let Some(spec) = icon_spec(&value) {
                    return Some(spec);
                }
            }
        }
        Some((exe.to_string(), 0))
    }

    pub fn associated_apps(source: &Path, cache: &std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>) -> Vec<AssociatedApp> {
        let Some(extension) = source.extension().and_then(|name| name.to_str()).map(|name| format!(".{}", name.to_lowercase())) else {
            return Vec::new();
        };
        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        let mut apps: Vec<AssociatedApp> = Vec::new();
        let mut seen_exes: BTreeSet<String> = BTreeSet::new();
        for (progid, is_default) in progid_candidates(&extension) {
            let command = open_subkey(&hkcr, &format!("{progid}\\shell\\open\\command"))
                .and_then(|key| read_string(&key, ""));
            let exe = command.as_deref().and_then(super::command_executable);
            let Some(exe) = exe else { continue };
            if !seen_exes.insert(exe.to_lowercase()) {
                continue;
            }
            let name = progid_display_name(&progid, &exe).unwrap_or_else(|| super::file_stem_label(&exe));
            let icon = icon_source_for_progid(&hkcr, &progid, &exe);
            apps.push(super::build_app(cache, exe, name, is_default, icon));
            if apps.len() >= 8 {
                return apps;
            }
        }
        for exe in open_with_list_exes(&extension) {
            let Some(path) = exe_path_for(&exe) else { continue };
            if !seen_exes.insert(path.to_lowercase()) {
                continue;
            }
            let name = super::native::file_description(&path).unwrap_or_else(|| super::file_stem_label(&path));
            let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
            let icon = open_subkey(&hkcr, &format!("Applications\\{exe}\\DefaultIcon"))
                .and_then(|key| read_string(&key, ""))
                .and_then(|value| icon_spec(&value))
                .unwrap_or_else(|| (path.clone(), 0));
            apps.push(super::build_app(cache, path, name, false, Some(icon)));
            if apps.len() >= 8 {
                break;
            }
        }
        apps
    }

    pub fn terminal_choices() -> Vec<TerminalChoice> {
        let mut choices = vec![
            TerminalChoice { id: "windows-terminal".into(), name: "Windows Terminal".into() },
            TerminalChoice { id: "powershell".into(), name: "PowerShell".into() },
            TerminalChoice { id: "cmd".into(), name: "CMD".into() },
        ];
        choices.retain(|choice| choice.id != "windows-terminal" || super::windows_terminal_path().is_some());
        choices
    }
}

#[cfg(not(windows))]
mod windows {
    use super::{AssociatedApp, TerminalChoice};
    use std::path::Path;
    pub fn associated_apps(_: &Path, _: &std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>) -> Vec<AssociatedApp> {
        Vec::new()
    }
    pub fn terminal_choices() -> Vec<TerminalChoice> {
        Vec::new()
    }
}

pub fn associated_apps(source: &Path, icons: &std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>) -> Vec<AssociatedApp> {
    windows::associated_apps(source, icons)
}

#[cfg(windows)]
pub(crate) use native::{file_description, resolve_indirect_string, windows_terminal_path};

pub fn terminal_choices() -> Vec<TerminalChoice> {
    windows::terminal_choices()
}

/// Launch the terminal matching the dropdown choice in the document directory.
#[cfg(windows)]
pub fn launch_terminal(terminal: &str, directory: &Path) -> Result<bool, crate::PluginError> {
    use std::os::windows::process::CommandExt;
    let system = crate::external_open::system_directory()?;
    let shell = crate::external_open::shell_path(directory);
    let mut command = match terminal {
        "cmd" => {
            let mut command = std::process::Command::new(system.join("cmd.exe"));
            command.arg("/K").current_dir(&shell);
            command
        }
        "windows-terminal" => {
            let Some(wt) = native::windows_terminal_path() else {
                return Err(crate::PluginError::unsupported("未找到 Windows Terminal"));
            };
            let mut command = std::process::Command::new(wt);
            command.arg("-d").arg(&shell);
            command.spawn().map_err(|e| crate::PluginError::internal(format!("无法启动 Windows Terminal：{e}")))?;
            return Ok(true);
        }
        _ => crate::external_open::terminal_command(&system, &shell),
    };
    command.creation_flags(0x00000010); // CREATE_NEW_CONSOLE: the user asked for a visible terminal.
    command.spawn().map_err(|e| crate::PluginError::internal(format!("无法启动终端：{e}")))?;
    Ok(true)
}

#[cfg(not(windows))]
pub fn launch_terminal(_: &str, _: &Path) -> Result<bool, crate::PluginError> {
    Err(crate::PluginError::unsupported("外部软件与终端入口当前仅支持 Windows"))
}

fn build_app(
    cache: &std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    exe: String,
    name: String,
    is_default: bool,
    icon: Option<(String, i32)>,
) -> AssociatedApp {
    let icon_key = icon.and_then(|(path, index)| cache_icon(cache, &path, index));
    AssociatedApp { id: exe.to_lowercase(), name, exe, icon_key, is_default }
}

fn file_stem_label(exe: &str) -> String {
    Path::new(exe)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(exe)
        .to_string()
}

/// Extract, encode and cache a 32×32 native icon; the key feeds the vw-icon scheme.
#[cfg(windows)]
fn cache_icon(
    cache: &std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    path: &str,
    index: i32,
) -> Option<String> {
    let key = blake3::hash(format!("{path}|{index}").as_bytes()).to_hex()[..32].to_string();
    let mut guard = cache.lock().ok()?;
    if !guard.contains_key(&key) {
        let png = native::extract_icon_png(path, index)?;
        guard.insert(key.clone(), png);
    }
    Some(key)
}

#[cfg(not(windows))]
fn cache_icon(
    _: &std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    _: &str,
    _: i32,
) -> Option<String> {
    None
}

#[cfg(windows)]
mod native {
    //! Win32 helpers: indirect strings, version resources and icon extraction.
    use std::ffi::c_void;
    use std::path::PathBuf;
    use windows_sys::Win32::Graphics::Gdi as gdi;
    use windows_sys::Win32::Storage::FileSystem as version;
    use windows_sys::Win32::UI::Shell::SHLoadIndirectString;
    use windows_sys::Win32::UI::WindowsAndMessaging as user;

    pub fn windows_terminal_path() -> Option<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA")?;
        let path = PathBuf::from(local).join(r"Microsoft\WindowsApps\wt.exe");
        path.is_file().then_some(path)
    }

    /// Resolve "@dll,-resource" indirect strings; plain strings pass through.
    pub fn resolve_indirect_string(value: &str) -> Option<String> {
        if !value.starts_with('@') {
            return Some(value.to_string());
        }
        let source: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let mut out = [0u16; 1024];
        let result = unsafe { SHLoadIndirectString(source.as_ptr(), out.as_mut_ptr(), out.len() as u32, std::ptr::null()) };
        if result != 0 {
            return None;
        }
        let end = out.iter().position(|char| *char == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&out[..end]))
    }

    /// The FileDescription version resource of an executable, used as the app name.
    pub fn file_description(exe: &str) -> Option<String> {
        let wide: Vec<u16> = exe.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let size = version::GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            if version::GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
                return None;
            }
            let mut buffer: *mut c_void = std::ptr::null_mut();
            let mut length = 0u32;
            let translations: Vec<u16> = utf16("\\VarFileInfo\\Translation");
            if version::VerQueryValueW(data.as_ptr() as *const c_void, translations.as_ptr(), &mut buffer, &mut length) == 0
                || (length as usize) < 4
            {
                return None;
            }
            let pairs: &[u16] = std::slice::from_raw_parts(buffer as *const u16, (length as usize) / 2);
            let query = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", pairs[0], pairs[1]);
            let query: Vec<u16> = query.encode_utf16().chain(Some(0)).collect();
            if version::VerQueryValueW(data.as_ptr() as *const c_void, query.as_ptr(), &mut buffer, &mut length) == 0
                || length == 0
            {
                return None;
            }
            let text: &[u16] = std::slice::from_raw_parts(buffer as *const u16, length as usize);
            let end = text.iter().position(|char| *char == 0).unwrap_or(text.len());
            let description = String::from_utf16_lossy(&text[..end]);
            let trimmed = description.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
    }

    fn utf16(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    /// Extract a 32×32 icon as PNG bytes; legacy no-alpha icons fall back to the AND mask.
    pub fn extract_icon_png(path: &str, index: i32) -> Option<Vec<u8>> {
        if path.trim().is_empty() {
            return None;
        }
        const SIDE: usize = 32;
        const PIXELS: usize = SIDE * SIDE;
        let wide = utf16(path);
        unsafe {
            let mut icon = std::ptr::null_mut();
            let extracted = user::PrivateExtractIconsW(wide.as_ptr(), index, SIDE as i32, SIDE as i32, &mut icon, std::ptr::null_mut(), 1, 0);
            if extracted != 1 || icon.is_null() {
                return None;
            }
            struct IconGuard(user::HICON);
            impl Drop for IconGuard {
                fn drop(&mut self) {
                    unsafe { user::DestroyIcon(self.0) };
                }
            }
            let _icon_guard = IconGuard(icon);
            let mut info: user::ICONINFO = std::mem::zeroed();
            if user::GetIconInfo(icon, &mut info) == 0 {
                return None;
            }
            struct BitmapGuard(gdi::HBITMAP);
            impl Drop for BitmapGuard {
                fn drop(&mut self) {
                    unsafe { gdi::DeleteObject(self.0 as *mut c_void) };
                }
            }
            let _color_guard = BitmapGuard(info.hbmColor);
            let _mask_guard = BitmapGuard(info.hbmMask);
            let screen = gdi::GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return None;
            }
            let memory = gdi::CreateCompatibleDC(screen);
            if memory.is_null() {
                gdi::ReleaseDC(std::ptr::null_mut(), screen);
                return None;
            }
            struct DcGuard(gdi::HDC, gdi::HDC);
            impl Drop for DcGuard {
                fn drop(&mut self) {
                    unsafe {
                        gdi::DeleteDC(self.1);
                        gdi::ReleaseDC(std::ptr::null_mut(), self.0);
                    };
                }
            }
            let _dc_guard = DcGuard(screen, memory);
            let header = |bit_count: u16| gdi::BITMAPINFO {
                bmiHeader: gdi::BITMAPINFOHEADER {
                    biSize: size_of::<gdi::BITMAPINFOHEADER>() as u32,
                    biWidth: SIDE as i32,
                    biHeight: -(SIDE as i32),
                    biPlanes: 1,
                    biBitCount: bit_count,
                    biCompression: gdi::BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [gdi::RGBQUAD::default(); 1],
            };
            let mut pixels = vec![0u8; PIXELS * 4];
            let color_info = &mut header(32);
            let lines = gdi::GetDIBits(memory, info.hbmColor, 0, SIDE as u32, pixels.as_mut_ptr() as *mut c_void, color_info, gdi::DIB_RGB_COLORS);
            if lines != SIDE as i32 {
                return None;
            }
            let mut mask = vec![0u8; SIDE * (SIDE / 8)];
            let mask_info = &mut header(1);
            gdi::GetDIBits(memory, info.hbmMask, 0, SIDE as u32, mask.as_mut_ptr() as *mut c_void, mask_info, gdi::DIB_RGB_COLORS);
            let mut rgba = vec![0u8; PIXELS * 4];
            for pixel in 0..PIXELS {
                rgba[pixel * 4] = pixels[pixel * 4 + 2];
                rgba[pixel * 4 + 1] = pixels[pixel * 4 + 1];
                rgba[pixel * 4 + 2] = pixels[pixel * 4];
                rgba[pixel * 4 + 3] = pixels[pixel * 4 + 3];
            }
            if rgba.chunks_exact(4).all(|channel| channel[3] == 0) {
                for pixel in 0..PIXELS {
                    let byte = mask[pixel / SIDE * (SIDE / 8) + (pixel % SIDE) / 8];
                    let transparent = (byte >> (7 - pixel % SIDE % 8)) & 1;
                    rgba[pixel * 4 + 3] = if transparent == 1 { 0 } else { 255 };
                }
            }
            let image = image::RgbaImage::from_raw(SIDE as u32, SIDE as u32, rgba)?;
            let mut png = Vec::new();
            image::DynamicImage::ImageRgba8(image)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .ok()?;
            Some(png)
        }
    }
}

#[cfg(not(windows))]
mod native {
    pub fn resolve_indirect_string(_: &str) -> Option<String> {
        None
    }
    pub fn file_description(_: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_commands_reduce_to_their_executable() {
        assert_eq!(command_executable("\"C:\\Program Files\\App\\tool.exe\" \"%1\""), Some("C:\\Program Files\\App\\tool.exe".into()));
        assert_eq!(command_executable("C:\\Tools\\app.exe /flag \"%1\""), Some("C:\\Tools\\app.exe".into()));
        assert_eq!(command_executable("C:\\Tools\\app.EXE /flag"), Some("C:\\Tools\\app.EXE".into()));
        assert_eq!(command_executable("\"\""), None);
        assert_eq!(command_executable("rundll32 params"), None);
    }
    #[test]
    fn default_icon_values_split_into_path_and_index() {
        assert_eq!(icon_spec("\"C:\\x\\a.exe,-5\""), Some(("C:\\x\\a.exe".into(), -5)));
        assert_eq!(icon_spec("C:\\x\\a.exe,3"), Some(("C:\\x\\a.exe".into(), 3)));
        assert_eq!(icon_spec("C:\\x\\a.exe"), Some(("C:\\x\\a.exe".into(), 0)));
        assert_eq!(icon_spec("C:\\dir with,comma\\a.exe"), Some(("C:\\dir with,comma\\a.exe".into(), 0)));
        assert_eq!(icon_spec(""), None);
    }
    #[test]
    fn extension_labels_come_from_the_document() {
        assert_eq!(file_stem_label("C:\\Tools\\Code.exe"), "Code");
    }
}
