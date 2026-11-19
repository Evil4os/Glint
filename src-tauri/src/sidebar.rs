//! A companion window aligned to the Codex rail. This never changes Codex files
//! or reads chat content. Window bounds come from WindowServer, in points;
//! update priority comes from read-only accessibility queries or staged bundle metadata.

use crate::update_priority::{self, UpdatePriority, UpdateTarget};

use block2::RcBlock;
use objc2::{rc::Retained, runtime::ProtocolObject, MainThreadMarker};
use objc2_app_kit::{
    NSEvent, NSFloatingWindowLevel, NSScreen, NSWindow,
    NSWindowCollectionBehavior, NSWindowOrderingMode, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification, NSRunningApplication,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidDeactivateApplicationNotification,
};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType, CGRect};
use objc2_core_graphics::{
    kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize};
use std::{
    cell::RefCell,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Listener, Manager};

pub const COLLAPSED_SIDE: f64 = 52.0;
const EXPANDED_SIDE: f64 = 288.0;
const COLLAPSED_HALO_PADDING: f64 = 40.0;
const EXPANDED_HALO_PADDING: f64 = 110.0;
// Keep the soft halo's transparent 52 pt click target on the shared center
// and 44 pt vertical rhythm of the official controls.
const RAIL_CENTER_X: f64 = 26.0;
const BADGE_CENTER_FROM_BOTTOM: f64 = 114.0;
const SETTLE_TIME: Duration = Duration::from_millis(350);

thread_local! {
    // Keep the observer on the main thread, like the AppKit notification center.
    static WORKSPACE_OBSERVERS: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn contains(self, other: Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }

    fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let width = (self.x + self.width).min(other.x + other.width) - x;
        let height = (self.y + self.height).min(other.y + other.height) - y;
        (width > 0.0 && height > 0.0).then_some(Self { x, y, width, height })
    }

    fn contains_tool_surface(self, other: Self) -> bool {
        if self.contains(other) && self != other {
            return true;
        }
        // Browser surfaces can protrude beyond the host's edge. Treat a much
        // smaller, mostly overlapping surface as a child even in that case;
        // do not let a similarly sized independent main window take its place.
        let area = other.width * other.height;
        let overlap_width = (self.x + self.width).min(other.x + other.width)
            - self.x.max(other.x);
        let overlap_height = (self.y + self.height).min(other.y + other.height)
            - self.y.max(other.y);
        area > 0.0
            && self.width * self.height >= area * 1.5
            && overlap_width.max(0.0) * overlap_height.max(0.0) >= area * 0.8
    }

    fn native(self) -> NSRect {
        NSRect::new(
            NSPoint::new(self.x, self.y),
            NSSize::new(self.width, self.height),
        )
    }
}

impl From<NSRect> for Rect {
    fn from(value: NSRect) -> Self {
        Self {
            x: value.origin.x,
            y: value.origin.y,
            width: value.size.width,
            height: value.size.height,
        }
    }
}

fn from_window_server(bounds: CGRect, primary_screen_top: f64) -> Rect {
    // CG uses the primary display's top-left origin; AppKit uses bottom-left.
    // Do not use the current display's height: secondary displays can be offset.
    Rect {
        x: bounds.origin.x,
        y: primary_screen_top - bounds.origin.y - bounds.size.height,
        width: bounds.size.width,
        height: bounds.size.height,
    }
}

fn collapsed_frame(host: Rect) -> Rect {
    Rect {
        x: host.x + RAIL_CENTER_X - COLLAPSED_SIDE / 2.0,
        y: host.y + BADGE_CENTER_FROM_BOTTOM - COLLAPSED_SIDE / 2.0,
        width: COLLAPSED_SIDE,
        height: COLLAPSED_SIDE,
    }
}

fn halo_frame(core: Rect, expanded: bool) -> Rect {
    let padding = if expanded { EXPANDED_HALO_PADDING } else { COLLAPSED_HALO_PADDING };
    Rect {
        x: core.x - padding,
        y: core.y - padding,
        width: core.width + 2.0 * padding,
        height: core.height + 2.0 * padding,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HaloViewport {
    padding: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl HaloViewport {
    fn script(self) -> String {
        format!("{{const s=document.documentElement?.style;if(s){{s.setProperty('--native-halo-padding','{}px');s.setProperty('--native-halo-left','{}px');s.setProperty('--native-halo-top','{}px');s.setProperty('--native-halo-right','{}px');s.setProperty('--native-halo-bottom','{}px');}}}}",
            self.padding, self.left, self.top, self.right, self.bottom)
    }
}

fn halo_viewport(core: Rect, host: Rect, screen: Rect, expanded: bool) -> HaloViewport {
    let mut viewport = HaloViewport {
        padding: if expanded { EXPANDED_HALO_PADDING } else { COLLAPSED_HALO_PADDING },
        left: 0.0, top: 0.0, right: 0.0, bottom: 0.0,
    };
    if expanded {
        let halo = halo_frame(core, true);
        let visible = halo.intersection(host).and_then(|rect| rect.intersection(screen))
            .unwrap_or(Rect { width: 0.0, height: 0.0, ..halo });
        viewport.left = visible.x - halo.x;
        viewport.bottom = visible.y - halo.y;
        viewport.right = halo.x + halo.width - visible.x - visible.width;
        viewport.top = halo.y + halo.height - visible.y - visible.height;
    }
    viewport
}

pub fn create_halo(app: &AppHandle) -> tauri::Result<()> {
    let halo = tauri::WebviewWindowBuilder::new(app, "halo", tauri::WebviewUrl::App("index.html".into()))
        .title("Glint Halo")
        .inner_size(132.0, 132.0)
        .max_inner_size(640.0, 640.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .focusable(false)
        .focused(false)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    halo.set_ignore_cursor_events(true)?;
    if let Some(widget) = app.get_webview_window("widget") {
        let handle = app.clone();
        widget.listen("quota-halo-ready", move |_| {
            handle.state::<SidebarState>().halo_ready.store(true, Ordering::Relaxed);
            wake_sync(&handle);
            crate::log_lifecycle(&handle, "halo_renderer_ready", "presentation_events=connected");
        });
    }
    crate::log_lifecycle(app, "halo_created", "mouse_events=ignored collapsed_canvas=132 expanded_canvas=508");
    Ok(())
}

fn dock_frame(host: Rect, screen: Rect, expanded: bool) -> Option<Rect> {
    let badge = collapsed_frame(host);
    // Hide when the rail itself is offscreen instead of placing the badge over
    // unrelated content. Keep the expanded card inside the same display.
    if !screen.contains(badge) {
        return None;
    }
    if !expanded {
        return Some(badge);
    }
    let bounds = host.intersection(screen)?;
    if bounds.width < EXPANDED_SIDE || bounds.height < EXPANDED_SIDE {
        return None;
    }
    Some(Rect {
        x: badge
            .x
            .clamp(bounds.x, bounds.x + bounds.width - EXPANDED_SIDE),
        y: badge
            .y
            .clamp(bounds.y, bounds.y + bounds.height - EXPANDED_SIDE),
        width: EXPANDED_SIDE,
        height: EXPANDED_SIDE,
    })
}

#[derive(Default)]
struct Layout {
    expanded: bool,
    visible: bool,
    hidden_reason: Option<&'static str>,
    target_id: Option<u32>,
    motion: MotionGate,
    halo_viewport: Option<HaloViewport>,
}

impl Layout {
    fn hide(&mut self, host: Option<HostWindow>, reason: &'static str) {
        self.expanded = false;
        self.visible = false;
        self.hidden_reason = Some(reason);
        // A settling window is still our host. Keep it available if dismissing
        // the badge leaves the accessory app in front. An unrelated app has no
        // target and clears this association.
        self.target_id = host.map(|host| host.id);
    }
}

#[derive(Default)]
pub struct SidebarState {
    layout: Mutex<Layout>,
    queued: AtomicBool,
    stopped: AtomicBool,
    space_changed: AtomicBool,
    tracking: AtomicBool,
    halo_ready: AtomicBool,
    worker: Mutex<Option<std::thread::Thread>>,
    updates: UpdatePriority,
}

#[derive(Default)]
struct MotionGate {
    candidate: Option<HostWindow>,
    stable_since: Option<Instant>,
    accepted: Option<HostWindow>,
    anchor_unsettled: bool,
    blocked_until: Option<Instant>,
}

impl MotionGate {
    fn clear_target(&mut self) {
        self.candidate = None;
        self.stable_since = None;
        self.accepted = None;
        self.anchor_unsettled = false;
    }

    fn space_changed(&mut self, now: Instant) {
        self.clear_target();
        self.blocked_until = Some(now + SETTLE_TIME);
    }

    fn accepts(&mut self, host: HostWindow, mouse_down: bool, now: Instant) -> bool {
        let dragging_host = mouse_down && self.accepted.is_some_and(|last| last.id == host.id);
        if self.accepted.is_some_and(|last| collapsed_frame(last.frame) != collapsed_frame(host.frame)) {
            self.anchor_unsettled = true;
        }
        let same_anchor = !self.anchor_unsettled && self.accepted.is_some_and(|last| {
            collapsed_frame(last.frame) == collapsed_frame(host.frame)
        });
        if self.candidate != Some(host) {
            self.candidate = Some(host);
            self.stable_since = Some(now);
        }
        if self.blocked_until.is_some_and(|deadline| now < deadline) {
            return false;
        }
        // WindowServer can report intermediate, horizontally translated bounds
        // during a Spaces swipe. Only a real mouse drag may track those changes
        // live; other movement must settle before we position or show the badge.
        // Switching chats can replace a native window without moving its rail.
        // Its existing badge position is already safe, even if the ID or size
        // changes. Explicit Space changes still take the blocked path above.
        if dragging_host || same_anchor
            || self
                .stable_since
                .is_some_and(|since| now.duration_since(since) >= SETTLE_TIME)
        {
            self.accepted = Some(host);
            self.anchor_unsettled = false;
            true
        } else {
            false
        }
    }
}

fn number(dict: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<i64> {
    dict.get(key)?.downcast::<CFNumber>().ok()?.as_i64()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HostWindow {
    id: u32,
    frame: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TargetScope {
    Process(i64),
    RetainedWindow(u32),
}

fn target_scope(
    is_codex: bool,
    is_widget: bool,
    foreground_pid: i64,
    retained_id: Option<u32>,
) -> Option<TargetScope> {
    if is_codex {
        Some(TargetScope::Process(foreground_pid))
    } else if is_widget {
        // Clicking the companion can activate its accessory application even
        // when the window can't become key. Keep the same Codex host in that
        // case; only unrelated apps should dismiss the companion.
        retained_id.map(TargetScope::RetainedWindow)
    } else {
        None
    }
}

impl TargetScope {
    fn matches(self, owner_pid: Option<i64>, window_id: Option<i64>) -> bool {
        match self {
            Self::Process(pid) => owner_pid == Some(pid),
            Self::RetainedWindow(id) => window_id == Some(i64::from(id)),
        }
    }
}

fn select_host_window(
    windows: impl IntoIterator<Item = (HostWindow, i32)>,
) -> Option<(HostWindow, i32)> {
    // Project popovers and other transient surfaces also appear as layer-zero
    // Codex windows. Browser/tool surfaces may be large enough to pass the size
    // check, but overlap the main window. Prefer their containing host, then
    // retain front-to-back order between independent main windows. Identical
    // frames are allowed while navigation replaces a native window.
    let windows: Vec<_> = windows.into_iter().filter(|(window, _)| {
        window.frame.width >= 480.0 && window.frame.height >= 360.0
    }).collect();
    windows.iter().copied().find(|(window, pid)| {
        !windows.iter().any(|(other, other_pid)| {
            other_pid == pid && other.frame.contains_tool_surface(window.frame)
        })
    })
}

fn target_window(
    primary_screen_top: f64,
    retained_id: Option<u32>,
) -> Result<(HostWindow, i32), &'static str> {
    let foreground = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .ok_or("reason=foreground_app_unavailable")?;
    let pid = foreground.processIdentifier() as i64;
    let scope = target_scope(
        foreground
            .bundleIdentifier()
            .is_some_and(|id| id.to_string() == "com.openai.codex"),
        pid == i64::from(std::process::id()),
        pid,
        retained_id,
    ).ok_or(if pid == i64::from(std::process::id()) {
        "reason=widget_host_unavailable"
    } else {
        "reason=foreground_app_inactive"
    })?;
    let windows = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    ).ok_or("reason=window_list_unavailable")?;
    // WindowServer guarantees CFDictionary elements with CFType values.
    let windows =
        unsafe { CFRetained::cast_unchecked::<objc2_core_foundation::CFArray<CFType>>(windows) };
    let candidates = windows.iter().filter_map(|entry| {
        let dict = entry.downcast_ref::<CFDictionary>()?;
        // Window info dictionaries have CFString keys and CFType values.
        let dict = unsafe { dict.cast_unchecked::<CFString, CFType>() };
        // These constant keys belong to CoreGraphics and are immutable.
        let (owner, layer, id, bounds) = unsafe {
            (
                number(dict, kCGWindowOwnerPID),
                number(dict, kCGWindowLayer),
                number(dict, kCGWindowNumber),
                dict.get(kCGWindowBounds),
            )
        };
        if !scope.matches(owner, id) || layer != Some(0) {
            return None;
        }
        let owner_pid = i32::try_from(owner?).ok()?;
        if !NSRunningApplication::runningApplicationWithProcessIdentifier(owner_pid)
            .and_then(|application| application.bundleIdentifier())
            .is_some_and(|bundle| bundle.to_string() == "com.openai.codex")
        {
            return None;
        }
        let bounds = bounds?.downcast::<CFDictionary>().ok()?;
        let mut rect = CGRect::ZERO;
        if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds), &mut rect) } {
            return None;
        }
        Some((HostWindow {
            id: u32::try_from(id?).ok()?,
            frame: from_window_server(rect, primary_screen_top),
        }, owner_pid))
    });
    select_host_window(candidates).ok_or("reason=main_window_unavailable")
}

/// Must run on the main thread. All AppKit access stays on that thread.
fn sync(app: &AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(window) = app.get_webview_window("widget") else {
        return;
    };
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    // `window` keeps this native window alive throughout this main-thread call.
    let native = unsafe { &*(pointer as *const NSWindow) };
    let halo_window = app.get_webview_window("halo");
    let halo_pointer = halo_window.as_ref().and_then(|window| window.ns_window().ok());
    let halo = halo_pointer.map(|pointer| unsafe { &*(pointer as *const NSWindow) });
    let state = app.state::<SidebarState>();
    let Ok(mut layout) = state.layout.lock() else {
        return;
    };
    let now = Instant::now();
    if state.halo_ready.swap(false, Ordering::Relaxed) {
        layout.halo_viewport = None;
    }
    if state.space_changed.swap(false, Ordering::Relaxed) {
        layout.motion.space_changed(now);
        state.updates.clear();
    }
    let screens = NSScreen::screens(mtm);
    let primary_top = screens.firstObject().map(|primary| {
        let primary = primary.frame();
        primary.origin.y + primary.size.height
    });
    let resolution = primary_top
        .ok_or("reason=display_unavailable")
        .and_then(|top| target_window(top, layout.target_id));
    let target = resolution.ok();
    state.tracking.store(target.is_some(), Ordering::Relaxed);
    if target.is_none() {
        layout.motion.clear_target();
        state.updates.clear();
    }
    let mut hidden_reason = resolution.err().unwrap_or("reason=host_window_unavailable");
    let placement = target.and_then(|(host, pid)| {
        let yields_to_update = state.updates.should_yield(UpdateTarget {
            pid,
            window_id: host.id,
            x: (host.frame.x + RAIL_CENTER_X) as f32,
            y: (primary_top? - host.frame.y - BADGE_CENTER_FROM_BOTTOM) as f32,
        });
        hidden_reason = "reason=host_window_settling";
        if !layout
            .motion
            .accepts(host, NSEvent::pressedMouseButtons() & 1 != 0, now)
        {
            return None;
        }
        if yields_to_update {
            hidden_reason = "reason=app_update_priority";
            return None;
        }
        hidden_reason = "reason=rail_offscreen";
        let screen = screens
            .iter()
            .map(|screen| Rect::from(screen.visibleFrame()))
            .find(|screen| screen.contains(collapsed_frame(host.frame)))?;
        if layout.target_id != Some(host.id) {
            crate::log_lifecycle(app, "sidebar_target_changed", &format!(
                "window_id={} previous_id={:?} frame={:?}", host.id, layout.target_id, host.frame,
            ));
            if layout.expanded {
                let _ = app.emit_to("widget", "widget-collapsed", ());
            }
            layout.expanded = false;
        }
        dock_frame(host.frame, screen, layout.expanded).map(|frame| {
            (host.id, frame, halo_viewport(frame, host.frame, screen, layout.expanded))
        })
    });

    if let Some((id, frame, viewport)) = placement {
        let target = frame.native();
        if native.frame() != target {
            let resized = native.frame().size != target.size;
            native.setFrame_display(target, false);
            if resized {
                crate::log_lifecycle(
                    app,
                    "widget_resized",
                    if layout.expanded {
                        "state=expanded side=288"
                    } else {
                        "state=collapsed side=52"
                    },
                );
            }
        }
        let showing_widget = !native.isVisible();
        if showing_widget {
            native.orderFrontRegardless();
        }
        if let Some(halo) = halo {
            let halo_target = halo_frame(frame, layout.expanded).native();
            if halo.frame() != halo_target {
                let resized = halo.frame().size != halo_target.size;
                halo.setFrame_display(halo_target, false);
                if resized {
                    crate::log_lifecycle(app, "halo_resized", if layout.expanded {
                        "canvas=508 core=288 mouse_events=ignored"
                    } else {
                        "canvas=132 core=52 mouse_events=ignored"
                    });
                }
            }
            if layout.halo_viewport != Some(viewport) {
                if let Some(webview) = halo_window.as_ref() {
                    if webview.eval(&viewport.script()).is_ok() {
                        layout.halo_viewport = Some(viewport);
                        crate::log_lifecycle(app, "halo_clip_updated", &format!(
                            "padding={} left={} top={} right={} bottom={}",
                            viewport.padding, viewport.left, viewport.top, viewport.right, viewport.bottom,
                        ));
                    }
                }
            }
            // Both surfaces stay above Codex. The decorative surface always
            // passes input through; the original widget remains the click target.
            if showing_widget || !halo.isVisible() {
                halo.orderWindow_relativeTo(NSWindowOrderingMode::Below, native.windowNumber());
            }
        }
        if !layout.visible {
            crate::log_lifecycle(
                app,
                "sidebar_attached",
                "side=52 shape=soft-halo anchor=bottom-left",
            );
        }
        layout.visible = true;
        layout.hidden_reason = None;
        layout.target_id = Some(id);
    } else {
        if native.isVisible() {
            native.orderOut(None);
        }
        if let Some(halo) = halo {
            if halo.isVisible() { halo.orderOut(None); }
        }
        if layout.visible || layout.expanded {
            let _ = app.emit_to("widget", "widget-collapsed", ());
        }
        if layout.hidden_reason != Some(hidden_reason) {
            let foreground = NSWorkspace::sharedWorkspace().frontmostApplication();
            let bundle = foreground.as_ref().and_then(|app| app.bundleIdentifier())
                .map(|bundle| bundle.to_string()).unwrap_or_else(|| "unknown".into());
            crate::log_lifecycle(app, "sidebar_hidden", &format!(
                "{hidden_reason} foreground_bundle={bundle} foreground_pid={:?} candidate={:?} accepted={:?} retained_id={:?}",
                foreground.map(|app| app.processIdentifier()),
                layout.motion.candidate,
                layout.motion.accepted,
                layout.target_id,
            ));
        }
        // Retain motion history while hidden, otherwise every tick would restart
        // settling and the badge would never return after a Space transition.
        layout.hide(target.map(|(host, _)| host), hidden_reason);
    }
}

pub fn set_expanded(app: &AppHandle, expanded: bool) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Ok(mut layout) = handle.state::<SidebarState>().layout.lock() {
            layout.expanded = expanded;
        }
        sync(&handle);
    })
    .map_err(|error| format!("failed to resize sidebar badge: {error}"))
}

pub fn apply_mode(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or("widget window missing")?;
    let pointer = window.ns_window().map_err(|error| error.to_string())? as usize;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Ok(mut layout) = handle.state::<SidebarState>().layout.lock() {
            *layout = Layout::default();
        }
        handle.state::<SidebarState>().updates.clear();
        let _ = handle.emit_to("widget", "widget-collapsed", ());
        let native = unsafe { &*(pointer as *const NSWindow) };
        if let Some(halo_window) = handle.get_webview_window("halo") {
            if let Ok(pointer) = halo_window.ns_window() {
                let halo = unsafe { &*(pointer as *const NSWindow) };
                halo.setLevel(NSFloatingWindowLevel);
                halo.setHidesOnDeactivate(false);
                halo.setPreservesContentDuringLiveResize(true);
                halo.setIgnoresMouseEvents(true);
                halo.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::Stationary
                        | NSWindowCollectionBehavior::FullScreenAuxiliary
                        | NSWindowCollectionBehavior::IgnoresCycle,
                );
                halo.orderOut(None);
            }
        }
        native.setLevel(NSFloatingWindowLevel);
        native.setHidesOnDeactivate(false);
        native.setPreservesContentDuringLiveResize(true);
        native.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        native.orderOut(None);
        sync(&handle);
    })
    .map_err(|error| format!("failed to apply sidebar mode: {error}"))
}

fn queue_sync(app: &AppHandle) {
    let state = app.state::<SidebarState>();
    if state.stopped.load(Ordering::Relaxed)
        || state.queued.swap(true, Ordering::Relaxed) { return; }
    let handle = app.clone();
    if app.run_on_main_thread(move || {
        sync(&handle);
        handle.state::<SidebarState>().queued.store(false, Ordering::Relaxed);
    }).is_err() {
        state.queued.store(false, Ordering::Relaxed);
    }
}

fn wake_sync(app: &AppHandle) {
    if let Ok(worker) = app.state::<SidebarState>().worker.lock() {
        if let Some(worker) = worker.as_ref() { worker.unpark(); }
    }
}

pub fn start(app: &AppHandle) {
    let notification_handle = app.clone();
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(state) = notification_handle.try_state::<SidebarState>() {
            state.space_changed.store(true, Ordering::Relaxed);
        }
        wake_sync(&notification_handle);
    });
    // NSWorkspace posts to its own notification center. The center copies this
    // sendable block; it only sets a flag/wakes the worker, avoiding reentrant
    // AppKit calls while a window is being ordered or a layout lock is held.
    let observer = unsafe {
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceActiveSpaceDidChangeNotification),
                None,
                None,
                &block,
            )
    };
    WORKSPACE_OBSERVERS.with(|slot| slot.borrow_mut().push(observer));
    // Activation notifications keep show/hide responsive even when the idle
    // positioning timer is sleeping at its lower background frequency.
    // These are immutable AppKit notification-name constants.
    for name in unsafe { [NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidDeactivateApplicationNotification] } {
        let handle = app.clone();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| wake_sync(&handle));
        let observer = unsafe {
            NSWorkspace::sharedWorkspace().notificationCenter()
                .addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        WORKSPACE_OBSERVERS.with(|slot| slot.borrow_mut().push(observer));
    }
    let probe_handle = app.clone();
    std::thread::spawn(move || {
        let mut previous_trust = None;
        loop {
            let state = probe_handle.state::<SidebarState>();
            if state.stopped.load(Ordering::Relaxed) { break; }
            let trusted = update_priority::is_trusted();
            if previous_trust != Some(trusted) {
                crate::log_lifecycle(&probe_handle, "update_priority_permission", if trusted { "accessibility=granted" } else { "accessibility=optional_unavailable" });
                previous_trust = Some(trusted);
            }
            let target = state.updates.requested_target();
            if let Some(target) = target {
                let result = update_priority::probe(target, trusted);
                if state.updates.record(target, result) {
                    let source = if trusted { "accessibility" } else { "sparkle_stage" };
                    crate::log_lifecycle(&probe_handle, "update_priority", &format!("{} source={source}", result.log_value()));
                    wake_sync(&probe_handle);
                }
            }
            std::thread::sleep(Duration::from_millis(if trusted { 200 } else { 250 }));
        }
    });
    let handle = app.clone();
    std::thread::spawn(move || {
        if let Ok(mut worker) = handle.state::<SidebarState>().worker.lock() {
            *worker = Some(std::thread::current());
        }
        loop {
            let state = handle.state::<SidebarState>();
            if state.stopped.load(Ordering::Relaxed) {
                break;
            }
            queue_sync(&handle);
            // At most one pending tick. No WindowServer enumeration while inactive
            // or detached, and no network requests in the positioning loop.
            let tracking = state.tracking.load(Ordering::Relaxed);
            std::thread::park_timeout(Duration::from_millis(if tracking { 80 } else { 500 }));
        }
    });
}

pub fn stop(app: &AppHandle) {
    app.state::<SidebarState>()
        .stopped
        .store(true, Ordering::Relaxed);
    wake_sync(app);
    WORKSPACE_OBSERVERS.with(|slot| {
        for observer in slot.borrow_mut().drain(..) {
            // This is the same main-thread observer registered in start().
            unsafe {
                NSWorkspace::sharedWorkspace()
                    .notificationCenter()
                    .removeObserver((&*observer).as_ref());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CGPoint, CGSize};

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    };
    const HOST: Rect = Rect {
        x: 120.0,
        y: 50.0,
        width: 1000.0,
        height: 750.0,
    };

    #[test]
    fn full_width_square_keeps_the_official_center_and_spacing() {
        let badge = dock_frame(HOST, SCREEN, false).unwrap();
        assert_eq!(
            badge,
            Rect {
                x: 120.0,
                y: 138.0,
                width: 52.0,
                height: 52.0
            }
        );
        assert_eq!(badge.x + badge.width / 2.0, HOST.x + 26.0);
        assert_eq!(badge.y + badge.height / 2.0 - (HOST.y + 70.0), 44.0);
        assert_eq!((badge.width, badge.height), (52.0, 52.0));
    }

    #[test]
    fn movement_and_resize_keep_badge_aligned_to_window_bottom() {
        let moved = Rect {
            x: 320.0,
            y: 120.0,
            width: 800.0,
            height: 500.0,
        };
        let badge = dock_frame(moved, SCREEN, false).unwrap();
        assert_eq!((badge.x, badge.y), (320.0, 208.0));
        assert_eq!(
            dock_frame(
                Rect {
                    height: 600.0,
                    ..moved
                },
                SCREEN,
                false
            ),
            Some(badge)
        );
    }

    #[test]
    fn expansion_and_collapse_preserve_anchor_without_covering_official_button() {
        let expanded = dock_frame(HOST, SCREEN, true).unwrap();
        let collapsed = dock_frame(HOST, SCREEN, false).unwrap();
        assert_eq!((expanded.x, expanded.y), (collapsed.x, collapsed.y));
        assert_eq!((expanded.width, expanded.height), (288.0, 288.0));
    }

    #[test]
    fn extended_halo_preserves_both_core_centers() {
        for expanded in [false, true] {
            let core = dock_frame(HOST, SCREEN, expanded).unwrap();
            let halo = halo_frame(core, expanded);
            assert_eq!(halo.x + halo.width / 2.0, core.x + core.width / 2.0);
            assert_eq!(halo.y + halo.height / 2.0, core.y + core.height / 2.0);
            assert!(halo.contains(core));
            assert_eq!(halo.width, if expanded { 508.0 } else { 132.0 });
        }
    }

    #[test]
    fn halo_may_extend_past_display_edge_without_shifting_the_badge() {
        let host = Rect { x: 0.0, ..HOST };
        let core = dock_frame(host, SCREEN, false).unwrap();
        let halo = halo_frame(core, false);
        assert_eq!(core.x + core.width / 2.0, 26.0);
        assert_eq!(halo.x, -40.0);
        assert_eq!(halo.x + halo.width / 2.0, 26.0);
        assert!(SCREEN.contains(core));
    }

    #[test]
    fn halo_tracks_the_clamped_expanded_core_on_secondary_displays() {
        let screen = Rect { x: -1440.0, y: 900.0, ..SCREEN };
        let host = Rect { x: -700.0, y: 1100.0, ..HOST };
        let core = dock_frame(host, screen, true).unwrap();
        let halo = halo_frame(core, true);
        assert!(screen.contains(core));
        assert_eq!((halo.x + 110.0, halo.y + 110.0), (core.x, core.y));
    }

    #[test]
    fn secondary_displays_use_primary_origin_even_above_or_left_of_primary() {
        let bounds = CGRect::new(CGPoint::new(-1200.0, -1000.0), CGSize::new(1000.0, 700.0));
        let host = from_window_server(bounds, 900.0);
        assert_eq!((host.x, host.y), (-1200.0, 1200.0));
        let secondary = Rect {
            x: -1440.0,
            y: 900.0,
            ..SCREEN
        };
        let badge = dock_frame(host, secondary, false).unwrap();
        assert_eq!((badge.x, badge.y), (-1200.0, 1288.0));
    }

    #[test]
    fn expanded_card_stays_on_display_and_offscreen_rail_hides() {
        let near_edge = Rect {
            x: 1000.0,
            y: 250.0,
            ..HOST
        };
        let card = dock_frame(near_edge, SCREEN, true).unwrap();
        assert!(SCREEN.contains(card));
        assert!(near_edge.contains(card));
        assert!(dock_frame(Rect { x: 1320.0, y: 650.0, ..HOST }, SCREEN, true).is_none());
        assert!(dock_frame(Rect { x: -100.0, ..HOST }, SCREEN, false).is_none());
    }

    #[test]
    fn observed_client_expands_from_the_badge_and_clips_the_exterior_paint() {
        let host = Rect { x: 537.0, y: 0.0, width: 933.0, height: 923.0 };
        let screen = Rect { width: 1470.0, height: 956.0, ..SCREEN };
        let collapsed = dock_frame(host, screen, false).unwrap();
        let expanded = dock_frame(host, screen, true).unwrap();
        assert_eq!((expanded.x, expanded.y), (collapsed.x, collapsed.y));
        assert_eq!((expanded.x, expanded.y), (537.0, 88.0));
        let clip = halo_viewport(expanded, host, screen, true);
        assert_eq!(clip, HaloViewport { padding: 110.0, left: 110.0, top: 0.0, right: 0.0, bottom: 22.0 });
    }

    #[test]
    fn expanded_halo_paint_stays_inside_client_and_display_without_moving_the_badge() {
        let secondary = Rect { x: -1440.0, y: 900.0, ..SCREEN };
        for (host, screen) in [
            (HOST, SCREEN),
            (Rect { x: 1000.0, y: 250.0, ..HOST }, SCREEN),
            (Rect { y: -70.0, ..HOST }, SCREEN),
            (Rect { x: -700.0, y: 1100.0, ..HOST }, secondary),
            (Rect { width: 480.0, height: 360.0, ..HOST }, SCREEN),
        ] {
            let core = dock_frame(host, screen, true).unwrap();
            let halo = halo_frame(core, true);
            let clip = halo_viewport(core, host, screen, true);
            let paint = Rect {
                x: halo.x + clip.left,
                y: halo.y + clip.bottom,
                width: halo.width - clip.left - clip.right,
                height: halo.height - clip.top - clip.bottom,
            };
            assert!(paint.contains(core));
            assert!(host.contains(paint));
            assert!(screen.contains(paint));
        }
    }

    #[test]
    fn collapsed_halo_retains_its_existing_falloff_outside_the_client() {
        let core = dock_frame(HOST, SCREEN, false).unwrap();
        let clip = halo_viewport(core, HOST, SCREEN, false);
        assert_eq!(clip, HaloViewport { padding: 40.0, left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 });
    }

    #[test]
    fn shows_new_target_only_after_its_bounds_have_settled() {
        let now = Instant::now();
        let host = HostWindow { id: 1, frame: HOST };
        let mut motion = MotionGate::default();
        assert!(!motion.accepts(host, false, now));
        assert!(!motion.accepts(host, false, now + Duration::from_millis(349)));
        assert!(motion.accepts(host, false, now + SETTLE_TIME));
    }

    #[test]
    fn spaces_swipe_and_cancel_never_follow_intermediate_horizontal_frames() {
        let now = Instant::now();
        let host = HostWindow { id: 1, frame: HOST };
        let mut motion = MotionGate::default();
        assert!(!motion.accepts(host, false, now));
        assert!(motion.accepts(host, false, now + SETTLE_TIME));
        for (index, x) in [300.0, 700.0, 1100.0, 700.0, 300.0, HOST.x]
            .into_iter()
            .enumerate()
        {
            let intermediate = HostWindow {
                frame: Rect { x, ..HOST },
                ..host
            };
            assert!(!motion.accepts(
                intermediate,
                false,
                now + Duration::from_millis(400 + index as u64 * 80)
            ));
        }
        assert!(!motion.accepts(host, false, now + Duration::from_millis(1149)));
        assert!(motion.accepts(host, false, now + Duration::from_millis(1150)));
    }

    #[test]
    fn real_window_drag_still_tracks_without_waiting_for_settle() {
        let now = Instant::now();
        let host = HostWindow { id: 1, frame: HOST };
        let mut motion = MotionGate::default();
        motion.accepts(host, false, now);
        assert!(motion.accepts(host, false, now + SETTLE_TIME));
        let moved = HostWindow {
            frame: Rect {
                x: 320.0,
                y: 100.0,
                ..HOST
            },
            ..host
        };
        assert!(motion.accepts(moved, true, now + Duration::from_millis(400)));
    }

    #[test]
    fn rapid_space_changes_restart_settling_even_with_identical_window_bounds() {
        let now = Instant::now();
        let host = HostWindow { id: 1, frame: HOST };
        let mut motion = MotionGate::default();
        motion.accepts(host, false, now);
        assert!(motion.accepts(host, false, now + SETTLE_TIME));
        motion.space_changed(now + Duration::from_millis(400));
        assert!(!motion.accepts(host, false, now + Duration::from_millis(400)));
        motion.space_changed(now + Duration::from_millis(600));
        assert!(!motion.accepts(host, false, now + Duration::from_millis(600)));
        assert!(!motion.accepts(host, false, now + Duration::from_millis(949)));
        assert!(motion.accepts(host, false, now + Duration::from_millis(950)));
    }

    #[test]
    fn returning_from_another_space_or_app_does_not_reuse_stale_position() {
        let now = Instant::now();
        let host = HostWindow { id: 1, frame: HOST };
        let mut motion = MotionGate::default();
        motion.accepts(host, false, now);
        assert!(motion.accepts(host, false, now + SETTLE_TIME));
        let another = HostWindow { id: 2, frame: Rect { x: 320.0, ..HOST } };
        assert!(!motion.accepts(another, true, now + Duration::from_millis(400)));
        motion.clear_target();
        assert!(!motion.accepts(host, false, now + Duration::from_millis(500)));
        assert!(motion.accepts(host, false, now + Duration::from_millis(850)));
    }

    #[test]
    fn clicking_widget_retains_its_existing_codex_host() {
        let codex = target_scope(true, false, 100, None).unwrap();
        assert!(codex.matches(Some(100), Some(7)));
        let clicked_widget = target_scope(false, true, 200, Some(7)).unwrap();
        assert!(clicked_widget.matches(Some(100), Some(7)));
        assert!(!clicked_widget.matches(Some(100), Some(8)));
        assert!(!clicked_widget.matches(Some(200), Some(9)));
    }

    #[test]
    fn unrelated_apps_and_widget_without_a_host_do_not_attach() {
        assert_eq!(target_scope(false, false, 300, Some(7)), None);
        assert_eq!(target_scope(false, true, 200, None), None);
    }

    #[test]
    fn repeated_chat_switches_at_the_same_rail_do_not_hide_for_motion_or_update_probes() {
        let now = Instant::now();
        let mut motion = MotionGate::default();
        let updates = UpdatePriority::default();
        let initial = HostWindow { id: 1, frame: HOST };
        assert!(!motion.accepts(initial, false, now));
        assert!(motion.accepts(initial, false, now + SETTLE_TIME));
        for index in 0..100 {
            let host = HostWindow {
                id: index + 2,
                frame: Rect {
                    width: HOST.width + f64::from(index % 2) * 40.0,
                    height: HOST.height + f64::from(index % 3) * 10.0,
                    ..HOST
                },
            };
            assert!(motion.accepts(host, false, now + SETTLE_TIME + Duration::from_millis(u64::from(index) * 80)));
            let target = UpdateTarget {
                pid: 100,
                window_id: host.id,
                x: (host.frame.x + RAIL_CENTER_X) as f32,
                y: (SCREEN.height - host.frame.y - BADGE_CENTER_FROM_BOTTOM) as f32,
            };
            assert!(!updates.should_yield(target));
            updates.record(target, update_priority::Observation::Unavailable);
            assert!(!updates.should_yield(target));
        }
    }

    #[test]
    fn settling_retains_the_host_for_widget_activation_but_other_apps_clear_it() {
        let host = HostWindow { id: 7, frame: HOST };
        let mut layout = Layout { visible: true, target_id: Some(host.id), ..Layout::default() };
        layout.hide(Some(host), "reason=host_window_settling");
        let scope = target_scope(false, true, 200, layout.target_id).unwrap();
        assert!(scope.matches(Some(100), Some(host.id.into())));
        assert!(!layout.visible);
        assert_eq!(target_scope(false, false, 300, layout.target_id), None);
        layout.hide(None, "reason=foreground_app_inactive");
        assert_eq!(layout.target_id, None);
        assert_eq!(target_scope(false, true, 200, layout.target_id), None);
    }

    #[test]
    fn project_popovers_do_not_hide_the_main_window_or_move_its_badge() {
        let main = HostWindow { id: 7, frame: HOST };
        let popover = HostWindow {
            id: 8,
            frame: Rect { x: 400.0, y: 200.0, width: 340.0, height: 160.0 },
        };
        let selected = select_host_window([(popover, 100), (main, 100)]).unwrap();
        assert_eq!(selected, (main, 100));
        assert_eq!(
            dock_frame(selected.0.frame, SCREEN, false),
            dock_frame(main.frame, SCREEN, false),
        );
    }

    #[test]
    fn transient_windows_preserve_front_to_back_main_window_selection() {
        let background = HostWindow { id: 7, frame: HOST };
        let foreground = HostWindow {
            id: 9,
            frame: Rect { x: 300.0, ..HOST },
        };
        let short_panel = HostWindow {
            id: 10,
            frame: Rect { width: 600.0, height: 200.0, ..HOST },
        };
        let narrow_panel = HostWindow {
            id: 11,
            frame: Rect { width: 200.0, height: 600.0, ..HOST },
        };
        assert_eq!(
            select_host_window([
                (short_panel, 100),
                (narrow_panel, 100),
                (foreground, 100),
                (background, 100),
            ]),
            Some((foreground, 100)),
        );
        assert_eq!(
            select_host_window([(short_panel, 100), (narrow_panel, 100)]),
            None,
        );
        assert_eq!(select_host_window([]), None);
    }

    #[test]
    fn a_large_browser_surface_inside_codex_does_not_receive_the_badge() {
        let main = HostWindow { id: 7, frame: HOST };
        let browser = HostWindow {
            id: 8,
            frame: Rect { x: 400.0, y: 200.0, width: 600.0, height: 400.0 },
        };
        let chosen = select_host_window([(browser, 100), (main, 100)]).unwrap();
        assert_eq!(chosen, (main, 100));
        assert_eq!(dock_frame(chosen.0.frame, SCREEN, false), dock_frame(HOST, SCREEN, false));
    }

    #[test]
    fn nested_browser_surfaces_do_not_override_their_outer_main_window() {
        let main = HostWindow { id: 7, frame: HOST };
        let browser = HostWindow {
            id: 8,
            frame: Rect { x: 200.0, y: 100.0, width: 800.0, height: 600.0 },
        };
        let inner = HostWindow {
            id: 9,
            frame: Rect { x: 300.0, y: 200.0, width: 500.0, height: 400.0 },
        };
        assert_eq!(select_host_window([(inner, 100), (browser, 100), (main, 100)]), Some((main, 100)));
        let replacement = HostWindow { id: 10, ..main };
        assert_eq!(select_host_window([(replacement, 100), (main, 100)]), Some((replacement, 100)));
    }

    #[test]
    fn browser_protruding_past_the_main_window_keeps_the_badge_on_the_main_rail() {
        let main = HostWindow {
            id: 1,
            frame: Rect { x: 537.0, y: 0.0, width: 933.0, height: 923.0 },
        };
        let browser = HostWindow {
            id: 2,
            frame: Rect { x: 948.0, y: 529.0, width: 560.0, height: 392.0 },
        };
        // A real browser surface extended 38 points beyond its host's right
        // edge. Strict containment accepted it as a new host and moved Glint.
        assert!(!main.frame.contains(browser.frame));
        assert!(main.frame.contains_tool_surface(browser.frame));
        assert_eq!(select_host_window([(browser, 100), (main, 100)]), Some((main, 100)));
    }

    #[test]
    fn similar_sized_or_separate_windows_preserve_the_frontmost_main_window() {
        let background = HostWindow { id: 1, frame: HOST };
        let similar = HostWindow {
            id: 2,
            frame: Rect { x: HOST.x + 50.0, width: HOST.width - 10.0, ..HOST },
        };
        assert!(!background.frame.contains_tool_surface(similar.frame));
        let separate = HostWindow {
            id: 3,
            frame: Rect { x: HOST.x + HOST.width - 50.0, width: 500.0, height: 400.0, ..HOST },
        };
        assert!(!background.frame.contains_tool_surface(separate.frame));
        assert_eq!(select_host_window([(similar, 100), (background, 100)]), Some((similar, 100)));
        assert_eq!(select_host_window([(separate, 100), (background, 100)]), Some((separate, 100)));
    }
}
