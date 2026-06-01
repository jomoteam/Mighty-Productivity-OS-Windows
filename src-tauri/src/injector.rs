use std::thread;
use std::time::Duration;
use tauri_plugin_clipboard_manager::ClipboardExt;
use enigo::{Enigo, Keyboard, Settings};

/// Write text to clipboard then paste it into the target app via native keyboard simulation.
pub fn inject_via_clipboard(app: &tauri::AppHandle, text: &str, _target_app: Option<&str>) {
    // Write to clipboard using native Tauri clipboard manager
    let _ = app.clipboard().write_text(text.to_string());

    thread::sleep(Duration::from_millis(50));

    // Activate target app by PID using osascript (only for macOS)
    #[cfg(target_os = "macos")]
    if let Some(pid) = target_app {
        let script = format!(
            "tell application \"System Events\"\nset frontmost of first application process whose unix id is {pid} to true\nend tell"
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output();
        thread::sleep(Duration::from_millis(50));
    }

    // Simulate Cmd+V (or Ctrl+V) natively using enigo.
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        #[cfg(target_os = "macos")]
        {
            let _ = enigo.key(Key::Meta, Press);
            let _ = enigo.key(Key::Unicode('v'), Click);
            let _ = enigo.key(Key::Meta, Release);
        }

        #[cfg(not(target_os = "macos"))]
        {
            // On Windows/Linux, avoid Ctrl+V paste simulation because users may bind
            // push-to-talk to shortcuts containing V (e.g. Ctrl+V / Ctrl+Shift+V),
            // which can retrigger/echo keys. Inject text directly instead.
            let _ = enigo.text(text);
        }
    }
}

pub fn paste_on_main_thread(app: &tauri::AppHandle) {
    inject_via_clipboard(app, "", None);
}
