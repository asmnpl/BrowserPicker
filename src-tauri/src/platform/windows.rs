use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::{null, null_mut};

use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, S_OK};
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::UI::Shell::{
    SHChangeNotify, SHDefExtractIconW, SHLoadIndirectString, ShellExecuteW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DestroyIcon, GetIconInfo, ASFW_ANY, HICON, ICONINFO, SW_SHOWNORMAL,
};

use crate::browsers::{png_data_url, Browser, Launch};

const APP_KEY: &str = "BrowserPicker";
const URL_PROG_ID: &str = "BrowserPickerURL";
const HTML_PROG_ID: &str = "BrowserPickerHTML";
const CLIENTS: &str = r"Software\Clients\StartMenuInternet";
const ICON_PX: i32 = 64;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Key {
    fn open(root: HKEY, path: &str, flags: u32) -> Option<Key> {
        let mut key: HKEY = null_mut();
        let path = wide(path);
        let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ | flags, &mut key) };
        (status == ERROR_SUCCESS).then_some(Key(key))
    }

    fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut buf = [0u16; 256];
        for index in 0.. {
            let mut len = buf.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    buf.as_mut_ptr(),
                    &mut len,
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        names
    }

    /// Reads a string value; `name` of `None` reads the default value.
    fn string(&self, subkey: &str, name: Option<&str>) -> Option<String> {
        read_string(self.0, subkey, name)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn read_string(root: HKEY, subkey: &str, name: Option<&str>) -> Option<String> {
    let subkey = wide(subkey);
    let name = name.map(wide);
    let name_ptr = name.as_ref().map_or(null(), |n| n.as_ptr());
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(root, subkey.as_ptr(), name_ptr, RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size)
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u16; size as usize / 2 + 1];
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            name_ptr,
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then(|| from_wide(&buf)).filter(|s| !s.is_empty())
}

fn write_string(subkey: &str, name: Option<&str>, value: &str) {
    let subkey = wide(subkey);
    let name = name.map(wide);
    let value = wide(value);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ref().map_or(null(), |n| n.as_ptr()),
            REG_SZ,
            value.as_ptr() as *const c_void,
            (value.len() * 2) as u32,
        );
    }
}

/// Resolves `@file.dll,-123` style resource strings.
fn indirect(s: String) -> String {
    if !s.starts_with('@') {
        return s;
    }
    let src = wide(&s);
    let mut buf = [0u16; 512];
    let hr = unsafe { SHLoadIndirectString(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, null_mut()) };
    if hr == S_OK {
        from_wide(&buf)
    } else {
        s
    }
}

/// Splits a registered command line into the executable and the remaining arguments.
fn split_command(command: &str) -> (String, String) {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
    }
    let lower = command.to_ascii_lowercase();
    match lower.find(".exe") {
        Some(end) => (command[..end + 4].to_string(), command[end + 4..].trim().to_string()),
        None => (command.to_string(), String::new()),
    }
}

/// Splits an icon location such as `C:\app.exe,-101` into path and index.
fn split_icon(location: &str) -> (String, i32) {
    let location = location.trim().trim_matches('"');
    if let Some((path, index)) = location.rsplit_once(',') {
        if let Ok(index) = index.trim().parse() {
            return (path.trim().trim_matches('"').to_string(), index);
        }
    }
    (location.to_string(), 0)
}

pub fn discover() -> Vec<Browser> {
    let sources = [
        (HKEY_CURRENT_USER, 0),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ];
    let mut browsers: Vec<Browser> = Vec::new();
    for (root, flags) in sources {
        let Some(clients) = Key::open(root, CLIENTS, flags) else { continue };
        for id in clients.subkeys() {
            if id.eq_ignore_ascii_case(APP_KEY) || browsers.iter().any(|b| b.id.eq_ignore_ascii_case(&id)) {
                continue;
            }
            let Some(command) = clients.string(&format!(r"{id}\shell\open\command"), None) else { continue };
            let (path, args) = split_command(&command);
            if !std::path::Path::new(&path).exists() {
                continue;
            }
            let name = clients
                .string(&id, None)
                .or_else(|| clients.string(&format!(r"{id}\Capabilities"), Some("ApplicationName")))
                .map(indirect)
                .unwrap_or_else(|| id.clone());
            let (icon_path, icon_index) = clients
                .string(&format!(r"{id}\DefaultIcon"), None)
                .map(|loc| split_icon(&loc))
                .unwrap_or_else(|| (path.clone(), 0));
            browsers.push(Browser {
                icon: icon(&icon_path, icon_index).map(|png| png_data_url(&png)),
                id,
                name,
                launch: Launch { path, args },
            });
        }
    }
    browsers
}

fn icon(path: &str, index: i32) -> Option<Vec<u8>> {
    let path = wide(path);
    let mut hicon: HICON = null_mut();
    let hr = unsafe { SHDefExtractIconW(path.as_ptr(), index, 0, &mut hicon, null_mut(), ICON_PX as u32) };
    if hr != S_OK || hicon.is_null() {
        return None;
    }
    let png = unsafe { icon_to_png(hicon) };
    unsafe { DestroyIcon(hicon) };
    png
}

unsafe fn bitmap_pixels(bitmap: *mut c_void, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..std::mem::zeroed()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let dc = GetDC(null_mut());
    let lines = GetDIBits(
        dc,
        bitmap,
        0,
        height as u32,
        pixels.as_mut_ptr() as *mut c_void,
        &mut info,
        DIB_RGB_COLORS,
    );
    ReleaseDC(null_mut(), dc);
    (lines == height).then_some(pixels)
}

unsafe fn icon_to_png(hicon: HICON) -> Option<Vec<u8>> {
    let mut info: ICONINFO = std::mem::zeroed();
    if GetIconInfo(hicon, &mut info) == 0 {
        return None;
    }
    let result = (|| {
        if info.hbmColor.is_null() {
            return None;
        }
        let mut bm: BITMAP = std::mem::zeroed();
        if GetObjectW(info.hbmColor, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut c_void)
            == 0
        {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut pixels = bitmap_pixels(info.hbmColor, w, h)?;
        // Legacy icons carry no alpha channel: derive it from the AND mask.
        if pixels.chunks(4).all(|p| p[3] == 0) {
            let mask = bitmap_pixels(info.hbmMask, w, h)?;
            for (p, m) in pixels.chunks_mut(4).zip(mask.chunks(4)) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        for p in pixels.chunks_mut(4) {
            p.swap(0, 2); // BGRA -> RGBA
        }
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().ok()?.write_image_data(&pixels).ok()?;
        Some(png)
    })();
    DeleteObject(info.hbmColor);
    DeleteObject(info.hbmMask);
    result
}

pub fn launch(browser: &Browser, url: &str) -> std::io::Result<()> {
    let url = url.replace('"', "%22");
    let mut command = Command::new(&browser.launch.path);
    let args = &browser.launch.args;
    if args.contains("%1") {
        command.raw_arg(args.replace("%1", &url));
    } else {
        if !args.is_empty() {
            command.raw_arg(args);
        }
        command.raw_arg(format!("\"{url}\""));
    }
    command.creation_flags(CREATE_NO_WINDOW).spawn().map(|_| ())
}

pub fn is_default() -> bool {
    read_string(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        Some("ProgId"),
    )
    .is_some_and(|id| id == URL_PROG_ID)
}

/// Windows only lets the user pick the default browser, so open the right settings page.
pub fn make_default() {
    let operation = wide("open");
    let target = wide(&format!("ms-settings:defaultapps?registeredAppUser={APP_KEY}"));
    unsafe {
        ShellExecuteW(null_mut(), operation.as_ptr(), target.as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

/// Registers the app as a browser for the current user so it shows up in Default apps.
pub fn register() {
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.to_string_lossy().to_string();
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    let url_class = format!(r"Software\Classes\{URL_PROG_ID}");
    if read_string(HKEY_CURRENT_USER, &format!(r"{url_class}\shell\open\command"), None).as_deref()
        == Some(&open)
    {
        return;
    }

    let app = format!(r"{CLIENTS}\{APP_KEY}");
    let caps = format!(r"{app}\Capabilities");
    write_string(&app, None, APP_KEY);
    write_string(&format!(r"{app}\DefaultIcon"), None, &icon);
    write_string(&format!(r"{app}\shell\open\command"), None, &format!("\"{exe}\""));
    write_string(&caps, Some("ApplicationName"), APP_KEY);
    write_string(&caps, Some("ApplicationIcon"), &icon);
    write_string(&caps, Some("ApplicationDescription"), "Choose a browser for every link you open.");
    write_string(&format!(r"{caps}\StartMenu"), Some("StartMenuInternet"), APP_KEY);
    for scheme in ["http", "https"] {
        write_string(&format!(r"{caps}\URLAssociations"), Some(scheme), URL_PROG_ID);
    }
    for ext in [".htm", ".html", ".xhtml"] {
        write_string(&format!(r"{caps}\FileAssociations"), Some(ext), HTML_PROG_ID);
    }

    for (prog_id, label, is_url) in
        [(URL_PROG_ID, "BrowserPicker URL", true), (HTML_PROG_ID, "BrowserPicker HTML Document", false)]
    {
        let class = format!(r"Software\Classes\{prog_id}");
        write_string(&class, None, label);
        if is_url {
            write_string(&class, Some("URL Protocol"), "");
        }
        write_string(&format!(r"{class}\DefaultIcon"), None, &icon);
        write_string(&format!(r"{class}\shell\open\command"), None, &open);
    }

    write_string(r"Software\RegisteredApplications", Some(APP_KEY), &caps);
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, null(), null()) };
}

/// Lets the running instance take focus when a second instance hands over a link.
pub fn allow_foreground() {
    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
}

pub fn style_picker(_window: &WebviewWindow) {}

/// Mica backdrops need Windows 11 (build 22000+).
pub fn has_mica() -> bool {
    read_string(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        Some("CurrentBuildNumber"),
    )
    .and_then(|build| build.parse::<u32>().ok())
    .is_some_and(|build| build >= 22000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_commands() {
        assert_eq!(
            split_command(r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#),
            (r"C:\Program Files\Mozilla Firefox\firefox.exe".into(), r#"-osint -url "%1""#.into())
        );
        assert_eq!(split_command(r"C:\x\chrome.exe"), (r"C:\x\chrome.exe".into(), String::new()));
        assert_eq!(split_icon(r"C:\a\b.exe,-101"), (r"C:\a\b.exe".into(), -101));
        assert_eq!(split_icon(r#""C:\a\b.exe""#), (r"C:\a\b.exe".into(), 0));
    }
}
