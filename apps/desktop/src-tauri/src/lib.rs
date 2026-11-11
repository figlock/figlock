//! The desktop shell. The window only gets what a screen shows: the vault key and plaintext items stay on
//! this side of the IPC boundary (ADR-0020), and every command here is an explicit, narrow entry point.

use serde::Serialize;

/// What the window may know about the vault before anything is unlocked.
#[derive(Serialize)]
struct VaultState {
    initialised: bool,
    version: &'static str,
}

#[tauri::command]
fn vault_state() -> Result<VaultState, String> {
    let initialised = figlock_session::config::is_initialised()
        .map_err(|_| "could not read this device's FigLock setup".to_string())?;
    Ok(VaultState {
        initialised,
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![vault_state])
        .run(tauri::generate_context!())
        .expect("the FigLock desktop app failed to start");
}
