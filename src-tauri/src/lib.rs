mod browsers;
mod config;
mod placement;
mod platform;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::window::EffectState;
#[cfg(any(target_os = "macos", windows))]
use tauri::{utils::config::WindowEffectsConfig, window::Effect};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::{PhysicalPosition, PhysicalSize};
use tauri_plugin_autostart::ManagerExt as _;

use browsers::{Browser, BrowserView};
use config::{Config, Placement};

const PICKER: &str = "picker";
const SETTINGS: &str = "settings";
const BACKGROUND_ARG: &str = "--background";

#[cfg(target_os = "macos")]
const PLATFORM: &str = "macos";
#[cfg(windows)]
const PLATFORM: &str = "windows";
#[cfg(not(any(target_os = "macos", windows)))]
const PLATFORM: &str = "linux";

struct AppState {
    config: Mutex<Config>,
    browsers: Mutex<Vec<Browser>>,
    picker: Mutex<PickerState>,
    /// Set once any link was received; used to tell a plain launch from a link launch.
    got_url: AtomicBool,
}

#[derive(Default)]
struct PickerState {
    /// The picker page finished loading and listens for events.
    loaded: bool,
    /// A link that arrived before the page was ready.
    pending: Option<String>,
}

impl AppState {
    fn views(&self) -> Vec<BrowserView> {
        let browsers = self.browsers.lock().unwrap().clone();
        browsers::arrange(browsers, &self.config.lock().unwrap())
    }

    fn update_config(&self, app: &AppHandle, change: impl FnOnce(&mut Config)) {
        let mut config = self.config.lock().unwrap();
        change(&mut config);
        config.save(app);
    }
}

// ---------------------------------------------------------------------------
// Link handling

fn handle_url(app: &AppHandle, url: String) {
    let state = app.state::<AppState>();
    state.got_url.store(true, Ordering::SeqCst);

    let visible: Vec<Browser> = state.views().into_iter().filter(|v| !v.hidden).map(|v| v.browser).collect();
    // Nothing to choose from: skip the picker entirely.
    if visible.len() == 1 {
        if let Err(err) = platform::launch(&visible[0], &url) {
            eprintln!("failed to open {url}: {err}");
        }
        return;
    }

    let mut picker = state.picker.lock().unwrap();
    if picker.loaded {
        drop(picker);
        let _ = app.emit_to(PICKER, "picker:open", url);
    } else {
        picker.pending = Some(url);
    }
}

#[cfg(not(target_os = "macos"))]
fn handle_args(app: &AppHandle, args: &[String]) {
    match args.iter().find_map(|arg| browsers::normalize_url(arg)) {
        Some(url) => handle_url(app, url),
        None if !args.iter().any(|a| a == BACKGROUND_ARG) => open_settings(app),
        None => {}
    }
}

// ---------------------------------------------------------------------------
// Windows

fn create_picker(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, PICKER, WebviewUrl::App("picker.html".into()))
        .title("BrowserPicker")
        .inner_size(420.0, 180.0)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .focused(false);

    #[cfg(target_os = "macos")]
    let builder = builder.shadow(true).accept_first_mouse(true).visible_on_all_workspaces(true).effects(
        WindowEffectsConfig {
            effects: vec![Effect::Popover],
            state: Some(EffectState::Active),
            radius: Some(14.0),
            ..Default::default()
        },
    );

    // Windows draws the panel, its rounded corners and shadow in CSS.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.shadow(false);

    let window = builder.build()?;
    platform::style_picker(&window);
    Ok(window)
}

pub(crate) fn open_settings(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let builder = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings.html".into()))
        .title("BrowserPicker")
        .inner_size(520.0, 740.0)
        .min_inner_size(460.0, 480.0)
        .maximizable(false)
        .skip_taskbar(true)
        .center()
        .visible(false);

    #[cfg(target_os = "macos")]
    let builder =
        builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).transparent(true).effects(
            WindowEffectsConfig {
                effects: vec![Effect::UnderWindowBackground],
                state: Some(EffectState::FollowsWindowActiveState),
                ..Default::default()
            },
        );

    #[cfg(windows)]
    let builder = if platform::has_mica() {
        builder
            .transparent(true)
            .effects(WindowEffectsConfig { effects: vec![Effect::Mica], ..Default::default() })
    } else {
        builder
    };

    if let Err(err) = builder.build() {
        eprintln!("failed to open settings: {err}");
    }
}

/// Hides the picker and hands focus back to whatever the user was doing.
fn hide_picker_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PICKER) {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    if app.get_webview_window(SETTINGS).is_none_or(|w| !w.is_visible().unwrap_or(false)) {
        let _ = app.hide();
    }
}

// ---------------------------------------------------------------------------
// Commands used by the UI

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerInit {
    platform: &'static str,
    pending: Option<String>,
}

#[tauri::command]
fn picker_loaded(state: State<AppState>) -> PickerInit {
    let mut picker = state.picker.lock().unwrap();
    picker.loaded = true;
    PickerInit { platform: PLATFORM, pending: picker.pending.take() }
}

#[tauri::command]
fn get_browsers(state: State<AppState>, include_hidden: bool) -> Vec<BrowserView> {
    state.views().into_iter().filter(|v| include_hidden || !v.hidden).collect()
}

/// Sizes the picker to its content (logical px), places it and shows it.
#[tauri::command]
fn present_picker(app: AppHandle, state: State<AppState>, width: f64, height: f64) -> Result<(), String> {
    let window = app.get_webview_window(PICKER).ok_or("picker window is missing")?;
    let placement = state.config.lock().unwrap().placement;

    // AppKit's own screen geometry is used on macOS: tao reports the cursor in
    // primary-display pixels but looks monitors up in points, which sends the
    // picker to the main display whenever the cursor is on another one.
    #[cfg(target_os = "macos")]
    {
        // Undo the app-level hide from the last dismissal.
        let _ = app.show();
        platform::present_picker(&window, width, height, placement);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let cursor = app.cursor_position().unwrap_or_default();
        let monitor = app
            .monitor_from_point(cursor.x, cursor.y)
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())
            .ok_or("no monitor found")?;

        let scale = monitor.scale_factor();
        let (width, height) = ((width * scale).ceil(), (height * scale).ceil());
        let area = monitor.work_area();
        let area = placement::Rect {
            x: area.position.x as f64,
            y: area.position.y as f64,
            width: area.size.width as f64,
            height: area.size.height as f64,
        };
        let (x, y) = placement::position(
            area,
            (cursor.x, cursor.y),
            width,
            height,
            placement,
            placement::MARGIN * scale,
        );

        window
            .set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
            .map_err(|e| e.to_string())?;
        window.set_size(PhysicalSize::new(width as u32, height as u32)).map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[tauri::command]
fn hide_picker(app: AppHandle) {
    hide_picker_window(&app);
}

#[tauri::command]
fn open_url(state: State<AppState>, id: String, url: String) -> Result<(), String> {
    let browser =
        state.browsers.lock().unwrap().iter().find(|b| b.id == id).cloned().ok_or("browser not found")?;
    platform::launch(&browser, &url).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    platform: &'static str,
    version: String,
    show_tray: bool,
    placement: Placement,
    launch_at_login: bool,
    is_default: bool,
    material: bool,
}

#[tauri::command]
fn get_settings(app: AppHandle, state: State<AppState>) -> SettingsView {
    let config = state.config.lock().unwrap().clone();
    SettingsView {
        platform: PLATFORM,
        version: app.package_info().version.to_string(),
        show_tray: config.show_tray,
        placement: config.placement,
        launch_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        is_default: platform::is_default(),
        #[cfg(target_os = "macos")]
        material: true,
        #[cfg(windows)]
        material: platform::has_mica(),
        #[cfg(not(any(target_os = "macos", windows)))]
        material: false,
    }
}

#[tauri::command]
fn settings_ready(window: WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

#[tauri::command]
fn set_show_tray(app: AppHandle, state: State<AppState>, visible: bool) {
    state.update_config(&app, |c| c.show_tray = visible);
    tray::set_visible(&app, visible);
}

#[tauri::command]
fn set_placement(app: AppHandle, state: State<AppState>, placement: Placement) {
    state.update_config(&app, |c| c.placement = placement);
}

#[tauri::command]
fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let autostart = app.autolaunch();
    let result = if enabled { autostart.enable() } else { autostart.disable() };
    result.map_err(|e| e.to_string())?;
    autostart.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_browser_order(app: AppHandle, state: State<AppState>, ids: Vec<String>) {
    state.update_config(&app, |c| c.order = ids);
    let _ = app.emit_to(PICKER, "browsers:changed", ());
}

#[tauri::command]
fn set_browser_hidden(app: AppHandle, state: State<AppState>, id: String, hidden: bool) {
    state.update_config(&app, |c| {
        c.hidden.retain(|h| h != &id);
        if hidden {
            c.hidden.push(id);
        }
    });
    let _ = app.emit_to(PICKER, "browsers:changed", ());
}

/// Runs on the main thread, where the platform icon APIs are happiest.
#[tauri::command]
fn rescan_browsers(app: AppHandle, state: State<AppState>) -> Vec<BrowserView> {
    *state.browsers.lock().unwrap() = platform::discover();
    let _ = app.emit_to(PICKER, "browsers:changed", ());
    state.views()
}

#[tauri::command]
fn is_default_browser() -> bool {
    platform::is_default()
}

#[tauri::command]
fn make_default_browser() {
    platform::make_default();
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    platform::allow_foreground();

    let builder = tauri::Builder::default();

    // macOS delivers links to the running app through Apple Events instead.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        handle_args(app, argv.get(1..).unwrap_or_default());
    }));

    let app = builder
        .plugin(tauri_plugin_autostart::Builder::new().app_name("BrowserPicker").arg(BACKGROUND_ARG).build())
        .invoke_handler(tauri::generate_handler![
            picker_loaded,
            get_browsers,
            present_picker,
            hide_picker,
            open_url,
            get_settings,
            settings_ready,
            set_show_tray,
            set_placement,
            set_launch_at_login,
            set_browser_order,
            set_browser_hidden,
            rescan_browsers,
            is_default_browser,
            make_default_browser,
            quit,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let (config, existed) = Config::load(&handle);
            if !existed {
                config.save(&handle);
            }
            let show_tray = config.show_tray;
            platform::register();
            app.manage(AppState {
                config: Mutex::new(config),
                browsers: Mutex::new(platform::discover()),
                picker: Mutex::new(PickerState::default()),
                got_url: AtomicBool::new(false),
            });

            let picker = create_picker(&handle)?;
            picker.on_window_event({
                let handle = handle.clone();
                move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_picker_window(&handle);
                    }
                }
            });
            if show_tray {
                tray::set_visible(&handle, true);
            }

            let args: Vec<String> = std::env::args().skip(1).collect();
            #[cfg(not(target_os = "macos"))]
            handle_args(&handle, &args);

            // On macOS a link launch arrives as an event shortly after startup,
            // so only open settings when none came in.
            #[cfg(target_os = "macos")]
            if !args.iter().any(|a| a == BACKGROUND_ARG) {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    if !handle.state::<AppState>().got_url.load(Ordering::SeqCst) {
                        let app = handle.clone();
                        let _ = handle.run_on_main_thread(move || open_settings(&app));
                    }
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build BrowserPicker");

    app.run(on_run_event);
}

#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        // Keep running in the background when windows close; only `quit` exits.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            for url in urls {
                handle_url(app, url.to_string());
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => open_settings(app),
        _ => {}
    }
}
