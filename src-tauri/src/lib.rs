#[cfg(not(target_os = "macos"))]
compile_error!("This edition of Glint supports macOS only.");
mod companion;
pub use companion::prepare_launch;

mod codex;
mod models;
mod reset_credit;
#[cfg(target_os = "macos")]
mod update_priority;
#[cfg(target_os = "macos")]
mod sidebar;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    panic,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::{Local, SecondsFormat};

use models::ProviderSnapshot;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

struct AppState {
    client: reqwest::Client,
    data_dir: PathBuf,
    lifecycle_log_path: PathBuf,
    lifecycle_marker_path: PathBuf,
    fetch_lock: tokio::sync::Mutex<()>,
    reset_lock: tokio::sync::Mutex<()>,
    snapshot_cache: Mutex<Option<(Instant, Vec<ProviderSnapshot>)>>,
}

fn lifecycle_timestamp() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Millis, false)
}

fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn append_lifecycle_log(path: &Path, event: &str, details: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(
        file,
        "{} pid={} event={} {}",
        lifecycle_timestamp(),
        std::process::id(),
        single_line(event),
        single_line(details)
    );
    let _ = file.flush();
}

fn initialize_lifecycle_log(log_path: &Path, marker_path: &Path) {
    if let Ok(previous_session) = fs::read_to_string(marker_path) {
        append_lifecycle_log(
            log_path,
            "previous_session_unclean",
            &format!("previous={}", previous_session.trim()),
        );
    }

    if let Some(parent) = marker_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let marker = format!(
        "pid={} started_at={}",
        std::process::id(),
        lifecycle_timestamp()
    );
    let _ = fs::write(marker_path, marker);
    append_lifecycle_log(
        log_path,
        "app_started",
        &format!("version={}", env!("CARGO_PKG_VERSION")),
    );
}

fn log_lifecycle(app: &AppHandle, event: &str, details: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        append_lifecycle_log(&state.lifecycle_log_path, event, details);
    }
}

fn finish_lifecycle_log(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        append_lifecycle_log(
            &state.lifecycle_log_path,
            "app_exited",
            "event_loop=finished",
        );
        let _ = fs::remove_file(&state.lifecycle_marker_path);
    }
}

#[tauri::command]
async fn consume_reset_credit(window: tauri::WebviewWindow, state: State<'_, AppState>, idempotency_key: String) -> Result<reset_credit::Redemption, String> {
    if window.label() != "widget" { return Err("notAllowed".into()); }
    let _reset = state.reset_lock.try_lock().map_err(|_| "alreadyInProgress")?;
    let _fetch = state.fetch_lock.lock().await;
    let path = state.data_dir.join("reset-attempt.json");
    let result = reset_credit::consume(&idempotency_key, &path).await;
    if let Ok(mut cache) = state.snapshot_cache.lock() { *cache = None; }
    result
}

async fn fetch_snapshots_uncached(state: &State<'_, AppState>) -> Vec<ProviderSnapshot> {
    let _guard = state.fetch_lock.lock().await;
    let values = vec![codex::fetch_snapshot(&state.client).await];
    if let Ok(mut cache) = state.snapshot_cache.lock() {
        *cache = Some((Instant::now(), values.clone()));
    }
    values
}

#[tauri::command]
async fn get_snapshots(state: State<'_, AppState>) -> Result<Vec<ProviderSnapshot>, String> {
    const CACHE_TTL: Duration = Duration::from_secs(30);
    if let Ok(cache) = state.snapshot_cache.lock() {
        if let Some((time, values)) = &*cache {
            if time.elapsed() < CACHE_TTL {
                return Ok(values.clone());
            }
        }
    }
    let _guard = match state.fetch_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            if let Ok(cache) = state.snapshot_cache.lock() {
                if let Some((_, values)) = &*cache {
                    return Ok(values.clone());
                }
            }
            return Ok(vec![ProviderSnapshot::failure(
                "unavailable",
                "Quota refresh is already running.",
            )]);
        }
    };
    if let Ok(cache) = state.snapshot_cache.lock() {
        if let Some((time, values)) = &*cache {
            if time.elapsed() < CACHE_TTL {
                return Ok(values.clone());
            }
        }
    }
    let values = vec![codex::fetch_snapshot(&state.client).await];
    if let Ok(mut cache) = state.snapshot_cache.lock() {
        *cache = Some((Instant::now(), values.clone()));
    }
    Ok(values)
}

#[tauri::command]
async fn refresh_snapshots(state: State<'_, AppState>) -> Result<Vec<ProviderSnapshot>, String> {
    Ok(fetch_snapshots_uncached(&state).await)
}

#[tauri::command]
fn set_widget_expanded(expanded: bool, app: AppHandle) -> Result<(), String> {
    sidebar::set_expanded(&app, expanded)
}

pub fn run() {
    let app = tauri::Builder::default()
        .enable_macos_default_menu(false)
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            log_lifecycle(app, "secondary_launch_received", "action=sync_sidebar");
            let _ = sidebar::apply_mode(app);
        }))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            let data_dir = app.path().app_config_dir()?;
            let log_dir = app.path().app_log_dir()?;
            let lifecycle_log_path = log_dir.join("lifecycle.log");
            let lifecycle_marker_path = log_dir.join("lifecycle.active");
            initialize_lifecycle_log(&lifecycle_log_path, &lifecycle_marker_path);

            let panic_log_path = lifecycle_log_path.clone();
            let previous_panic_hook = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                let location = info
                    .location()
                    .map(|value| format!("{}:{}:{}", value.file(), value.line(), value.column()))
                    .unwrap_or_else(|| "unknown".to_string());
                append_lifecycle_log(&panic_log_path, "panic", &format!("location={location}"));
                previous_panic_hook(info);
            }));

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("QuotaFloat/0.1")
                .build()
                .expect("static HTTP client configuration must be valid");
            app.manage(AppState {
                client,
                data_dir,
                lifecycle_log_path,
                lifecycle_marker_path,
                fetch_lock: tokio::sync::Mutex::new(()),
                reset_lock: tokio::sync::Mutex::new(()),
                snapshot_cache: Mutex::new(None),
            });
            #[cfg(target_os = "macos")]
            {
                app.manage(sidebar::SidebarState::default());
                sidebar::create_halo(app.handle())?;
            }
            if let Some(window) = app.get_webview_window("widget") {
                let _ = window.set_size(tauri::LogicalSize::new(52.0, 52.0));
                let _ = window.set_focusable(false);
                let _ = window.set_always_on_top(true);
                #[cfg(target_os = "macos")]
                {
                    sidebar::apply_mode(app.handle())?;
                    sidebar::start(app.handle());
                }
                #[cfg(not(target_os = "macos"))]
                let _ = window.show();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshots,
            refresh_snapshots,
            consume_reset_credit,
            set_widget_expanded,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                log_lifecycle(
                    window.app_handle(),
                    "quit_requested",
                    &format!("source=window_close label={}", window.label()),
                );
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Glint");
    app.run(|app_handle, event| match event {
        tauri::RunEvent::Resumed => {
            let _ = app_handle.emit_to("widget", "refresh-requested", ());
        }
        tauri::RunEvent::ExitRequested { code, .. } => {
            log_lifecycle(app_handle, "exit_requested", &format!("code={code:?}"))
        }
        tauri::RunEvent::Exit => {
            #[cfg(target_os = "macos")]
            sidebar::stop(app_handle);
            finish_lifecycle_log(app_handle);
        }
        _ => {}
    });
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_log_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock must be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "quota-float-lifecycle-test-{}-{nonce}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn lifecycle_log_appends_single_line_events() {
        let directory = temporary_log_directory();
        let log_path = directory.join("lifecycle.log");

        append_lifecycle_log(&log_path, "quit_requested", "source=window_close\nignored");

        let contents = fs::read_to_string(&log_path).expect("lifecycle log should exist");
        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("event=quit_requested source=window_close ignored"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn initialization_reports_an_unclean_previous_session() {
        let directory = temporary_log_directory();
        let log_path = directory.join("lifecycle.log");
        let marker_path = directory.join("lifecycle.active");
        fs::create_dir_all(&directory).expect("test directory should be created");
        fs::write(&marker_path, "pid=123 started_at=earlier").expect("marker should be written");

        initialize_lifecycle_log(&log_path, &marker_path);

        let contents = fs::read_to_string(&log_path).expect("lifecycle log should exist");
        assert!(
            contents.contains("event=previous_session_unclean previous=pid=123 started_at=earlier")
        );
        assert!(contents.contains("event=app_started version="));
        assert!(marker_path.exists());
        let _ = fs::remove_dir_all(directory);
    }
}
