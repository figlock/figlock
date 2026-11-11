//! The desktop shell. The window only gets what a screen shows: the vault key and plaintext items stay on
//! this side of the IPC boundary (ADR-0020), and every command here is an explicit, narrow entry point.

mod desk;

use std::time::Duration;

use desk::{CmdError, CmdRes, CreatedVault, Desk, VaultState};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

/// The window is told the vault locked by itself (idle or sleep) with this event.
const LOCKED_EVENT: &str = "figlock://locked";

#[tauri::command]
async fn vault_state(desk: State<'_, Desk>) -> CmdRes<VaultState> {
    desk.state()
}

/// The folder a new vault goes in unless the person picks another.
#[tauri::command]
async fn suggested_folder() -> Option<String> {
    Desk::suggested_folder().map(|p| p.display().to_string())
}

/// The native folder picker; `None` when the person cancels.
#[tauri::command]
async fn choose_folder(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
}

#[tauri::command]
async fn create_vault(
    desk: State<'_, Desk>,
    folder: String,
    password: String,
) -> CmdRes<CreatedVault> {
    desk.create(&folder, &password)
}

/// Ask where to save the Emergency Kit and write it there. `false` when the person cancels.
#[tauri::command]
async fn save_emergency_kit(app: AppHandle, desk: State<'_, Desk>) -> CmdRes<bool> {
    let pdf = desk.emergency_kit()?;
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name("FigLock Emergency Kit.pdf")
        .add_filter("PDF", &["pdf"])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(false);
    };
    std::fs::write(path, &*pdf).map_err(|_| {
        CmdError::from(figlock_session::Fail::new(
            "Could not save the Emergency Kit there.",
        ))
    })?;
    Ok(true)
}

#[tauri::command]
async fn check_kit_key(desk: State<'_, Desk>, typed: String) -> CmdRes<bool> {
    desk.check_kit_key(&typed)
}

#[tauri::command]
async fn finish_create(desk: State<'_, Desk>, touch_id: bool) -> CmdRes<VaultState> {
    desk.finish_create(touch_id)
}

#[tauri::command]
async fn cancel_create(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.cancel_create();
    Ok(())
}

#[tauri::command]
async fn open_existing(
    desk: State<'_, Desk>,
    folder: String,
    password: String,
    secret_key: String,
    touch_id: bool,
) -> CmdRes<VaultState> {
    desk.open_existing(&folder, &password, &secret_key, touch_id)
}

/// Returns the fingerprint to compare with the trusted device.
#[tauri::command]
async fn join_start(desk: State<'_, Desk>, folder: String, code: String) -> CmdRes<String> {
    desk.join_start(&folder, &code)
}

#[tauri::command]
async fn join_answered(desk: State<'_, Desk>) -> CmdRes<bool> {
    desk.join_answered()
}

#[tauri::command]
async fn join_cancel(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.join_cancel();
    Ok(())
}

#[tauri::command]
async fn join_finish(
    desk: State<'_, Desk>,
    password: String,
    touch_id: bool,
) -> CmdRes<VaultState> {
    desk.join_finish(&password, touch_id)
}

#[tauri::command]
async fn unlock(desk: State<'_, Desk>, password: String, secret_key: String) -> CmdRes<VaultState> {
    desk.unlock(&password, &secret_key)
}

#[tauri::command]
async fn quick_unlock(desk: State<'_, Desk>) -> CmdRes<VaultState> {
    desk.quick_unlock()
}

#[tauri::command]
async fn lock(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.lock();
    Ok(())
}

/// The window saw keyboard or pointer input; restarts the idle timer.
#[tauri::command]
async fn activity(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.activity();
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Desk::new())
        .setup(|app| {
            let handle = app.handle().clone();
            // Locks on idle, and as the Mac wakes: a tick that arrives long after the last one means it slept.
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let desk = tauri::Manager::state::<Desk>(&handle);
                    if desk.tick() {
                        let _ = handle.emit(LOCKED_EVENT, ());
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault_state,
            suggested_folder,
            choose_folder,
            create_vault,
            save_emergency_kit,
            check_kit_key,
            finish_create,
            cancel_create,
            open_existing,
            join_start,
            join_answered,
            join_cancel,
            join_finish,
            unlock,
            quick_unlock,
            lock,
            activity,
        ])
        .run(tauri::generate_context!())
        .expect("the FigLock desktop app failed to start");
}
