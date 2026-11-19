//! Yield the sidebar slot to the app's update control. Without Accessibility,
//! check the updater's staged bundle metadata instead of reading the screen.

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::{CFBundle, CFRetained, CFString, CFType, CFURL};
use std::{
    fs,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Mutex,
    time::{Duration, Instant},
};

const RESTORE_DELAY: Duration = Duration::from_millis(350);
const CODEX_BUNDLE_ID: &str = "com.openai.codex";
const SPARKLE_INSTALLATION: &str = "org.sparkle-project.Sparkle/Installation";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> *const CFType;
    fn AXUIElementCopyElementAtPosition(
        application: *const CFType,
        x: f32,
        y: f32,
        element: *mut *const CFType,
    ) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: *const CFType,
        attribute: *const CFString,
        value: *mut *const CFType,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: *const CFType, seconds: f32) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct UpdateTarget {
    pub pid: i32,
    pub window_id: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Observation {
    Visible,
    Clear,
    Unavailable,
}

impl Observation {
    pub fn log_value(self) -> &'static str {
        match self {
            Self::Visible => "update_control_visible",
            Self::Clear => "update_control_absent",
            Self::Unavailable => "update_control_unknown",
        }
    }
}

#[derive(Default)]
struct Cache {
    target: Option<UpdateTarget>,
    observation: Option<Observation>,
    confirmed: Option<Observation>,
    clear_since: Option<Instant>,
}

impl Cache {
    fn should_yield(&mut self, target: UpdateTarget, now: Instant) -> bool {
        if self.target != Some(target) {
            *self = Self {
                target: Some(target),
                ..Self::default()
            };
        }
        match self.confirmed {
            Some(Observation::Visible) => true,
            Some(Observation::Clear) => self
                .clear_since
                .is_some_and(|since| now.duration_since(since) < RESTORE_DELAY),
            // A fresh or unavailable probe is not evidence of an update.
            // Keep the badge visible until a control is actually detected.
            _ => false,
        }
    }

    fn record(&mut self, target: UpdateTarget, observation: Observation, now: Instant) -> bool {
        // A reply from another window or a pre-Space-transition probe is stale.
        if self.target != Some(target) {
            return false;
        }
        let changed = self.observation != Some(observation);
        if observation == Observation::Clear {
            if self.confirmed == Some(Observation::Visible) {
                self.clear_since = Some(now);
            }
            self.confirmed = Some(Observation::Clear);
        } else if observation == Observation::Visible {
            self.clear_since = None;
            self.confirmed = Some(Observation::Visible);
        }
        self.observation = Some(observation);
        changed
    }
}

#[derive(Default)]
pub(crate) struct UpdatePriority(Mutex<Cache>);

impl UpdatePriority {
    pub fn clear(&self) {
        if let Ok(mut cache) = self.0.lock() {
            *cache = Cache::default();
        }
    }

    pub fn should_yield(&self, target: UpdateTarget) -> bool {
        self.0
            .lock()
            .map(|mut cache| cache.should_yield(target, Instant::now()))
            .unwrap_or(false)
    }

    pub fn requested_target(&self) -> Option<UpdateTarget> {
        self.0.lock().ok()?.target
    }

    pub fn record(&self, target: UpdateTarget, observation: Observation) -> bool {
        self.0
            .lock()
            .map(|mut cache| cache.record(target, observation, Instant::now()))
            .unwrap_or(false)
    }
}

pub(crate) fn is_trusted() -> bool {
    // Optional update-control detection never prompts at startup.
    unsafe { AXIsProcessTrusted() != 0 }
}

fn codex_build(app: &Path) -> Option<u64> {
    let url = CFURL::from_directory_path(app)?;
    let info = CFBundle::info_dictionary_in_directory(Some(&url))?;
    let info = unsafe { info.cast_unchecked::<CFString, CFType>() };
    let identifier = info.get(&CFString::from_str("CFBundleIdentifier"))?;
    if identifier.downcast_ref::<CFString>()?.to_string() != CODEX_BUNDLE_ID {
        return None;
    }
    let build = info.get(&CFString::from_str("CFBundleVersion"))?;
    build.downcast_ref::<CFString>()?.to_string().parse().ok()
}

fn directories(entries: fs::ReadDir) -> impl Iterator<Item = PathBuf> {
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
}

fn probe_staged_update(
    installation: &Path,
    running_build: impl FnOnce() -> Option<u64>,
) -> Observation {
    let stages = match fs::read_dir(installation) {
        Ok(stages) => stages,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Observation::Clear,
        Err(_) => return Observation::Unavailable,
    };
    let staged_build = directories(stages)
        .flat_map(|stage| fs::read_dir(stage).into_iter().flat_map(directories))
        .flat_map(|payload| fs::read_dir(payload).into_iter().flat_map(directories))
        .filter(|app| app.extension().is_some_and(|extension| extension == "app"))
        .filter_map(|app| codex_build(&app))
        .max();
    let Some(staged_build) = staged_build else {
        return Observation::Clear;
    };
    match running_build() {
        Some(build) if staged_build > build => Observation::Visible,
        Some(_) => Observation::Clear,
        None => Observation::Unavailable,
    }
}

pub(crate) fn probe(target: UpdateTarget, trusted: bool) -> Observation {
    if trusted {
        return probe_accessibility(target);
    }
    let Some(cache) = dirs::cache_dir() else {
        return Observation::Unavailable;
    };
    probe_staged_update(
        &cache.join(CODEX_BUNDLE_ID).join(SPARKLE_INSTALLATION),
        || {
            autoreleasepool(|_| {
                let app =
                    NSRunningApplication::runningApplicationWithProcessIdentifier(target.pid)?;
                let path = app.bundleURL()?.to_file_path()?;
                codex_build(&path)
            })
        },
    )
}

fn retained(pointer: *const CFType) -> Option<CFRetained<CFType>> {
    // AX Create/Copy functions return owned CoreFoundation objects.
    NonNull::new(pointer.cast_mut()).map(|pointer| unsafe { CFRetained::from_raw(pointer) })
}

fn attribute(element: &CFType, name: &str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_str(name);
    let mut value = std::ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element, &*name, &mut value) };
    (status == 0).then(|| retained(value)).flatten()
}

fn string_attribute(element: &CFType, name: &str) -> Option<String> {
    Some(
        attribute(element, name)?
            .downcast_ref::<CFString>()?
            .to_string(),
    )
}

fn is_button(role: &str) -> bool {
    matches!(role, "AXButton" | "AXPopUpButton" | "AXMenuButton")
}

fn is_update_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    ["update", "restart", "relaunch"]
        .iter()
        .any(|word| lower.contains(word))
        || ["更新", "升级", "升級", "重启", "重啟", "重新启动"]
            .iter()
            .any(|word| label.contains(word))
}

fn is_update_progress(role: &str) -> bool {
    // The official navigation-rail update action uses role=progressbar during
    // downloading/installing. These controls have no button ancestor.
    matches!(role, "AXProgressIndicator" | "AXBusyIndicator")
}

/// Runs off the AppKit thread. Hit testing is restricted to the Codex process,
/// so our overlaid window does not hide the original control from this query.
fn probe_accessibility(target: UpdateTarget) -> Observation {
    let Some(application) = retained(unsafe { AXUIElementCreateApplication(target.pid) }) else {
        return Observation::Unavailable;
    };
    unsafe {
        AXUIElementSetMessagingTimeout(&*application, 0.08);
    }
    let mut hit = std::ptr::null();
    if unsafe { AXUIElementCopyElementAtPosition(&*application, target.x, target.y, &mut hit) } != 0
    {
        return Observation::Unavailable;
    }
    let Some(mut element) = retained(hit) else {
        return Observation::Unavailable;
    };
    for _ in 0..16 {
        unsafe {
            AXUIElementSetMessagingTimeout(&*element, 0.08);
        }
        let Some(role) = string_attribute(&element, "AXRole") else {
            return Observation::Unavailable;
        };
        if is_update_progress(&role) {
            return Observation::Visible;
        }
        if is_button(&role) {
            // Match only the app update/restart button at this precise point.
            // Never request AXValue, window titles, children, or document text.
            return if ["AXTitle", "AXDescription", "AXHelp", "AXIdentifier"]
                .iter()
                .filter_map(|name| string_attribute(&element, name))
                .any(|label| is_update_label(&label))
            {
                Observation::Visible
            } else {
                Observation::Clear
            };
        }
        if matches!(role.as_str(), "AXWindow" | "AXApplication") {
            return Observation::Clear;
        }
        let Some(parent) = attribute(&element, "AXParent") else {
            return Observation::Unavailable;
        };
        element = parent;
    }
    Observation::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TARGET: UpdateTarget = UpdateTarget {
        pid: 100,
        window_id: 7,
        x: 146.0,
        y: 736.0,
    };

    fn fixture_bundle(app: &Path, identifier: &str, build: &str) {
        let contents = app.join("Contents");
        fs::create_dir_all(&contents).unwrap();
        fs::write(
            contents.join("Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundleVersion</key><string>{build}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_newer_staged_codex_build_hides_quota_until_installation_clears() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "glint-staged-update-{}-{nonce}",
            std::process::id()
        ));
        let installation = root.join("Installation");
        let payload = installation.join("stage/payload");
        assert_eq!(
            probe_staged_update(&installation, || None),
            Observation::Clear
        );
        fixture_bundle(&payload.join("Other.app"), "com.example.other", "999");
        fixture_bundle(&payload.join("Older.app"), CODEX_BUNDLE_ID, "99");
        assert_eq!(
            probe_staged_update(&installation, || Some(100)),
            Observation::Clear
        );

        let update = payload.join("ChatGPT.app");
        fixture_bundle(&update, CODEX_BUNDLE_ID, "101");
        let now = Instant::now();
        let mut cache = Cache::default();
        assert!(!cache.should_yield(TARGET, now));
        cache.record(
            TARGET,
            probe_staged_update(&installation, || Some(100)),
            now,
        );
        assert!(cache.should_yield(TARGET, now));
        assert_eq!(
            probe_staged_update(&installation, || Some(101)),
            Observation::Clear
        );
        assert_eq!(
            probe_staged_update(&installation, || None),
            Observation::Unavailable
        );

        fs::remove_dir_all(&update).unwrap();
        cache.record(
            TARGET,
            probe_staged_update(&installation, || Some(100)),
            now,
        );
        assert!(cache.should_yield(TARGET, now));
        assert!(!cache.should_yield(TARGET, now + RESTORE_DELAY));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn update_presence_hides_immediately_and_absence_must_settle() {
        let now = Instant::now();
        let mut cache = Cache::default();
        assert!(!cache.should_yield(TARGET, now));
        cache.record(TARGET, Observation::Visible, now);
        assert!(cache.should_yield(TARGET, now));
        cache.record(TARGET, Observation::Clear, now);
        assert!(cache.should_yield(TARGET, now + Duration::from_millis(349)));
        assert!(!cache.should_yield(TARGET, now + RESTORE_DELAY));
        cache.record(TARGET, Observation::Visible, now + RESTORE_DELAY);
        assert!(cache.should_yield(TARGET, now + RESTORE_DELAY));
    }

    #[test]
    fn unknown_detection_never_hides_normal_quota_even_during_the_first_probe() {
        let now = Instant::now();
        let mut cache = Cache::default();
        assert!(!cache.should_yield(TARGET, now));
        cache.record(TARGET, Observation::Unavailable, now);
        assert!(!cache.should_yield(TARGET, now));
        cache.record(TARGET, Observation::Clear, now);
        assert!(!cache.should_yield(TARGET, now + RESTORE_DELAY));
        cache.record(
            TARGET,
            Observation::Unavailable,
            now + Duration::from_secs(1),
        );
        assert!(!cache.should_yield(TARGET, now + Duration::from_secs(30)));
        cache.record(TARGET, Observation::Visible, now + Duration::from_secs(31));
        cache.record(
            TARGET,
            Observation::Unavailable,
            now + Duration::from_secs(32),
        );
        assert!(cache.should_yield(TARGET, now + Duration::from_secs(32)));
        cache.record(TARGET, Observation::Clear, now + Duration::from_secs(33));
        assert!(!cache.should_yield(TARGET, now + Duration::from_secs(34)));
    }

    #[test]
    fn switching_windows_or_spaces_discards_previous_replies() {
        let now = Instant::now();
        let mut cache = Cache::default();
        cache.should_yield(TARGET, now);
        cache.record(TARGET, Observation::Visible, now);
        assert!(cache.should_yield(TARGET, now));
        let next = UpdateTarget {
            window_id: 8,
            ..TARGET
        };
        assert!(!cache.should_yield(next, now + RESTORE_DELAY));
        assert!(!cache.record(TARGET, Observation::Visible, now + RESTORE_DELAY));
        assert!(!cache.should_yield(next, now + RESTORE_DELAY));
        cache = Cache::default();
        assert!(!cache.record(next, Observation::Clear, now + RESTORE_DELAY));
    }

    #[test]
    fn an_initial_clear_probe_does_not_start_an_update_restore_delay() {
        let now = Instant::now();
        let mut cache = Cache::default();
        assert!(!cache.should_yield(TARGET, now));
        cache.record(TARGET, Observation::Clear, now);
        assert!(!cache.should_yield(TARGET, now));
        cache.record(TARGET, Observation::Visible, now + Duration::from_secs(1));
        assert!(cache.should_yield(TARGET, now + Duration::from_secs(1)));
        cache.record(TARGET, Observation::Clear, now + Duration::from_secs(2));
        assert!(cache.should_yield(TARGET, now + Duration::from_secs(2)));
        cache.record(
            TARGET,
            Observation::Clear,
            now + Duration::from_millis(2200),
        );
        assert!(!cache.should_yield(TARGET, now + Duration::from_secs(2) + RESTORE_DELAY));
    }

    #[test]
    fn only_app_updates_and_restart_actions_match_the_button_label() {
        for name in [
            "Update available",
            "Update",
            "Restart to update",
            "更新可用",
            "下载更新",
            "更新並重啟",
        ] {
            assert!(is_update_label(name));
        }
        for name in [
            "Downloads",
            "Download complete",
            "下载",
            "下載完成",
            "Usage limits",
            "额度",
            "账户",
            "",
        ] {
            assert!(!is_update_label(name));
        }
        assert!(is_button("AXButton"));
        assert!(is_button("AXPopUpButton"));
        assert!(!is_button("AXStaticText"));
    }

    #[test]
    fn update_download_and_install_progress_have_priority_without_a_button() {
        assert!(is_update_progress("AXProgressIndicator"));
        assert!(is_update_progress("AXBusyIndicator"));
        assert!(!is_update_progress("AXButton"));
        assert!(!is_update_progress("AXStaticText"));
    }
}
