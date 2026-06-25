mod core;

use tauri::Emitter;

/// IPC command: greet — verifies front-to-back communication
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Cognest backend is ready.", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Emit event to frontend to verify back-to-front IPC
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // Small delay to ensure frontend listener is ready
                std::thread::sleep(std::time::Duration::from_millis(500));
                let _ = handle.emit("backend-ready", "Backend initialized successfully");
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
