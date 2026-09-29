use std::process::Command;

use objc2::rc::{autoreleasepool, Retained};
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSEvent,
    NSGraphicsContext, NSScreen, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWorkspace,
};
use objc2_foundation::{NSBundle, NSDictionary, NSFileManager, NSPoint, NSRect, NSSize, NSString, NSURL};
use tauri::WebviewWindow;

use crate::browsers::{png_data_url, Browser, Launch};
use crate::config::Placement;
use crate::placement::{self, Rect};

const ICON_PX: isize = 128;

fn probe_url() -> Retained<NSURL> {
    NSURL::URLWithString(&NSString::from_str("https://example.com")).expect("valid URL")
}

fn own_bundle_id() -> Option<String> {
    NSBundle::mainBundle().bundleIdentifier().map(|id| id.to_string())
}

pub fn discover() -> Vec<Browser> {
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let own = own_bundle_id();
        let mut browsers: Vec<Browser> = Vec::new();
        for app in workspace.URLsForApplicationsToOpenURL(&probe_url()).iter() {
            let Some(path) = app.path() else { continue };
            let Some(bundle) = NSBundle::bundleWithURL(&app) else { continue };
            let Some(id) = bundle.bundleIdentifier().map(|s| s.to_string()) else { continue };
            if Some(&id) == own.as_ref() || browsers.iter().any(|b| b.id == id) {
                continue;
            }
            let name = NSFileManager::defaultManager().displayNameAtPath(&path).to_string();
            let name = name.strip_suffix(".app").unwrap_or(&name).to_string();
            browsers.push(Browser {
                icon: icon(&workspace, &path).map(|png| png_data_url(&png)),
                id,
                name,
                launch: Launch { path: path.to_string(), args: String::new() },
            });
        }
        browsers
    })
}

/// Renders the Finder icon of `path` into a PNG.
fn icon(workspace: &NSWorkspace, path: &NSString) -> Option<Vec<u8>> {
    let image = workspace.iconForFile(path);
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            ICON_PX,
            ICON_PX,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(ICON_PX as f64, ICON_PX as f64));
    image.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::Copy, 1.0);
    ctx.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    let data =
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    Some(data.to_vec())
}

pub fn launch(browser: &Browser, url: &str) -> std::io::Result<()> {
    Command::new("/usr/bin/open").arg("-a").arg(&browser.launch.path).arg(url).spawn().map(|_| ())
}

pub fn is_default() -> bool {
    let workspace = NSWorkspace::sharedWorkspace();
    let current = workspace.URLForApplicationToOpenURL(&probe_url()).and_then(|u| u.path());
    let own = NSBundle::mainBundle().bundlePath();
    current.is_some_and(|path| path.to_string() == own.to_string())
}

pub fn make_default() {
    let workspace = NSWorkspace::sharedWorkspace();
    let own = NSBundle::mainBundle().bundleURL();
    // Changing the http handler asks the user to switch the default web browser,
    // which covers https and HTML files as well.
    workspace.setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
        &own,
        &NSString::from_str("http"),
        None,
    );
}

/// Native touches for the picker: pop-in animation and showing over full-screen apps.
pub fn style_picker(window: &WebviewWindow) {
    let Ok(ptr) = window.ns_window() else { return };
    let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::AlertPanel);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient,
    );
}

/// Cocoa rects have a bottom-left origin; `placement` works with y growing downwards.
fn flipped(rect: NSRect) -> Rect {
    Rect {
        x: rect.origin.x,
        y: -(rect.origin.y + rect.size.height),
        width: rect.size.width,
        height: rect.size.height,
    }
}

fn describe(rect: NSRect) -> String {
    format!("({:.0}, {:.0}) {:.0}x{:.0}", rect.origin.x, rect.origin.y, rect.size.width, rect.size.height)
}

fn same_origin(a: NSRect, b: NSRect) -> bool {
    (a.origin.x - b.origin.x).abs() < 1.0 && (a.origin.y - b.origin.y).abs() < 1.0
}

/// Overwrites `~/Library/Logs/BrowserPicker/placement.log` with the last placement,
/// so multi-display issues can be diagnosed from a user's report.
fn log_placement(lines: &[String]) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let dir = std::path::Path::new(&home).join("Library/Logs/BrowserPicker");
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("placement.log"), lines.join("\n") + "\n");
    }
}

/// Places the picker (`width`×`height` points) on the display under the cursor and shows it.
pub fn present_picker(window: &WebviewWindow, width: f64, height: f64, placement: Placement) {
    let handle = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Ok(ptr) = handle.ns_window() else { return };
        let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };

        let mouse = NSEvent::mouseLocation();
        let cursor = (mouse.x, -mouse.y);
        let screens = NSScreen::screens(mtm);
        let mut log = vec![
            format!("version {}", env!("CARGO_PKG_VERSION")),
            format!("placement {placement:?}, size {width:.0}x{height:.0}"),
            format!("mouse ({:.0}, {:.0})", mouse.x, mouse.y),
            format!("separate spaces {}", NSScreen::screensHaveSeparateSpaces(mtm)),
        ];
        for (i, screen) in screens.iter().enumerate() {
            log.push(format!(
                "screen {i}: frame {} visible {}",
                describe(screen.frame()),
                describe(screen.visibleFrame())
            ));
        }

        let screen = screens
            .iter()
            .find(|screen| flipped(screen.frame()).contains(cursor.0, cursor.1))
            .or_else(|| NSScreen::mainScreen(mtm));
        let target = screen.map(|screen| {
            let area = flipped(screen.visibleFrame());
            let (x, top) = placement::position(area, cursor, width, height, placement, placement::MARGIN);
            NSRect::new(NSPoint::new(x, -top - height), NSSize::new(width, height))
        });
        if let Some(frame) = target {
            log.push(format!("target {}", describe(frame)));
            ns_window.setFrame_display(frame, true);
        } else {
            log.push("no screen found".into());
        }

        let _ = handle.show();
        let _ = handle.set_focus();

        // Ordering the window in or activating the app can move it back to the
        // main display; put it where it belongs if that happened.
        if let Some(frame) = target {
            let shown = ns_window.frame();
            log.push(format!("after show {}", describe(shown)));
            if !same_origin(shown, frame) {
                ns_window.setFrame_display(frame, true);
            }
        }
        log_placement(&log);

        // Check once more after AppKit has settled the activation.
        let Some(frame) = target else { return };
        let later = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let window = later.clone();
            let _ = later.run_on_main_thread(move || {
                let Ok(ptr) = window.ns_window() else { return };
                let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
                let settled = ns_window.frame();
                if !same_origin(settled, frame) {
                    ns_window.setFrame_display(frame, true);
                }
                log.push(format!("after 100ms {}", describe(settled)));
                log_placement(&log);
            });
        });
    });
}

pub fn register() {}
