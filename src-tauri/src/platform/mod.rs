//! Platform integration: browser discovery, launching and default-browser handling.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

/// Minimal fallback so the crate builds on other systems during development.
#[cfg(not(any(target_os = "macos", windows)))]
mod other {
    use crate::browsers::Browser;
    use tauri::WebviewWindow;

    pub fn discover() -> Vec<Browser> {
        Vec::new()
    }
    pub fn launch(browser: &Browser, url: &str) -> std::io::Result<()> {
        std::process::Command::new(&browser.launch.path).arg(url).spawn().map(|_| ())
    }
    pub fn is_default() -> bool {
        false
    }
    pub fn make_default() {}
    pub fn register() {}
    pub fn style_picker(_window: &WebviewWindow) {}
}
#[cfg(not(any(target_os = "macos", windows)))]
pub use other::*;
