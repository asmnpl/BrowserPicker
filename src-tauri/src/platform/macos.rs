use std::process::Command;

use objc2::rc::{autoreleasepool, Retained};
use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSGraphicsContext, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{NSBundle, NSDictionary, NSFileManager, NSPoint, NSRect, NSSize, NSString, NSURL};
use tauri::WebviewWindow;

use crate::browsers::{png_data_url, Browser, Launch};

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

pub fn register() {}
