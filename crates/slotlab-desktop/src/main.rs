// SlotLab desktop shell: starts the axum server in-process (UI :3600 +
// emulated RGS :3601), shows the bundled splash while it boots, then
// navigates the window to the embedded UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

const UI_PORT: u16 = 3600;
const RGS_PORT: u16 = 3601;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "slotlab=info,tower_http=warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || boot(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running SlotLab");
}

/// Boot sequence, off the main thread so the splash shows immediately:
/// reuse a running instance if one answers on :3600, otherwise start the
/// server in-process, then swap the window to the UI once it responds.
///
/// When attached to an EXTERNAL server (a `slotlab` CLI), keep watching it:
/// if it goes away (user quits the CLI), start our own server and reload.
fn boot(handle: tauri::AppHandle) {
    loop {
        match probe(UI_PORT) {
            Probe::SlotLab => {
                navigate_to_ui(&handle);
                watch_external(); // returns when the external server is gone
                tracing::warn!("attached SlotLab server disappeared — starting our own");
            }
            Probe::Other => {
                fail(
                    &handle,
                    &format!(
                        "Port {UI_PORT} is already used by another application.\n\
                         Close it and launch SlotLab again."
                    ),
                );
                return;
            }
            Probe::Free => {
                std::thread::spawn(|| {
                    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                    if let Err(e) = rt.block_on(slotlab_app::run_server(Default::default())) {
                        tracing::error!("server exited: {e:#}");
                    }
                });

                // The server binds fast; the generous timeout covers a cold
                // start with many registered games (registry parse).
                for _ in 0..300 {
                    if matches!(probe(UI_PORT), Probe::SlotLab) {
                        navigate_to_ui(&handle);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                fail(
                    &handle,
                    &format!(
                        "The local server did not start (ports {UI_PORT}/{RGS_PORT}).\n\
                         Check that no other application uses these ports, then relaunch."
                    ),
                );
                return;
            }
        }
    }
}

/// Blocks while an external SlotLab server answers on :3600; returns after
/// a few consecutive missed probes (transient hiccups don't count).
fn watch_external() {
    let mut misses = 0;
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if matches!(probe(UI_PORT), Probe::SlotLab) {
            misses = 0;
        } else {
            misses += 1;
            if misses >= 3 {
                return;
            }
        }
    }
}

enum Probe {
    /// Nothing listens on the port.
    Free,
    /// A SlotLab server answers.
    SlotLab,
    /// Something else listens on the port.
    Other,
}

/// Minimal HTTP GET /api/ui/health over a raw socket (no client dependency).
fn probe(port: u16) -> Probe {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return Probe::Free;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let request =
        format!("GET /api/ui/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Probe::Other;
    }
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body);
    // New servers answer JSON on /api/ui/health; older ones fall back to the
    // embedded UI's index.html — both are SlotLab.
    if body.contains("\"app\":\"slotlab\"") || body.contains("<title>SlotLab</title>") {
        Probe::SlotLab
    } else {
        Probe::Other
    }
}

fn navigate_to_ui(handle: &tauri::AppHandle) {
    let url = format!("http://localhost:{UI_PORT}")
        .parse()
        .expect("UI url");
    if let Some(window) = handle.get_webview_window("main") {
        if let Err(e) = window.navigate(url) {
            fail(handle, &format!("Could not open the UI: {e}"));
        }
    }
}

fn fail(handle: &tauri::AppHandle, message: &str) {
    tracing::error!("{message}");
    handle
        .dialog()
        .message(message)
        .kind(MessageDialogKind::Error)
        .title("SlotLab")
        .blocking_show();
    handle.exit(1);
}
