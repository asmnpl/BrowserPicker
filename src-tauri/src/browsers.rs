use serde::Serialize;

use crate::config::Config;

/// A browser installed on the system.
#[derive(Debug, Clone, Serialize)]
pub struct Browser {
    /// Stable identifier: bundle id on macOS, `StartMenuInternet` key on Windows.
    pub id: String,
    pub name: String,
    /// PNG icon as a `data:` URL.
    pub icon: Option<String>,
    #[serde(skip)]
    pub launch: Launch,
}

/// How to start a browser with a URL.
#[derive(Debug, Clone, Default)]
pub struct Launch {
    /// `.app` bundle path on macOS, executable path on Windows.
    pub path: String,
    /// Raw extra arguments from the registered command line (Windows).
    #[allow(dead_code)]
    pub args: String,
}

/// A browser as shown in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserView {
    #[serde(flatten)]
    pub browser: Browser,
    pub hidden: bool,
}

/// Applies the user's order and visibility to discovered browsers.
pub fn arrange(mut browsers: Vec<Browser>, config: &Config) -> Vec<BrowserView> {
    let rank = |id: &str| config.order.iter().position(|o| o == id).unwrap_or(usize::MAX);
    browsers.sort_by(|a, b| {
        rank(&a.id).cmp(&rank(&b.id)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    browsers
        .into_iter()
        .map(|browser| BrowserView { hidden: config.hidden.contains(&browser.id), browser })
        .collect()
}

#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64(png))
}

#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Turns a command-line argument into a URL the picker can handle.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn normalize_url(arg: &str) -> Option<String> {
    let arg = arg.trim().trim_matches('"');
    let lower = arg.to_ascii_lowercase();
    if ["http://", "https://", "file://"].iter().any(|p| lower.starts_with(p)) {
        return Some(arg.to_string());
    }
    // Local .html files handed over by the file association.
    let path = std::path::Path::new(arg);
    if path.is_absolute() && path.exists() {
        let path = arg.replace('\\', "/").replace(' ', "%20");
        let path = path.trim_start_matches('/');
        return Some(format!("file:///{path}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn urls_are_recognized() {
        assert_eq!(normalize_url("https://example.com").as_deref(), Some("https://example.com"));
        assert_eq!(normalize_url("\"HTTP://a.b/c\"").as_deref(), Some("HTTP://a.b/c"));
        assert_eq!(normalize_url("--background"), None);
        assert_eq!(normalize_url("mailto:me@example.com"), None);
    }

    #[test]
    fn arrange_respects_order_and_hidden() {
        let b = |id: &str| Browser { id: id.into(), name: id.into(), icon: None, launch: Launch::default() };
        let config =
            Config { order: vec!["c".into(), "a".into()], hidden: vec!["a".into()], ..Config::default() };
        let out = arrange(vec![b("a"), b("b"), b("c")], &config);
        let ids: Vec<_> = out.iter().map(|v| v.browser.id.as_str()).collect();
        assert_eq!(ids, ["c", "a", "b"]);
        assert!(out[1].hidden && !out[0].hidden);
    }
}
