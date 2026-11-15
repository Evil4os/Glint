//! A small native watcher follows Codex without keeping either WebView alive.
//! It runs only from an installed app and never asks for Accessibility access.
use objc2::{rc::autoreleasepool, MainThreadMarker};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSDate, NSRunLoop};
use std::{fs, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};

const LABEL: &str = "app.quotafloat.desktop.watch";
const HOST: &str = "com.openai.codex";

#[derive(Debug, PartialEq)]
enum Action { Start, Stop, Wait }

fn action(host_running: bool, widget_running: bool, cooling_down: bool) -> Action {
    match (host_running, widget_running, cooling_down) {
        (true, false, false) => Action::Start,
        (false, true, _) => Action::Stop,
        _ => Action::Wait,
    }
}

fn installed_bundle(executable: &Path, home: &Path) -> Option<PathBuf> {
    let bundle = executable.parent()?.parent()?.parent()?;
    if executable != bundle.join("Contents/MacOS/glint") || bundle.extension()? != "app" {
        return None;
    }
    let parent = bundle.parent()?;
    (parent == Path::new("/Applications") || parent == home.join("Applications"))
        .then(|| bundle.to_path_buf())
}

fn xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&apos;")
}

fn agent_plist(executable: &Path) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>--watch</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>10</integer>
<key>ProcessType</key><string>Background</string>
</dict></plist>
"#, xml(&executable.to_string_lossy()))
}

fn domain() -> Result<String, String> {
    let result = Command::new("/usr/bin/id").arg("-u").output().map_err(|e| e.to_string())?;
    let uid = String::from_utf8_lossy(&result.stdout).trim().to_owned();
    if !result.status.success() || uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Cannot resolve user launchd domain".into());
    }
    Ok(format!("gui/{uid}"))
}

fn register(executable: &Path, home: &Path) -> Result<(), String> {
    let path = home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"));
    let content = agent_plist(executable);
    let domain = domain()?;
    let service = format!("{domain}/{LABEL}");
    if fs::read_to_string(&path).ok().as_deref() == Some(&content) {
        // Don't keep overwriting a job or override a user's OS-level disable.
        if Command::new("/bin/launchctl").args(["print", &service]).output()
            .map_err(|e| e.to_string())?.status.success() { return Ok(()); }
    } else {
        let _ = Command::new("/bin/launchctl").args(["bootout", &service]).output();
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("plist.tmp");
        fs::write(&temporary, content).map_err(|e| e.to_string())?;
        fs::rename(temporary, &path).map_err(|e| e.to_string())?;
    }
    let result = Command::new("/bin/launchctl").args(["bootstrap", &domain])
        .arg(&path).output().map_err(|e| e.to_string())?;
    if !result.status.success() { return Err("Could not register Codex companion watcher".into()); }
    Ok(())
}

fn unregister(home: &Path) -> Result<(), String> {
    let service = format!("{}/{LABEL}", domain()?);
    let _ = Command::new("/bin/launchctl").args(["bootout", &service]).output();
    let path = home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"));
    if path.exists() { fs::remove_file(path).map_err(|e| e.to_string())?; }
    Ok(())
}

fn host_running() -> bool {
    NSWorkspace::sharedWorkspace().runningApplications().iter().any(|app|
        app.bundleIdentifier().is_some_and(|id| id.to_string() == HOST) && !app.isTerminated())
}

fn watch(executable: &Path, bundle: &Path) {
    let mut last_start: Option<Instant> = None;
    loop {
        let cycle = Instant::now();
        autoreleasepool(|_| {
            let workspace = NSWorkspace::sharedWorkspace();
            let apps = workspace.runningApplications();
            let widgets: Vec<_> = apps.iter().filter(|app| {
                app.processIdentifier() != std::process::id() as i32 && !app.isTerminated()
                    && app.executableURL().and_then(|url| url.path())
                        .is_some_and(|path| Path::new(&path.to_string()) == executable)
            }).collect();
            let host = apps.iter().any(|app|
                app.bundleIdentifier().is_some_and(|id| id.to_string() == HOST) && !app.isTerminated());
            match action(host, !widgets.is_empty(), last_start.is_some_and(|t| t.elapsed() < Duration::from_secs(15))) {
                Action::Start => {
                    last_start = Some(Instant::now());
                    let _ = Command::new("/usr/bin/open").args(["-g", "-j"]).arg(bundle).status();
                }
                Action::Stop => { for widget in widgets { widget.terminate(); } last_start = None; }
                Action::Wait => { if !host { last_start = None; } }
            }
            // Pump the native run loop so NSWorkspace keeps its process list fresh.
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(2.0));
        });
        // An empty run loop can return early: never spin at full CPU.
        if let Some(rest) = Duration::from_secs(2).checked_sub(cycle.elapsed()) { std::thread::sleep(rest); }
    }
}

/// Runs before Tauri: the watcher never initializes WebKit or reads account data.
pub fn prepare_launch() -> bool {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(home) = dirs::home_dir() else { return false; };
    if args == ["--uninstall-watcher"] {
        if let Err(error) = unregister(&home) { eprintln!("{error}"); }
        return false;
    }
    if args == ["--watch-status"] {
        let service = domain().map(|domain| format!("{domain}/{LABEL}"));
        let loaded = service.ok().is_some_and(|service| Command::new("/bin/launchctl")
            .args(["print", &service]).output().is_ok_and(|result| result.status.success()));
        let host = autoreleasepool(|_| host_running());
        println!("watcher_loaded={loaded} codex_running={host}");
        return false;
    }
    if args == ["--help"] {
        println!("Glint follows Codex. Install in Applications and open once.\n--uninstall-watcher  Stop following Codex; account data is untouched.");
        return false;
    }
    let Ok(executable) = std::env::current_exe() else { return false; };
    let bundle = installed_bundle(&executable, &home);
    if args == ["--watch"] {
        let Some(bundle) = bundle else { return false; };
        assert!(MainThreadMarker::new().is_some(), "Watcher requires the main thread");
        watch(&executable, &bundle);
        return false;
    }
    if !args.is_empty() { return false; }
    if bundle.is_some() {
        if let Err(error) = register(&executable, &home) { eprintln!("{error}"); }
        // First launch while Codex is closed registers the watcher and exits.
        return autoreleasepool(|_| host_running());
    }
    // Local builds may run for development, but never install a login agent.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_host_and_recovers_without_a_launch_storm() {
        assert_eq!(action(true, false, false), Action::Start);
        assert_eq!(action(true, false, true), Action::Wait);
        assert_eq!(action(true, true, false), Action::Wait);
        assert_eq!(action(false, true, false), Action::Stop);
        assert_eq!(action(false, false, false), Action::Wait);
    }
    #[test]
    fn only_installed_apps_can_register_a_watcher() {
        let home = Path::new("/Users/test");
        for path in ["/Applications/Glint.app/Contents/MacOS/glint", "/Users/test/Applications/Glint.app/Contents/MacOS/glint"] {
            assert!(installed_bundle(Path::new(path), home).is_some());
        }
        for path in ["/tmp/Glint.app/Contents/MacOS/glint", "/project/target/release/glint", "/Applications/Glint.app/Contents/MacOS/other"] {
            assert!(installed_bundle(Path::new(path), home).is_none());
        }
    }
    #[test]
    fn launch_agent_quotes_paths_and_uses_no_shell() {
        let plist = agent_plist(Path::new("/Applications/A & B.app/Contents/MacOS/glint"));
        assert!(plist.contains("A &amp; B.app"));
        assert!(plist.contains("<string>--watch</string>"));
        assert!(!plist.contains("/bin/sh"));
    }
}
