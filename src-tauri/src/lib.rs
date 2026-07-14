mod codex;

use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::Mutex;

const FETCH_INTERVAL_SECS: u64 = 300;
const WORKER_TICK_SECS: u64 = 10;

struct AppState {
    last_fetch_time: SystemTime,
}

fn setup_window_position(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

async fn fetch_and_record(state: &Arc<Mutex<AppState>>) -> Result<codex::CodexLimitData, String> {
    let data = codex::fetch().await?;
    state.lock().await.last_fetch_time = SystemTime::now();
    Ok(data)
}

#[tauri::command]
async fn fetch_codex_limit_data(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<codex::CodexLimitData, String> {
    fetch_and_record(&state).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![fetch_codex_limit_data])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            setup_window_position(&handle);

            let state = Arc::new(Mutex::new(AppState {
                last_fetch_time: SystemTime::now(),
            }));
            app.manage(state.clone());

            let worker_handle = handle.clone();
            let worker_state = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(WORKER_TICK_SECS));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;
                    let elapsed = SystemTime::now()
                        .duration_since(worker_state.lock().await.last_fetch_time)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(0);
                    if elapsed < FETCH_INTERVAL_SECS {
                        continue;
                    }

                    match fetch_and_record(&worker_state).await {
                        Ok(data) => {
                            let _ = worker_handle.emit("limits-updated", data);
                        }
                        Err(error) => {
                            let _ = worker_handle.emit("limits-error", error);
                        }
                    }
                }
            });

            use tauri::menu::Menu;
            use tauri::menu::MenuItem;
            use tauri::tray::MouseButton;
            use tauri::tray::MouseButtonState;
            use tauri::tray::TrayIconBuilder;
            use tauri::tray::TrayIconEvent;

            let refresh = MenuItem::with_id(app, "refresh", "更新", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&refresh, &quit])?;

            TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/tray-icon.png"
                ))?)
                .icon_as_template(true)
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    if event.id == "quit" {
                        app.exit(0);
                    } else if event.id == "refresh" {
                        let _ = app.emit("force-refresh", ());
                    }
                })
                .on_tray_icon_event(move |tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Codex Limit Monitor");
}
