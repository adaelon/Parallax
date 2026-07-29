//! Thin Tauri host for S07.

mod state;

use std::{
    env,
    ffi::OsStr,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use eam_desktop_host::{ExitReason, LaunchMode};
use serde::Serialize;
use state::{HostStatusView, ManagedHost};
use tauri::{
    AppHandle, Manager, RunEvent, WindowEvent,
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tauri_plugin_updater::UpdaterExt;
use url::Url;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
static EXIT_ALLOWED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct UpdateChannel {
    endpoint: Option<Url>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAvailability {
    configured: bool,
    available: bool,
    version: Option<String>,
    notes: Option<String>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects command guards by value.
fn get_host_status(host: tauri::State<'_, ManagedHost>) -> HostStatusView {
    host.status()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects command handles by value.
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects command handles by value.
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|error| error.to_string())?;
    } else {
        manager.disable().map_err(|error| error.to_string())?;
    }
    manager.is_enabled().map_err(|error| error.to_string())
}

#[tauri::command]
async fn check_update(
    app: AppHandle,
    channel: tauri::State<'_, UpdateChannel>,
) -> Result<UpdateAvailability, String> {
    let Some(endpoint) = channel.endpoint.clone() else {
        return Ok(UpdateAvailability {
            configured: false,
            available: false,
            version: None,
            notes: None,
        });
    };
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater.check().await.map_err(|error| error.to_string())?;
    Ok(match update {
        Some(update) => UpdateAvailability {
            configured: true,
            available: true,
            version: Some(update.version),
            notes: update.body,
        },
        None => UpdateAvailability {
            configured: true,
            available: false,
            version: None,
            notes: None,
        },
    })
}

#[tauri::command]
async fn install_update(
    app: AppHandle,
    channel: tauri::State<'_, UpdateChannel>,
    host: tauri::State<'_, ManagedHost>,
) -> Result<(), String> {
    let endpoint = channel
        .endpoint
        .clone()
        .ok_or_else(|| "signed updater is not configured".to_owned())?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no update is available".to_owned())?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;

    if let Err(errors) = host.shutdown(ExitReason::Update) {
        let reopen = host.reopen_after_update_failure();
        return Err(format_update_failure(
            "secure shutdown failed",
            &errors,
            reopen,
        ));
    }
    if let Err(error) = update.install(&bytes) {
        let install_error = error.to_string();
        let reopen = host.reopen_after_update_failure();
        return Err(format_update_failure(
            &format!("update installation failed: {install_error}"),
            &[],
            reopen,
        ));
    }

    EXIT_ALLOWED.store(true, Ordering::Release);
    app.restart();
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects command handles by value.
fn exit_application(app: AppHandle) {
    shutdown_and_exit(&app);
}

#[must_use]
pub fn builder() -> tauri::Builder<tauri::Wry> {
    let update_configuration = update_configuration();
    let updater_configured = update_configuration.is_some();
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            activate_existing_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ));
    if let Some((_, public_key)) = &update_configuration {
        builder = builder.plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(public_key)
                .build(),
        );
    }
    let update_endpoint = update_configuration.map(|(endpoint, _)| endpoint);

    builder
        .manage(UpdateChannel {
            endpoint: update_endpoint,
        })
        .setup(move |app| {
            let launch_mode = launch_mode();
            let vault_root = vault_root(app.handle())?;
            app.manage(ManagedHost::open(
                vault_root,
                launch_mode,
                updater_configured,
            ));
            install_tray(app)?;
            if launch_mode == LaunchMode::Background
                && let Some(window) = app.get_webview_window("main")
            {
                window.hide()?;
            }
            spawn_heartbeat(app.handle().clone())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                if let Some(host) = window.app_handle().try_state::<ManagedHost>() {
                    let _ = host.mark_hidden();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_host_status,
            get_autostart,
            set_autostart,
            check_update,
            install_update,
            exit_application,
        ])
}

/// Runs the single-instance desktop event loop.
///
/// # Panics
///
/// Panics when the generated Tauri context or native runtime cannot be built.
pub fn run() {
    EXIT_ALLOWED.store(false, Ordering::Release);
    let app = builder()
        .build(tauri::generate_context!())
        .expect("failed to build the S07 Tauri host");
    app.run(|_, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !EXIT_ALLOWED.load(Ordering::Acquire)
        {
            api.prevent_exit();
        }
    });
}

fn update_configuration() -> Option<(Url, String)> {
    update_configuration_from(
        env::var("EAM_UPDATE_ENDPOINT").ok(),
        env::var("EAM_UPDATE_PUBKEY").ok(),
    )
}

fn update_configuration_from(
    endpoint: Option<String>,
    public_key: Option<String>,
) -> Option<(Url, String)> {
    let endpoint = Url::parse(&endpoint?).ok()?;
    let public_key = public_key.filter(|value| !value.trim().is_empty())?;
    (endpoint.scheme() == "https").then_some((endpoint, public_key))
}

fn launch_mode() -> LaunchMode {
    if env::args_os().any(|argument| argument == OsStr::new("--background")) {
        LaunchMode::Background
    } else {
        LaunchMode::Foreground
    }
}

fn vault_root(app: &AppHandle) -> tauri::Result<PathBuf> {
    env::var_os("EAM_VAULT_ROOT").map_or_else(
        || {
            app.path()
                .app_local_data_dir()
                .map(|path| path.join("vault"))
        },
        |path| Ok(PathBuf::from(path)),
    )
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &exit])?;
    TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => activate_existing_window(app),
            "exit" => shutdown_and_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                activate_existing_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn tray_icon() -> Image<'static> {
    let mut rgba = vec![0_u8; 16 * 16 * 4];
    for y in 0_usize..16 {
        for x in 0_usize..16 {
            let index = (y * 16 + x) * 4;
            let inside = (3..=12).contains(&x) && (3..=12).contains(&y);
            rgba[index] = if inside { 124 } else { 28 };
            rgba[index + 1] = if inside { 154 } else { 30 };
            rgba[index + 2] = if inside { 112 } else { 27 };
            rgba[index + 3] = 255;
        }
    }
    Image::new_owned(rgba, 16, 16)
}

fn activate_existing_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(host) = app.try_state::<ManagedHost>() {
        let _ = host.mark_visible();
    }
}

fn shutdown_and_exit(app: &AppHandle) {
    let exit_code = app.try_state::<ManagedHost>().map_or(1, |host| {
        i32::from(host.shutdown(ExitReason::Explicit).is_err())
    });
    EXIT_ALLOWED.store(true, Ordering::Release);
    app.exit(exit_code);
}

fn spawn_heartbeat(app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("eam-host-heartbeat".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(HEARTBEAT_INTERVAL);
                if EXIT_ALLOWED.load(Ordering::Acquire) {
                    break;
                }
                if let Some(host) = app.try_state::<ManagedHost>() {
                    let _ = host.heartbeat();
                }
            }
        })?;
    Ok(())
}

fn format_update_failure(
    prefix: &str,
    shutdown_errors: &[String],
    reopen: Result<(), String>,
) -> String {
    let mut parts = vec![prefix.to_owned()];
    parts.extend(shutdown_errors.iter().cloned());
    if let Err(error) = reopen {
        parts.push(format!("Core reopen failed: {error}"));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_requires_https_endpoint_and_nonempty_public_key() {
        let configured = update_configuration_from(
            Some("https://updates.example.test/latest.json".to_owned()),
            Some("public-key".to_owned()),
        );
        assert_eq!(
            configured.as_ref().map(|(endpoint, _)| endpoint.scheme()),
            Some("https")
        );
        assert!(
            update_configuration_from(
                Some("http://updates.example.test/latest.json".to_owned()),
                Some("public-key".to_owned()),
            )
            .is_none()
        );
        assert!(
            update_configuration_from(
                Some("https://updates.example.test/latest.json".to_owned()),
                Some("   ".to_owned()),
            )
            .is_none()
        );
        assert!(update_configuration_from(None, Some("public-key".to_owned())).is_none());
    }

    #[test]
    fn update_failure_reports_reopen_failure_without_hiding_shutdown_errors() {
        let message = format_update_failure(
            "secure shutdown failed",
            &["checkpoint failed".to_owned()],
            Err("unlock failed".to_owned()),
        );
        assert!(message.contains("checkpoint failed"));
        assert!(message.contains("unlock failed"));
    }

    #[test]
    fn generated_tray_icon_is_fixed_size_and_opaque() {
        let icon = tray_icon();
        assert_eq!(icon.width(), 16);
        assert_eq!(icon.height(), 16);
        assert!(icon.rgba().chunks_exact(4).all(|pixel| pixel[3] == 255));
    }
}
