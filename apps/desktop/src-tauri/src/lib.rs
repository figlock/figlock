//! The desktop shell. The window only gets what a screen shows: the vault key and plaintext items stay on
//! this side of the IPC boundary (ADR-0020), and every command here is an explicit, narrow entry point.

mod desk;

use std::time::Duration;

use desk::cli_tool::CliToolView;
use desk::credentials::Changed;
use desk::devices::DeviceRow;
use desk::drive::{DriveView, SignedOut};
use desk::forgot::{CardInfo, Found, NewCredentials};
use desk::generate::{Generated, PassphraseSpec, PasswordSpec, StrengthView};
use desk::health::{HealthReport, PwnedPasswords, RangeSource};
use desk::items::{ItemDetail, ItemDraft, ItemForEdit, ItemSummary, Staged};
use desk::pairing::{InviteView, PairingStatus};
use desk::recovery::RecoveryMaterial;
use desk::sync::SyncView;
use desk::{CmdError, CmdRes, CreatedVault, Desk, Storage, VaultState};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

/// The window is told the vault locked by itself (idle or sleep) with this event.
const LOCKED_EVENT: &str = "figlock://locked";
/// The window is told a sync ran, so it can reload the items and the sync status.
const SYNCED_EVENT: &str = "figlock://synced";

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
    storage: Storage,
    password: String,
) -> CmdRes<CreatedVault> {
    desk.create(&storage, &password)
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

/// Make a new Recovery Key to show: a single code, or `count` cards of which `threshold` open the vault.
#[tauri::command]
async fn recovery_prepare(
    desk: State<'_, Desk>,
    shares: bool,
    threshold: u8,
    count: u8,
) -> CmdRes<RecoveryMaterial> {
    desk.recovery_prepare(shares, threshold, count)
}

/// The forced check that the Recovery Key shown is readable.
#[tauri::command]
async fn recovery_check(desk: State<'_, Desk>, answers: Vec<String>) -> CmdRes<bool> {
    desk.recovery_check(&answers)
}

/// Ask where to save the Recovery Key pages and write them there. `false` when the person cancels.
#[tauri::command]
async fn save_recovery_kit(app: AppHandle, desk: State<'_, Desk>) -> CmdRes<bool> {
    let pdf = desk.recovery_kit()?;
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name("FigLock Recovery Key.pdf")
        .add_filter("PDF", &["pdf"])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(false);
    };
    std::fs::write(path, &*pdf).map_err(|_| {
        CmdError::from(figlock_session::Fail::new(
            "Could not save the Recovery Key there.",
        ))
    })?;
    Ok(true)
}

#[tauri::command]
async fn recovery_cancel(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.recovery_cancel();
    Ok(())
}

/// Setup: no Recovery Key, with the acknowledgement that a forgotten password cannot be recovered.
#[tauri::command]
async fn recovery_skip(desk: State<'_, Desk>, acknowledged: bool) -> CmdRes<()> {
    desk.recovery_skip(acknowledged)
}

/// Setup: use the confirmed Recovery Key; it is written with the vault.
#[tauri::command]
async fn recovery_use_draft(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.recovery_use_draft()
}

/// Settings: write the confirmed Recovery Key into the vault.
#[tauri::command]
async fn recovery_enable(
    desk: State<'_, Desk>,
    password: String,
    secret_key: Option<String>,
) -> CmdRes<()> {
    desk.recovery_enable(&password, secret_key.as_deref())
}

/// Settings: turn the Recovery Key off.
#[tauri::command]
async fn recovery_disable(
    desk: State<'_, Desk>,
    password: String,
    secret_key: Option<String>,
) -> CmdRes<()> {
    desk.recovery_disable(&password, secret_key.as_deref())
}

/// What one Recovery Key card says: how many cards are needed, and this card's number.
#[tauri::command]
async fn forgot_card_info(text: String) -> CmdRes<CardInfo> {
    Desk::forgot_card_info(&text)
}

/// "I forgot my password": open the vault in `storage` with a Recovery Key (the code, or the cards) and say
/// what was found. Stores nothing.
#[tauri::command]
async fn forgot_find(
    desk: State<'_, Desk>,
    storage: Storage,
    answers: Vec<String>,
    shares: bool,
) -> CmdRes<Found> {
    desk.forgot_find(&storage, &answers, shares)
}

/// Choose the new master password; makes the new Secret Key (in memory) for a new Emergency Kit. With
/// `new_recovery_key` the confirmed new Recovery Key replaces the one just used.
#[tauri::command]
async fn forgot_prepare(
    desk: State<'_, Desk>,
    new_password: String,
    new_recovery_key: bool,
) -> CmdRes<NewCredentials> {
    desk.forgot_prepare(&new_password, new_recovery_key)
}

/// Store the new credentials and unlock. Needs the new Emergency Kit confirmed first.
#[tauri::command]
async fn forgot_commit(desk: State<'_, Desk>, touch_id: bool) -> CmdRes<VaultState> {
    desk.forgot_commit(touch_id)
}

#[tauri::command]
async fn forgot_cancel(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.forgot_cancel();
    Ok(())
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
    storage: Storage,
    password: String,
    secret_key: String,
    touch_id: bool,
) -> CmdRes<VaultState> {
    desk.open_existing(&storage, &password, &secret_key, touch_id)
}

/// Returns the fingerprint to compare with the trusted device.
#[tauri::command]
async fn join_start(desk: State<'_, Desk>, storage: Storage, code: String) -> CmdRes<String> {
    desk.join_start(&storage, &code)
}

/// Whether Google sign-in is possible in this build, and whether this device is signed in.
#[tauri::command]
async fn drive_status() -> CmdRes<DriveView> {
    desk::drive::status()
}

/// Opens the browser to sign in to Google and resolves when the person finishes (or the sign-in times out).
#[tauri::command]
async fn drive_sign_in() -> CmdRes<DriveView> {
    tauri::async_runtime::spawn_blocking(desk::drive::sign_in)
        .await
        .map_err(|_| CmdError::new("failed", "Sign-in stopped unexpectedly. Try again."))?
}

#[tauri::command]
async fn drive_sign_out() -> CmdRes<SignedOut> {
    desk::drive::sign_out()
}

/// Whether sync is on and how it is going, with no network call.
#[tauri::command]
async fn sync_status(desk: State<'_, Desk>) -> CmdRes<SyncView> {
    desk.sync_status()
}

/// Sync once now. `reupload` is the person's confirmation after the vault was lost from Drive.
#[tauri::command]
async fn sync_now(app: AppHandle, reupload: bool) -> CmdRes<SyncView> {
    let view = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || tauri::Manager::state::<Desk>(&app).sync_now(reupload)
    })
    .await
    .map_err(|_| CmdError::new("failed", "Sync stopped unexpectedly. Try again."))??;
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(view)
}

#[tauri::command]
async fn sync_enable(app: AppHandle) -> CmdRes<SyncView> {
    let view = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || tauri::Manager::state::<Desk>(&app).sync_enable()
    })
    .await
    .map_err(|_| CmdError::new("failed", "Sync stopped unexpectedly. Try again."))??;
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(view)
}

#[tauri::command]
async fn sync_disable(desk: State<'_, Desk>) -> CmdRes<SyncView> {
    desk.sync_disable()
}

/// The devices that can open this vault, then the revoked ones.
#[tauri::command]
async fn devices(desk: State<'_, Desk>) -> CmdRes<Vec<DeviceRow>> {
    desk.devices()
}

/// Revoke a device. `new_password` also changes the master password in the same write.
#[tauri::command]
async fn revoke_device(
    app: AppHandle,
    device: String,
    password: String,
    secret_key: Option<String>,
    new_password: Option<String>,
) -> CmdRes<Vec<DeviceRow>> {
    let rows = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            tauri::Manager::state::<Desk>(&app).revoke_device(
                &device,
                &password,
                secret_key.as_deref(),
                new_password.as_deref(),
            )
        }
    })
    .await
    .map_err(|_| {
        CmdError::new(
            "failed",
            "Revoking stopped unexpectedly. Check the device list.",
        )
    })??;
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(rows)
}

/// Change the master password, rotate the Secret Key, or both. A rotation returns the new key for the kit.
/// Open an invite for a new device: its code and QR. Replaces any invite already open.
#[tauri::command]
async fn add_device_start(desk: State<'_, Desk>) -> CmdRes<InviteView> {
    desk.add_device_start()
}

/// Whether the new device has answered, polled while the invite dialog is open.
#[tauri::command]
async fn add_device_status(desk: State<'_, Desk>) -> CmdRes<PairingStatus> {
    desk.add_device_status()
}

/// Approve the new device. Touch ID releases the kept Secret Key; without one, the password and key are typed.
#[tauri::command]
async fn add_device_approve(
    desk: State<'_, Desk>,
    password: Option<String>,
    secret_key: Option<String>,
) -> CmdRes<Vec<DeviceRow>> {
    desk.add_device_approve(password.as_deref(), secret_key.as_deref())
}

/// Close the open invite, or turn down the device that asked.
#[tauri::command]
async fn add_device_cancel(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.add_device_cancel()
}

#[tauri::command]
async fn change_credentials(
    app: AppHandle,
    password: String,
    secret_key: Option<String>,
    new_password: Option<String>,
    rotate: bool,
) -> CmdRes<Changed> {
    let changed = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            tauri::Manager::state::<Desk>(&app).change_credentials(
                &password,
                secret_key.as_deref(),
                new_password.as_deref(),
                rotate,
            )
        }
    })
    .await
    .map_err(|_| {
        CmdError::new(
            "failed",
            "The change stopped unexpectedly. Lock and unlock, then check which password works.",
        )
    })??;
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(changed)
}

/// Close the new Emergency Kit step once the new Secret Key is confirmed.
#[tauri::command]
async fn end_rotation(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.end_rotation()
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

/// `secret_key` is blank when this device keeps it. The kept key is read after Touch ID, which can wait for the
/// person, so this runs off the async workers.
#[tauri::command]
async fn unlock(
    app: AppHandle,
    password: String,
    secret_key: Option<String>,
) -> CmdRes<VaultState> {
    tauri::async_runtime::spawn_blocking(move || {
        tauri::Manager::state::<Desk>(&app).unlock(&password, secret_key.as_deref())
    })
    .await
    .map_err(|_| CmdError::new("failed", "Unlocking stopped unexpectedly. Try again."))?
}

/// The Secret Key kept on this device, after Touch ID or the device password.
#[tauri::command]
async fn show_secret_key(app: AppHandle) -> CmdRes<String> {
    tauri::async_runtime::spawn_blocking(move || {
        tauri::Manager::state::<Desk>(&app).show_secret_key()
    })
    .await
    .map_err(|_| CmdError::new("failed", "Showing the key stopped unexpectedly. Try again."))?
}

/// Stop keeping the Secret Key on this device.
#[tauri::command]
async fn forget_secret_key(desk: State<'_, Desk>) -> CmdRes<VaultState> {
    desk.forget_secret_key()
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

/// Whether the command line tool inside this app is linked onto the person's PATH. Needs no vault.
#[tauri::command]
async fn cli_tool_status() -> CmdRes<CliToolView> {
    desk::cli_tool::status()
}

/// Link the bundled command line tool at `~/.local/bin/figlock`. Only ever runs when the person asks.
#[tauri::command]
async fn install_cli_tool() -> CmdRes<CliToolView> {
    desk::cli_tool::install_here()
}

#[tauri::command]
async fn remove_cli_tool() -> CmdRes<CliToolView> {
    desk::cli_tool::remove_here()
}

/// A fresh password. Needs no vault, so it also works on the lock screen.
#[tauri::command]
async fn generate_password(spec: PasswordSpec) -> CmdRes<Generated> {
    desk::generate::password(&spec)
}

#[tauri::command]
async fn generate_passphrase(spec: PassphraseSpec) -> CmdRes<Generated> {
    desk::generate::passphrase(&spec)
}

/// How strong a typed password is, by the same rules the generator's own meter uses.
#[tauri::command]
async fn password_strength(value: String) -> StrengthView {
    desk::generate::strength(&value)
}

/// Weak, reused and old passwords, and with `check_breaches` also the ones in known breaches. That check sends
/// only the first 5 hex characters of each password's SHA-1 to the breach service.
#[tauri::command]
async fn health_report(app: AppHandle, check_breaches: bool) -> CmdRes<HealthReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = PwnedPasswords::new();
        tauri::Manager::state::<Desk>(&app)
            .health_report(check_breaches.then_some(&source as &dyn RangeSource))
    })
    .await
    .map_err(|_| {
        CmdError::new(
            "failed",
            "The health check stopped unexpectedly. Try again.",
        )
    })?
}

#[tauri::command]
async fn list_items(desk: State<'_, Desk>, query: String) -> CmdRes<Vec<ItemSummary>> {
    desk.list_items(&query)
}

#[tauri::command]
async fn list_deleted(desk: State<'_, Desk>) -> CmdRes<Vec<ItemSummary>> {
    desk.list_deleted()
}

#[tauri::command]
async fn item_detail(desk: State<'_, Desk>, id: String) -> CmdRes<ItemDetail> {
    desk.item_detail(&id)
}

/// One secret value, because the person pressed Show.
#[tauri::command]
async fn reveal_field(desk: State<'_, Desk>, id: String, field: String) -> CmdRes<String> {
    desk.reveal_field(&id, &field)
}

/// Copies a field without it passing through the window; resolves the seconds until it is cleared.
#[tauri::command]
async fn copy_field(desk: State<'_, Desk>, id: String, field: String) -> CmdRes<u64> {
    desk.copy_field(&id, &field)
}

/// Copies a generated value in the app core; resolves the seconds until it is cleared.
#[tauri::command]
async fn copy_text(desk: State<'_, Desk>, value: String) -> CmdRes<u64> {
    desk.copy_text(&value)
}

#[tauri::command]
async fn add_item(desk: State<'_, Desk>, kind: String, draft: ItemDraft) -> CmdRes<String> {
    desk.add_item(&kind, draft)
}

#[tauri::command]
async fn item_for_edit(desk: State<'_, Desk>, id: String) -> CmdRes<ItemForEdit> {
    desk.item_for_edit(&id)
}

#[tauri::command]
async fn update_item(desk: State<'_, Desk>, id: String, draft: ItemDraft) -> CmdRes<()> {
    desk.update_item(&id, draft)
}

#[tauri::command]
async fn delete_item(desk: State<'_, Desk>, id: String) -> CmdRes<()> {
    desk.delete_item(&id)
}

#[tauri::command]
async fn restore_item(desk: State<'_, Desk>, id: String) -> CmdRes<()> {
    desk.restore_item(&id)
}

/// The native file picker for a document's files; the chosen files are read here, not in the window.
#[tauri::command]
async fn choose_files(app: AppHandle, desk: State<'_, Desk>) -> CmdRes<Staged> {
    let paths: Vec<String> = app
        .dialog()
        .file()
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
        .collect();
    desk.stage_files(&paths)
}

/// Files dropped on the window.
#[tauri::command]
async fn stage_files(desk: State<'_, Desk>, paths: Vec<String>) -> CmdRes<Staged> {
    desk.stage_files(&paths)
}

#[tauri::command]
async fn unstage_file(desk: State<'_, Desk>, token: String) -> CmdRes<()> {
    desk.unstage_file(&token);
    Ok(())
}

#[tauri::command]
async fn clear_staged(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.clear_staged();
    Ok(())
}

/// Ask where to save a file of a document and write it there. `false` when the person cancels.
#[tauri::command]
async fn save_file(
    app: AppHandle,
    desk: State<'_, Desk>,
    id: String,
    file_id: String,
) -> CmdRes<bool> {
    let (name, bytes) = desk.file_bytes(&id, &file_id)?;
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name(&name)
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(false);
    };
    figlock_session::docfile::write_out(&bytes, &path.display().to_string(), true)
        .map_err(CmdError::from)?;
    Ok(true)
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
            // Keeps the local vault matched to Drive, locked or not: it only moves ciphertext.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let desk = tauri::Manager::state::<Desk>(&handle);
                    if desk.sync_tick() {
                        let _ = handle.emit(SYNCED_EVENT, ());
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
            forgot_card_info,
            forgot_find,
            forgot_prepare,
            forgot_commit,
            forgot_cancel,
            recovery_prepare,
            recovery_check,
            save_recovery_kit,
            recovery_cancel,
            recovery_skip,
            recovery_use_draft,
            recovery_enable,
            recovery_disable,
            finish_create,
            cancel_create,
            open_existing,
            join_start,
            drive_status,
            drive_sign_in,
            drive_sign_out,
            sync_status,
            sync_now,
            sync_enable,
            sync_disable,
            devices,
            revoke_device,
            add_device_start,
            add_device_status,
            add_device_approve,
            add_device_cancel,
            change_credentials,
            end_rotation,
            join_answered,
            join_cancel,
            join_finish,
            unlock,
            show_secret_key,
            forget_secret_key,
            quick_unlock,
            lock,
            activity,
            cli_tool_status,
            install_cli_tool,
            remove_cli_tool,
            generate_password,
            generate_passphrase,
            password_strength,
            health_report,
            list_items,
            list_deleted,
            item_detail,
            reveal_field,
            copy_field,
            copy_text,
            add_item,
            item_for_edit,
            update_item,
            delete_item,
            restore_item,
            choose_files,
            stage_files,
            unstage_file,
            clear_staged,
            save_file,
        ])
        .run(tauri::generate_context!())
        .expect("the FigLock desktop app failed to start");
}
