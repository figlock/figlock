//! The desktop shell. The window only gets what a screen shows: the vault key and plaintext items stay on
//! this side of the IPC boundary (ADR-0020), and every command here is an explicit, narrow entry point.

mod about;
mod desk;

use std::time::Duration;

use desk::cli_tool::CliToolView;
use desk::credentials::Changed;
use desk::devices::DeviceRow;
use desk::drive::{DriveView, SignedOut};
use desk::forgot::{Found, NewCredentials};
use desk::generate::{Generated, PassphraseSpec, PasswordSpec, StrengthView};
use desk::health::{HealthReport, PwnedPasswords, RangeSource};
use desk::items::{ItemDetail, ItemDraft, ItemForEdit, ItemSummary, Staged};
use desk::pairing::{InviteView, PairingStatus};
use desk::recovery::RecoveryMaterial;
use desk::sync::SyncView;
use desk::update::UpdateView;
use desk::{CmdError, CmdRes, CreatedVault, Desk, Storage, VaultState};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

/// The window is told the vault locked by itself (idle or sleep) with this event.
const LOCKED_EVENT: &str = "figlock://locked";
/// The window is told a sync ran, so it can reload the items and the sync status.
const SYNCED_EVENT: &str = "figlock://synced";
/// An agent asks for a secret; the payload is the card to show (ADR-0040).
const AGENT_REQUEST_EVENT: &str = "figlock://agent-request";
/// A request was answered or timed out; the payload is its id.
const AGENT_DONE_EVENT: &str = "figlock://agent-request-done";

#[tauri::command]
async fn vault_state(desk: State<'_, Desk>) -> CmdRes<VaultState> {
    desk.state()
}

/// Change how long the vault waits without activity before it locks.
#[tauri::command]
async fn set_autolock(desk: State<'_, Desk>, secs: u64) -> CmdRes<VaultState> {
    desk.set_autolock(secs)
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

/// Make the Emergency Kit again from the Secret Key kept on this Mac (or the one typed, with the password,
/// when none is kept) and ask where to save it. Reading the kept key asks for Touch ID or the device password
/// first. `false` when the person cancels the save dialog; nothing is written then (FIG-134).
#[tauri::command]
async fn save_emergency_kit_again(
    app: AppHandle,
    desk: State<'_, Desk>,
    password: Option<String>,
    secret_key: Option<String>,
) -> CmdRes<bool> {
    let typed = match (password.as_deref(), secret_key.as_deref()) {
        (Some(p), Some(k)) => Some((p, k)),
        _ => None,
    };
    let pdf = desk.kit_again(typed)?;
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

/// Ask where to save the vault as JSON and write it there, owner-only. Secrets are included only after Touch ID
/// or the password again (FIG-287). `false` when the person cancels the save dialog; nothing is written then.
#[tauri::command]
async fn export_vault(
    app: AppHandle,
    desk: State<'_, Desk>,
    include_secrets: bool,
    include_deleted: bool,
    password: Option<String>,
    secret_key: Option<String>,
) -> CmdRes<bool> {
    let text = desk.export_vault(
        include_secrets,
        include_deleted,
        password.as_deref(),
        secret_key.as_deref(),
    )?;
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name("figlock-export.json")
        .add_filter("JSON", &["json"])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(false);
    };
    figlock_session::docfile::write_out(text.as_bytes(), &path.display().to_string(), true)
        .map_err(CmdError::from)?;
    Ok(true)
}

/// Make a new Recovery Key to show.
#[tauri::command]
async fn recovery_prepare(desk: State<'_, Desk>) -> CmdRes<RecoveryMaterial> {
    desk.recovery_prepare()
}

/// The forced check that the Recovery Key shown is readable.
#[tauri::command]
async fn recovery_check(desk: State<'_, Desk>, answer: String) -> CmdRes<bool> {
    desk.recovery_check(&answer)
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

/// "I forgot my password": open the vault in `storage` with the Recovery Key code and say what was found.
/// Stores nothing.
#[tauri::command]
async fn forgot_find(desk: State<'_, Desk>, storage: Storage, code: String) -> CmdRes<Found> {
    desk.forgot_find(&storage, &code)
}

/// Choose the new password; makes the new Secret Key (in memory) for a new Emergency Kit. With
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

/// The vault folder is gone: point this device at the folder the person moved it to.
#[tauri::command]
async fn locate_folder(folder: String) -> CmdRes<()> {
    desk::folder::locate(&folder)
}

/// The vault folder is gone: download the vault from Google Drive into a new local folder.
#[tauri::command]
async fn restore_from_drive(folder: Option<String>) -> CmdRes<()> {
    restore_from(folder, desk::folder::RestoreFrom::Drive).await
}

/// The vault folder is gone: copy the vault from the `FigLock` folder in iCloud Drive into a new local folder.
#[tauri::command]
async fn restore_from_icloud(folder: Option<String>) -> CmdRes<()> {
    restore_from(folder, desk::folder::RestoreFrom::Icloud).await
}

async fn restore_from(folder: Option<String>, from: desk::folder::RestoreFrom) -> CmdRes<()> {
    tauri::async_runtime::spawn_blocking(move || desk::folder::restore(folder.as_deref(), from))
        .await
        .map_err(|_| CmdError::new("failed", "Restoring stopped unexpectedly. Try again."))?
}

/// The vault folder is gone: this Mac stops using the vault. Needs the person's confirmation.
#[tauri::command]
async fn forget_vault(confirmed: bool) -> CmdRes<()> {
    desk::folder::forget(confirmed)
}

/// A locked Mac that cannot get back in stops using its vault (R3). Needs the person's confirmation.
#[tauri::command]
async fn forget_locked_vault(desk: State<'_, Desk>, confirmed: bool) -> CmdRes<()> {
    desk.forget_locked(confirmed)
}

/// The vault removed this Mac: stop using it here. Refused unless the vault says so.
#[tauri::command]
async fn leave_vault(desk: State<'_, Desk>) -> CmdRes<()> {
    desk.leave_vault()
}

/// Whether Google sign-in is possible in this build, and whether this device is signed in.
#[tauri::command]
async fn drive_status() -> CmdRes<DriveView> {
    desk::drive::status()
}

/// Ends a Google sign-in that is waiting for the browser, so the person need not wait out the timeout.
#[tauri::command]
async fn drive_cancel_sign_in() {
    desk::drive::cancel_sign_in();
}

/// Opens the browser to sign in to Google and resolves when the person finishes (or the sign-in times out).
#[tauri::command]
async fn drive_sign_in(app: AppHandle) -> CmdRes<DriveView> {
    let view = tauri::async_runtime::spawn_blocking(desk::drive::sign_in)
        .await
        .map_err(|_| CmdError::new("failed", "Sign-in stopped unexpectedly. Try again."))??;
    // The window re-reads the sync status the moment the sign-in is done, so it never shows an older state.
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(view)
}

#[tauri::command]
async fn drive_sign_out() -> CmdRes<SignedOut> {
    desk::drive::sign_out()
}

/// Sign out of Google from Settings: turns sync off too, then revokes the sign-in.
#[tauri::command]
async fn sync_sign_out(app: AppHandle) -> CmdRes<SignedOut> {
    let out = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || tauri::Manager::state::<Desk>(&app).sync_sign_out()
    })
    .await
    .map_err(|_| CmdError::new("failed", "Signing out stopped unexpectedly. Try again."))??;
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(out)
}

/// Whether iCloud Drive can be used on this Mac, for the setup screens. No vault is needed.
#[tauri::command]
async fn icloud_status() -> CmdRes<desk::sync::IcloudView> {
    Ok(desk::sync::icloud_view())
}

/// Whether sync is on and how it is going, with no network call.
#[tauri::command]
async fn sync_status(desk: State<'_, Desk>) -> CmdRes<SyncView> {
    desk.sync_status()
}

/// Learns which Google account this Mac is signed in with, for a sign-in made before the address was kept.
/// Asks Drive, so it runs off the async runtime; offline just changes nothing.
#[tauri::command]
async fn sync_refresh_account(app: AppHandle) -> CmdRes<SyncView> {
    tauri::async_runtime::spawn_blocking(move || {
        tauri::Manager::state::<Desk>(&app).sync_refresh_account()
    })
    .await
    .map_err(|_| CmdError::new("failed", "Reading the Google account stopped unexpectedly."))?
}

/// Sync once now: every place, or only `remote` (its id). `reupload` is the person's confirmation after the vault
/// was lost from a place.
#[tauri::command]
async fn sync_now(app: AppHandle, remote: Option<String>, reupload: bool) -> CmdRes<SyncView> {
    let view = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || tauri::Manager::state::<Desk>(&app).sync_now(remote.as_deref(), reupload)
    })
    .await
    .map_err(|_| CmdError::new("failed", "Sync stopped unexpectedly. Try again."))??;
    let _ = app.emit(SYNCED_EVENT, ());
    Ok(view)
}

/// Delete the vault's copy from Google Drive after the typed phrase. Runs off the UI thread: it makes a request per
/// file.
#[tauri::command]
async fn sync_delete_remote(app: AppHandle, confirmation: String) -> CmdRes<SyncView> {
    let view = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || tauri::Manager::state::<Desk>(&app).sync_delete_remote(&confirmation)
    })
    .await
    .map_err(|_| CmdError::new("failed", "Deleting stopped unexpectedly. Try again."))??;
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

/// Turn iCloud Drive sync on: no sign-in, the `FigLock` folder in iCloud Drive.
#[tauri::command]
async fn sync_enable_icloud(app: AppHandle) -> CmdRes<SyncView> {
    let view = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || tauri::Manager::state::<Desk>(&app).sync_enable_icloud()
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

/// Stop syncing to one place (its id), leaving the others.
#[tauri::command]
async fn sync_disable_remote(desk: State<'_, Desk>, remote: String) -> CmdRes<SyncView> {
    desk.sync_disable_remote(&remote)
}

/// The devices that can open this vault, then the revoked ones.
#[tauri::command]
async fn devices(desk: State<'_, Desk>) -> CmdRes<Vec<DeviceRow>> {
    desk.devices()
}

/// Revoke a device. `new_password` also changes the password in the same write.
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

/// Change the password, rotate the Secret Key, or both. A rotation returns the new key for the kit.
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

/// Keep the Secret Key another device just made, in a window that is already open.
#[tauri::command]
async fn update_secret_key(
    desk: State<'_, Desk>,
    password: String,
    secret_key: String,
) -> CmdRes<VaultState> {
    desk.update_secret_key(&password, &secret_key)
}

/// Keep the Secret Key on this device again.
#[tauri::command]
async fn keep_secret_key(
    desk: State<'_, Desk>,
    password: String,
    secret_key: String,
) -> CmdRes<VaultState> {
    desk.keep_secret_key(&password, &secret_key)
}

/// Stop keeping the Secret Key on this device.
#[tauri::command]
async fn forget_secret_key(desk: State<'_, Desk>) -> CmdRes<VaultState> {
    desk.forget_secret_key()
}

/// Stop unlocking this vault with Touch ID.
#[tauri::command]
async fn turn_touch_id_off(desk: State<'_, Desk>) -> CmdRes<VaultState> {
    desk.turn_touch_id_off()
}

/// Unlock this vault with Touch ID from now on; the password confirms it.
#[tauri::command]
async fn turn_touch_id_on(
    desk: State<'_, Desk>,
    password: String,
    secret_key: String,
) -> CmdRes<VaultState> {
    desk.turn_touch_id_on(&password, &secret_key)
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

/// Who answers agents on the broker socket: this window, a broker started in a terminal, or nobody (ADR-0040).
#[tauri::command]
async fn broker_status(desk: State<'_, Desk>) -> CmdRes<desk::broker_host::Mode> {
    Ok(desk.broker_mode())
}

/// Stop the broker running in a terminal and host here instead. Its grants end.
#[tauri::command(async)]
fn broker_take_over(desk: State<'_, Desk>) -> CmdRes<desk::broker_host::Mode> {
    desk.take_over_broker()
}

/// The agent requests waiting for an answer, for a window that opened after one came in (ADR-0040).
#[tauri::command]
async fn agent_requests(desk: State<'_, Desk>) -> CmdRes<Vec<desk::agent_requests::Card>> {
    Ok(desk.agent_cards())
}

/// The person pressed a button on a request card. Approve opens the system Touch ID prompt and only an
/// accepted prompt allows the request; this waits for it, so it must not run on the main thread.
#[tauri::command(async)]
fn agent_decide(
    desk: State<'_, Desk>,
    id: u64,
    approve: bool,
) -> CmdRes<desk::agent_requests::Decided> {
    Ok(desk.agent_decide(id, approve))
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

/// Look for a newer version of the app. Needs no vault. A build without an update key answers `unavailable`.
#[tauri::command]
async fn update_check(app: AppHandle) -> CmdRes<UpdateView> {
    desk::update::check(&app).await
}

/// Install the update the last check found, after the person asked for it. Needs no vault.
#[tauri::command]
async fn update_install(app: AppHandle) -> CmdRes<UpdateView> {
    desk::update::install(&app).await
}

/// Start the app again so the installed update runs. The vault locks as the process ends.
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
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

/// Show an agent request to the person. True when someone can see it: the window is up and either a
/// notification went out or the window is already in front.
fn agent_event(handle: &tauri::AppHandle, event: &desk::agent_requests::Event) -> bool {
    use desk::agent_requests::Event;
    use tauri_plugin_notification::NotificationExt;
    match event {
        Event::Open(card) => {
            let Some(window) = tauri::Manager::get_webview_window(handle, "main") else {
                return false;
            };
            let in_front = window.is_focused().unwrap_or(false);
            let notified = handle
                .notification()
                .builder()
                .title("FigLock")
                .body(card.headline())
                .show()
                .is_ok();
            if !notified && !in_front {
                return false;
            }
            handle.emit(AGENT_REQUEST_EVENT, &**card).is_ok()
        }
        Event::Closed(id) => handle.emit(AGENT_DONE_EVENT, id).is_ok(),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Desk::new())
        .manage(desk::update::Pending::default())
        // Coming back to the window picks up what the CLI wrote while it was in the background (FIG-126).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(true) = event {
                let handle = tauri::Manager::app_handle(window).clone();
                std::thread::spawn(move || {
                    if tauri::Manager::state::<Desk>(&handle).refresh_local() {
                        let _ = handle.emit(SYNCED_EVENT, ());
                    }
                });
            }
        })
        .setup(|app| {
            // Only a build whose config has an update key and feed can update; the plugin cannot start without
            // them, so a development build leaves it out and the update commands say so (FIG-112).
            if desk::update::configured(app.handle()) {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            about::install_menu(app.handle())?;
            // An agent request shows a card in the window and posts a notification (ADR-0040). If neither can
            // be seen, the broker falls back to the direct Touch ID prompt.
            let handle = app.handle().clone();
            tauri::Manager::state::<Desk>(app.handle())
                .approvals()
                .set_sink(Box::new(move |event| agent_event(&handle, event)));
            // Agents can ask for secrets as soon as the vault is unlocked, with no terminal step (ADR-0040).
            tauri::Manager::state::<Desk>(app.handle()).host_broker();
            // Locks as the Mac goes to sleep or the screen locks, before anyone sees it again (FIG-131).
            let handle = app.handle().clone();
            figlock_biometric::power::watch_sleep(move || {
                let desk = tauri::Manager::state::<Desk>(&handle);
                if desk.lock_for_sleep() {
                    let _ = handle.emit(LOCKED_EVENT, ());
                }
            });
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
            // Keeps the local vault matched to its remotes, locked or not: it only moves ciphertext. It sleeps
            // until a place is due or a save wakes it, so a save goes out at once and an idle app stays idle
            // (FIG-261).
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    let desk = tauri::Manager::state::<Desk>(&handle);
                    // Both run: a sync that pulled, and another process (the CLI) that wrote locally.
                    let synced = desk.sync_tick();
                    if desk.local_tick() || synced {
                        let _ = handle.emit(SYNCED_EVENT, ());
                    }
                    desk.wait_for_sync_work();
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
            save_emergency_kit_again,
            leave_vault,
            check_kit_key,
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
            locate_folder,
            restore_from_drive,
            restore_from_icloud,
            forget_vault,
            forget_locked_vault,
            drive_status,
            drive_sign_in,
            drive_cancel_sign_in,
            drive_sign_out,
            sync_status,
            sync_refresh_account,
            icloud_status,
            sync_now,
            sync_enable,
            sync_enable_icloud,
            sync_delete_remote,
            sync_sign_out,
            sync_disable,
            sync_disable_remote,
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
            keep_secret_key,
            update_secret_key,
            set_autolock,
            quick_unlock,
            turn_touch_id_off,
            turn_touch_id_on,
            lock,
            broker_status,
            agent_requests,
            agent_decide,
            broker_take_over,
            activity,
            update_check,
            update_install,
            restart_app,
            about::open_about_page,
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
            export_vault,
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
        .build(tauri::generate_context!())
        .expect("the FigLock desktop app failed to start")
        .run(|app, event| {
            // Frees the broker socket so the next start (or a CLI broker) finds it clean.
            if let tauri::RunEvent::Exit = event {
                tauri::Manager::state::<Desk>(app).stop_broker();
            }
        });
}
