//! The `figlock` command line: create a vault in a folder you own and add, find, read, copy and export items.
//!
//! Secrets are never taken from arguments: password, Secret Key and secret field values come from a
//! hidden prompt (or stdin for field values). `get` and `export` hide secret values unless asked.

#[cfg(unix)]
#[cfg(unix)]
mod broker;
mod commands;
mod credentials;
mod drive;
#[cfg(unix)]
mod import;
mod pairing;
mod prompt;
mod quick;
mod recovery;
mod remotes;
mod render;
#[cfg(unix)]
#[cfg(unix)]
mod skill;
mod sync_cmd;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use figlock_core::{ItemType, Source};
use figlock_session::error;

const TOP_HELP: &str = "\
Getting started:
  figlock init --store ~/Documents/figlock-vault    Create a vault (a synced folder works), set up this device
  figlock init --icloud                             ...and keep a copy in iCloud Drive (Macs; add --drive for Google Drive too)
  figlock add -t login --title GitHub --set username=ada --secret password
  figlock find github                               Search (fuzzy)
  figlock get github --reveal                       Show an item with its secrets
  figlock copy github                               Copy the password; the clipboard is cleared after 30 seconds

Adding another device:
  figlock device add                                On a device that has the vault: show an invite code, approve
  figlock join <code> --store <folder>              On the new device (--drive, --icloud or --sync-folder <path> for a synced vault)
  figlock open --store <folder>                     No device at hand: password and Secret Key instead

Giving an agent access without handing it the secret:
  figlock broker start                              Unlock once and approve requests (Touch ID)
  figlock run --item \"API key\" -- ./deploy.sh     Run a program with the secret in its environment
  figlock http --item \"API key\" https://api.example.com/me

Item types: login, token, note, env, ssh, doc. `figlock add --help` lists the fields of each.
Everything is stored encrypted; nothing leaves this machine except ciphertext in the folder or Drive you choose.
More: https://figlock.com";

#[derive(Parser)]
#[command(
    name = "figlock",
    version,
    about = "Headless, agent-aware password manager",
    long_about = "FigLock keeps passwords, API tokens and files in one encrypted vault you can use from the \
command line, and lets AI agents use a secret after you approve it, without ever seeing it.",
    after_help = TOP_HELP
)]
struct Cli {
    /// Machine-readable output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a vault in a folder and set up this device.
    Init(InitArgs),
    /// Add an item.
    Add(AddArgs),
    /// Change, delete or restore an item.
    Edit(EditArgs),
    /// Search items (fuzzy; empty query lists the most recent).
    Find(FindArgs),
    /// Show an item. Secret values stay hidden unless --reveal.
    Get(GetArgs),
    /// Copy a field to the clipboard and clear it again.
    Copy(CopyArgs),
    /// Print a random password.
    Generate(GenerateArgs),
    /// Print every item as JSON. Secret values need --include-secrets.
    Export(ExportArgs),
    /// Import another manager's export or a notes dump.
    Import(ImportArgs),
    /// Run and talk to the broker that gives agents approved, time-boxed access.
    Broker {
        #[command(subcommand)]
        action: BrokerCmd,
    },
    /// Verify and print the broker's audit log.
    Audit(AuditArgs),
    /// Run a program with secrets in its environment, after the broker's approval. Its output is scrubbed.
    Run(RunArgs),
    /// Make an HTTP request through the broker, which adds the secret itself. You never see the secret.
    Http(HttpArgs),
    /// Unlock with Touch ID or the device password instead of the password and Secret Key.
    QuickUnlock {
        #[command(subcommand)]
        action: QuickCmd,
    },
    /// Turn the Recovery Key on, replace it or turn it off. It lets `figlock forgot` reset a forgotten password.
    RecoveryKey {
        #[command(subcommand)]
        action: RecoveryCmd,
    },
    /// Reset a forgotten password with the Recovery Key. The old password and Secret Key stop working.
    Forgot(ForgotArgs),
    /// Change the password.
    Password {
        #[command(subcommand)]
        action: PasswordCmd,
    },
    /// Add this device to a vault that already exists, using an invite code from a device that has it.
    Join(JoinArgs),
    /// Open a vault that already exists with your password and Secret Key, when no other device is at hand.
    Open(OpenArgs),
    /// Replace, show or forget the Secret Key.
    SecretKey {
        #[command(subcommand)]
        action: SecretKeyCmd,
    },
    /// See and revoke the devices that can open this vault.
    Device {
        #[command(subcommand)]
        action: DeviceCmd,
    },
    /// Lock the vault: the next command asks for the full unlock (or Touch ID again if quick unlock is on).
    Lock,
    /// Sign this device in to Google Drive, where a vault can be stored.
    Drive {
        #[command(subcommand)]
        action: DriveCmd,
    },
    /// Keep this device's vault matched to Google Drive. With no subcommand, sync once now.
    Sync(SyncArgs),
    /// The Claude skill that teaches agents to use secrets through the broker.
    Skill {
        #[command(subcommand)]
        action: SkillCmd,
    },
}

#[derive(Subcommand)]
enum PasswordCmd {
    /// Ask for the current password and Secret Key, then for a new password.
    Change,
}

#[derive(Subcommand)]
enum SecretKeyCmd {
    /// Make a new Secret Key and show it once. The old one stops working everywhere.
    Rotate,
    /// Show the Secret Key kept on this device, after Touch ID or the device password.
    Show,
    /// Save the Emergency Kit PDF again, made now from the Secret Key kept on this device, after Touch ID or
    /// the device password.
    Kit(KitArgs),
    /// Stop keeping the Secret Key on this device. Every full unlock asks for it until you run `keep`.
    Forget,
    /// Keep the Secret Key on this device again, after asking for the password and Secret Key once.
    Keep,
}

#[derive(Args)]
struct KitArgs {
    /// Where to write the PDF (`-` for standard output).
    path: String,
    /// Replace the file if it already exists.
    #[arg(long)]
    force: bool,
}

#[derive(Subcommand)]
enum DeviceCmd {
    /// List the devices, newest records included, and mark this one.
    List,
    /// Add a device: show an invite code, then approve the device that uses it.
    Add(DeviceAddArgs),
    /// Stop a device from opening anything written from now on.
    Revoke(RevokeArgs),
}

#[derive(Args)]
pub struct RevokeArgs {
    /// The device id (4 or more hex digits) from `figlock device list`.
    pub device: String,
    /// Also set a new password, in the same step. Use this if the device may have been compromised.
    #[arg(long)]
    pub change_password: bool,
    /// Also replace the Secret Key, in the same step. Use this if the device may have been compromised.
    #[arg(long)]
    pub rotate_secret_key: bool,
}

#[derive(Subcommand)]
enum QuickCmd {
    /// Turn it on. Asks for the password and Secret Key once.
    Enable,
    /// Turn it off and delete the kept key.
    Disable,
    /// Show whether it is on.
    Status,
}

#[derive(Args)]
struct SyncArgs {
    /// Upload everything from this device again, after the vault was lost from Google Drive.
    #[arg(long)]
    reupload: bool,
    #[command(subcommand)]
    action: Option<SyncCmd>,
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Turn sync to a place on and upload what is already on this device: `--drive` (sign in first with
    /// `figlock drive login`, and the default), `--icloud` or `--sync-folder <path>`. Several can be on.
    On(#[command(flatten)] RemoteArgs),
    /// Stop syncing to the places given, or to all of them. The vault on this device and the copies already
    /// stored there are left alone.
    Off(#[command(flatten)] RemoteArgs),
    /// Show every place this vault syncs to: whether it can be reached, when it last synced and what went wrong.
    Status,
    /// Permanently delete this vault's copy from Google Drive. The vault on this device is kept, and sync is
    /// turned off here.
    DeleteRemote {
        /// The confirmation phrase, `delete from Google Drive`, for scripts; at a terminal it is asked for.
        #[arg(long)]
        confirm: Option<String>,
    },
}

#[derive(Subcommand)]
enum DriveCmd {
    /// Sign in with your browser. FigLock only gets a hidden app folder, never your files.
    Login,
    /// Revoke FigLock's access and forget the sign-in on this device.
    Logout,
}

#[derive(Subcommand)]
enum SkillCmd {
    /// Install the skill where Claude Code finds it (`~/.claude/skills/figlock`).
    Install(SkillInstallArgs),
    /// Print the skill text.
    Show,
}

#[derive(Args)]
pub struct SkillInstallArgs {
    /// Install into this project's `.claude/skills` instead of your user folder.
    #[arg(long)]
    pub project: bool,
    /// Replace a different copy that is already installed.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct HttpArgs {
    /// The token item to use, by title. The request goes only to hosts on its allow-list.
    #[arg(long)]
    pub item: String,
    /// HTTP method (default GET, or POST when there is a body).
    #[arg(short = 'X', long = "request")]
    pub method: Option<String>,
    /// Add a header, `Name: value`. Repeatable. The secret's own header cannot be set.
    #[arg(short = 'H', long = "header", value_name = "NAME: VALUE", value_parser = parse_header)]
    pub headers: Vec<(String, String)>,
    /// Request body; `@-` reads it from standard input.
    #[arg(short = 'd', long = "data")]
    pub data: Option<String>,
    /// Header that carries the secret.
    #[arg(long, default_value = "Authorization")]
    pub auth_header: String,
    /// Word before the secret in that header; empty sends the bare value.
    #[arg(long, default_value = "Bearer")]
    pub auth_scheme: String,
    /// The item field that holds the secret.
    #[arg(long, default_value = "token")]
    pub field: String,
    /// How long the approval lasts.
    #[arg(long, value_enum, default_value = "15m")]
    pub ttl: GrantTtl,
    /// Why, shown to the person approving (unchecked text).
    #[arg(long)]
    pub reason: Option<String>,
    /// Print the status line and response headers before the body.
    #[arg(short = 'i', long)]
    pub include: bool,
    /// Exit with 22 when the server answers 400 or above.
    #[arg(long)]
    pub fail: bool,
    /// The address. https only (plain http only to this machine).
    pub url: String,
}

fn parse_header(s: &str) -> Result<(String, String), String> {
    let (name, value) = s.split_once(':').ok_or("expected 'Name: value'")?;
    if name.trim().is_empty() {
        return Err("expected 'Name: value'".into());
    }
    Ok((name.trim().to_owned(), value.trim().to_owned()))
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("what").required(true).args(["item", "bundle"])))]
pub struct RunArgs {
    /// The item to take secrets from, by title. A token item sets its own variable (`env_var`).
    #[arg(long)]
    pub item: Option<String>,
    /// An env bundle item, by title: every `env/NAME` field becomes the variable NAME.
    #[arg(long)]
    pub bundle: Option<String>,
    /// Map a variable to an item field, `NAME=field` (with --item). Repeatable.
    #[arg(long = "env", value_name = "NAME=FIELD", value_parser = parse_env, conflicts_with = "bundle")]
    pub env: Vec<(String, String)>,
    /// How long the approval lasts.
    #[arg(long, value_enum, default_value = "15m")]
    pub ttl: GrantTtl,
    /// Why, shown to the person approving (unchecked text).
    #[arg(long)]
    pub reason: Option<String>,
    /// The program and its arguments, after `--`.
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum GrantTtl {
    /// One use, then it must be approved again.
    #[value(name = "once")]
    Once,
    /// Fifteen minutes.
    #[value(name = "15m")]
    Min15,
    /// One day.
    #[value(name = "1d")]
    Day,
    /// Until `figlock lock` or `figlock broker lock`.
    #[value(name = "until-lock")]
    UntilLock,
}

impl GrantTtl {
    fn as_str(self) -> &'static str {
        match self {
            GrantTtl::Once => "once",
            GrantTtl::Min15 => "15m",
            GrantTtl::Day => "1d",
            GrantTtl::UntilLock => "until-lock",
        }
    }
}

fn parse_env(s: &str) -> Result<(String, String), String> {
    let (name, field) = s.split_once('=').ok_or("expected NAME=field")?;
    if name.is_empty() || field.is_empty() {
        return Err("expected NAME=field".into());
    }
    Ok((name.to_owned(), field.to_owned()))
}

#[derive(Subcommand)]
enum BrokerCmd {
    /// Unlock the vault and serve requests until `broker lock`. Runs in the foreground.
    Start {
        /// Never ask; deny every request. For tests and scripts that only need the socket.
        #[arg(long, hide = true)]
        deny_all: bool,
        /// Run for an agent with nobody at the keyboard: unlock only with quick unlock (never ask for a
        /// password), refuse to start when nothing could ask for approval, and lock after 30 minutes with no
        /// request and no live grant. `figlock http` and `figlock run` start it for you when it is needed.
        #[arg(long)]
        background: bool,
    },
    /// Show whether the broker is running, its grants and the audit log position.
    Status,
    /// List active grants (never their tokens).
    Grants,
    /// End a grant now.
    Revoke {
        /// The grant id shown by `broker grants`.
        grant: String,
    },
    /// Forget the vault and every grant, and stop the broker.
    Lock,
}

#[derive(Subcommand)]
enum RecoveryCmd {
    /// Say whether the vault has a Recovery Key. Needs no password.
    Status,
    /// Make a Recovery Key, show it once, and add it to the vault after you confirm you saved it.
    Enable(RecoveryArgs),
    /// Make a new Recovery Key. The old code opens nothing from then on.
    Replace(RecoveryArgs),
    /// Turn the Recovery Key off. A forgotten password can then no longer be reset.
    Disable {
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args)]
pub struct RecoveryArgs {
    /// You have stored the code: skip typing its last two groups back. For scripts.
    #[arg(long)]
    pub saved: bool,
    /// Also write the one page Recovery Key PDF to this path (owner-only; `-` is stdout).
    #[arg(long, value_name = "PATH")]
    pub kit: Option<String>,
    /// Overwrite the file given with --kit.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct ForgotArgs {
    /// Where the vault is, on a device that does not have it yet. A device that already has the vault uses its own.
    #[command(flatten)]
    pub at: WhereArgs,
    /// You have stored the new Secret Key and Recovery Key: skip typing their last two groups back. For scripts.
    #[arg(long)]
    pub saved: bool,
    /// Keep the Recovery Key you just used instead of making a new one (not recommended: it was just typed).
    #[arg(long)]
    pub keep_recovery_key: bool,
    /// Also write the new Emergency Kit PDF to this path (owner-only; `-` is stdout).
    #[arg(long, value_name = "PATH")]
    pub kit: Option<String>,
    /// Also write the new Recovery Key PDF to this path.
    #[arg(long, value_name = "PATH")]
    pub recovery_kit: Option<String>,
    /// Overwrite the files given with --kit and --recovery-kit.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct AuditArgs {
    /// Show only the last N records.
    #[arg(long)]
    pub tail: Option<usize>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum KdfProfile {
    Default,
    /// The lowest cost the spec allows. For tests and slow machines only.
    #[value(hide = true)]
    Floor,
}

/// Places a vault is synced to besides this device's own folder. Any number can be on at once (ADR-0043).
#[derive(Args, Default)]
pub struct RemoteArgs {
    /// Google Drive (run `figlock drive login` first).
    #[arg(long)]
    pub drive: bool,
    /// iCloud Drive: the `FigLock` folder in it. Macs with iCloud Drive on only; there is no sign-in step.
    #[arg(long)]
    pub icloud: bool,
    /// Any other synced folder (Dropbox, Syncthing), as a full path. Can be given more than once.
    #[arg(long = "sync-folder", value_name = "PATH")]
    pub sync_folder: Vec<PathBuf>,
}

/// Where an existing vault is, for `join` and `open`.
#[derive(Args)]
pub struct WhereArgs {
    /// Folder that holds the encrypted vault (a synced folder works). Defaults to a folder in the FigLock
    /// config directory, which is where a Drive copy is downloaded.
    #[arg(long)]
    pub store: Option<PathBuf>,
    /// Where the vault is, when not in a folder: `--drive`, `--icloud` or `--sync-folder`. This device keeps
    /// its own copy in `--store` (or the default folder) and keeps it synced.
    #[command(flatten)]
    pub remotes: RemoteArgs,
    /// Seconds to wait for a synced folder (iCloud) to finish delivering the vault before giving up.
    #[arg(long, default_value_t = 120, value_name = "SECS")]
    pub wait_for_sync: u64,
    /// Name shown for this device.
    #[arg(long, default_value = "cli")]
    pub device_name: String,
    /// Turn on quick unlock (Touch ID) for this device without asking.
    #[arg(long, conflicts_with = "no_quick_unlock")]
    pub quick_unlock: bool,
    /// Do not turn on quick unlock and do not ask.
    #[arg(long)]
    pub no_quick_unlock: bool,
}

#[derive(Args)]
pub struct JoinArgs {
    /// The invite code shown on the device that already has the vault.
    pub code: String,
    #[command(flatten)]
    pub at: WhereArgs,
    /// Seconds to wait for the other device to approve before giving up.
    #[arg(long, default_value_t = 600)]
    pub timeout: u64,
}

#[derive(Args)]
pub struct OpenArgs {
    #[command(flatten)]
    pub at: WhereArgs,
}

#[derive(Args)]
pub struct DeviceAddArgs {
    /// Approve the device that answers without asking, after you have checked its fingerprint yourself.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args)]
pub struct InitArgs {
    /// Folder that will hold the encrypted vault (a synced folder works). Defaults to a folder in the
    /// FigLock config directory.
    #[arg(long)]
    pub store: Option<PathBuf>,
    /// Also keep synced copies: `--drive`, `--icloud`, `--sync-folder <path>`; any number of them.
    #[command(flatten)]
    pub remotes: RemoteArgs,
    /// Name shown for this device.
    #[arg(long, default_value = "cli")]
    pub device_name: String,

    /// Make a Recovery Key now, so a forgotten password can be reset (`figlock forgot`). Without this or
    /// --no-recovery-key, a terminal is asked.
    #[arg(long, conflicts_with = "no_recovery_key")]
    pub recovery_key: bool,
    /// Do not make a Recovery Key: a forgotten password then cannot be recovered. Do not ask.
    #[arg(long)]
    pub no_recovery_key: bool,
    #[arg(long, value_enum, default_value = "default", hide = true)]
    pub kdf: KdfProfile,
    /// Turn on quick unlock (Touch ID) for this device without asking.
    #[arg(long, conflicts_with = "no_quick_unlock")]
    pub quick_unlock: bool,
    /// Do not turn on quick unlock and do not ask.
    #[arg(long)]
    pub no_quick_unlock: bool,
}

fn parse_type(s: &str) -> Result<ItemType, String> {
    ItemType::parse(s).map_err(|_| "expected login, token, note, env, ssh or doc".to_owned())
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .ok_or_else(|| "expected field=value".to_owned())
}

const ADD_HELP: &str = "\
Fields by item type (a trailing * marks a secret: give it with --secret, never --set):
  login   username, password*, totp*, url/<id>
  token   provider, token*, env_var, scopes, expires_at, allowed_hosts/<id>
  note    body
  env     env/<NAME>*          one per variable, for example env/DATABASE_URL
  ssh     private_key*, public_key, passphrase*
  doc     password*, expires_at, url/<id>; attach files with --file
Every type also takes: notes, tags/<tag>, custom/<id>/name and custom/<id>/value*.
<id> is any short lowercase name you pick, such as 1 or work.

Examples:
  figlock add -t login --title GitHub --set username=ada --set url/1=https://github.com --secret password
  figlock add -t token --title \"API key\" --set provider=example --set env_var=EXAMPLE_TOKEN --set allowed_hosts/1=api.example.com --secret token
  figlock add -t doc --title \"Apple signing\" --file cert.p12 --secret password

Limits: each field has a size cap (a title 1 KiB, a password 4 KiB, notes 256 KiB); a file is up to 768 KiB, eight per item.";

#[derive(Args)]
#[command(after_help = ADD_HELP)]
pub struct AddArgs {
    /// login, token, note, env, ssh or doc.
    #[arg(long = "type", short = 't', value_parser = parse_type)]
    pub item_type: ItemType,
    /// What the item is called. It is what `find` searches first.
    #[arg(long)]
    pub title: String,
    /// Attach a file (doc items only, up to 768 KiB). Repeat for several files.
    #[arg(long = "file", value_name = "PATH")]
    pub files: Vec<PathBuf>,
    /// A non-secret field, `name=value` (for example `username=ada`, `url/1=https://example.com`).
    #[arg(long = "set", value_name = "FIELD=VALUE", value_parser = parse_kv)]
    pub set: Vec<(String, String)>,
    /// A secret field (`password`, `token`, ...). The value is prompted, or read from stdin.
    #[arg(long = "secret", value_name = "FIELD")]
    pub secret: Vec<String>,
}

#[derive(Args)]
pub struct EditArgs {
    /// Item id, unique id prefix, or a search that finds one item.
    pub item: String,
    /// Set a non-secret field, `name=value`. Replaces the current value.
    #[arg(long = "set", value_name = "FIELD=VALUE", value_parser = parse_kv)]
    pub set: Vec<(String, String)>,
    /// Set a secret field. The new value is prompted, or read from stdin.
    #[arg(long = "secret", value_name = "FIELD")]
    pub secret: Vec<String>,
    /// Remove a field.
    #[arg(long = "unset", value_name = "FIELD")]
    pub unset: Vec<String>,
    /// Attach a file (doc items only, up to 768 KiB). Repeat for several files.
    #[arg(long = "file", value_name = "PATH")]
    pub files: Vec<PathBuf>,
    /// Remove an attached file, by name or id.
    #[arg(long = "remove-file", value_name = "NAME")]
    pub remove_files: Vec<String>,
    /// Move the item to the trash.
    #[arg(long, conflicts_with = "restore")]
    pub delete: bool,
    /// Bring a trashed item back.
    #[arg(long)]
    pub restore: bool,
}

#[derive(Args)]
pub struct FindArgs {
    /// Words to look for in titles, usernames, URLs, tags and file names. Secrets are never searched.
    pub query: Vec<String>,
    /// Only items of this type: login, token, note, env, ssh or doc.
    #[arg(long = "type", short = 't', value_parser = parse_type)]
    pub item_type: Option<ItemType>,
    /// Show at most this many matches.
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Look in the trash instead: list deleted items and when they were deleted. Restore one with
    /// `figlock edit <id or words> --restore`.
    #[arg(long)]
    pub deleted: bool,
}

#[derive(Args)]
pub struct GetArgs {
    /// Item id, unique id prefix, or a search that finds one item.
    pub item: String,
    /// Print only this field's value.
    #[arg(long, short = 'f', conflicts_with = "file")]
    pub field: Option<String>,
    /// Save an attached file (by name or id) to --out.
    #[arg(long = "file", value_name = "NAME", requires = "out")]
    pub file: Option<String>,
    /// Where to write the file: a new path (created owner-only) or `-` for stdout.
    #[arg(long, value_name = "PATH", requires = "file")]
    pub out: Option<String>,
    /// Replace --out if it already exists.
    #[arg(long, requires = "out")]
    pub force: bool,
    /// Show secret values.
    #[arg(long)]
    pub reveal: bool,
}

#[derive(Args)]
pub struct CopyArgs {
    /// Item id, unique id prefix, or a search that finds one item.
    pub item: String,
    /// Field to copy; defaults to the item's main secret.
    #[arg(long, short = 'f')]
    pub field: Option<String>,
    /// Seconds before the clipboard is cleared (0 keeps it).
    #[arg(long, default_value_t = 30)]
    pub clear_after: u64,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// How many characters.
    #[arg(long, short = 'l', default_value_t = 20, conflicts_with = "passphrase")]
    pub length: usize,
    /// Leave out lowercase letters.
    #[arg(long, conflicts_with = "passphrase")]
    pub no_lower: bool,
    /// Leave out uppercase letters.
    #[arg(long, conflicts_with = "passphrase")]
    pub no_upper: bool,
    /// Leave out digits.
    #[arg(long, conflicts_with = "passphrase")]
    pub no_digits: bool,
    /// Leave out symbols (letters and digits only).
    #[arg(long, conflicts_with = "passphrase")]
    pub no_symbols: bool,
    /// Make a passphrase of random words instead of a password.
    #[arg(long, short = 'p')]
    pub passphrase: bool,
    /// How many words in the passphrase.
    #[arg(long, default_value_t = 6, requires = "passphrase")]
    pub words: usize,
    /// What goes between the words.
    #[arg(long, value_enum, default_value_t = SeparatorArg::Hyphen, requires = "passphrase")]
    pub separator: SeparatorArg,
    /// Capitalize every word.
    #[arg(long, requires = "passphrase")]
    pub capitalize: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum SeparatorArg {
    Hyphen,
    Dot,
    Space,
    None,
}

#[derive(Args)]
pub struct ExportArgs {
    /// Write secret values too. Without it they are left out.
    #[arg(long)]
    pub include_secrets: bool,
    /// Include items in the trash.
    #[arg(long)]
    pub include_deleted: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ImportSource {
    /// Chrome's password export (CSV).
    Chrome,
    /// Apple Passwords export (CSV).
    Apple,
    /// Bitwarden export (CSV).
    Bitwarden,
    /// 1Password export (CSV).
    #[value(name = "1password")]
    OnePassword,
    /// A plain text dump of notes; notes are split at lines of `---` or `===`.
    Notes,
}

impl ImportSource {
    pub fn source(self) -> Source {
        match self {
            ImportSource::Chrome => Source::Chrome,
            ImportSource::Apple => Source::Apple,
            ImportSource::Bitwarden => Source::Bitwarden,
            ImportSource::OnePassword => Source::OnePassword,
            ImportSource::Notes => Source::Notes,
        }
    }
}

#[derive(Args)]
pub struct ImportArgs {
    /// The export file (CSV) or notes dump (text).
    pub file: PathBuf,
    /// Which program the file came from.
    #[arg(long, value_enum)]
    pub from: ImportSource,
    /// Show what would be imported without asking for the password or writing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[cfg(unix)]
fn broker_command(action: BrokerCmd, json: bool) -> error::Res<()> {
    match action {
        BrokerCmd::Start {
            deny_all,
            background,
        } => broker::cli::start(deny_all, background),
        BrokerCmd::Status => broker::cli::status(json),
        BrokerCmd::Grants => broker::cli::grants(json),
        BrokerCmd::Revoke { grant } => broker::cli::revoke(&grant, json),
        BrokerCmd::Lock => broker::cli::lock(json),
    }
}

#[cfg(unix)]
fn audit_command(a: &AuditArgs, json: bool) -> error::Res<()> {
    broker::cli::show_audit(a.tail, json)
}

#[cfg(not(unix))]
fn broker_command(_: BrokerCmd, _: bool) -> error::Res<()> {
    Err(error::Fail::new(
        "the broker is not available on this system yet",
    ))
}

#[cfg(not(unix))]
fn audit_command(_: &AuditArgs, _: bool) -> error::Res<()> {
    Err(error::Fail::new(
        "the broker is not available on this system yet",
    ))
}

#[cfg(unix)]
fn run_command(a: RunArgs) -> error::Res<i32> {
    figlock_broker::run_cmd::run(&figlock_broker::run_cmd::RunSpec {
        item: a.item.or(a.bundle).unwrap_or_default(),
        vars: a.env,
        ttl: a.ttl.as_str().to_owned(),
        reason: a.reason,
        command: a.command,
    })
}

#[cfg(unix)]
fn http_command(a: HttpArgs) -> error::Res<i32> {
    let body = a
        .data
        .as_deref()
        .map(figlock_broker::http_cmd::read_body)
        .transpose()?;
    let method = a
        .method
        .unwrap_or_else(|| if body.is_some() { "POST" } else { "GET" }.to_owned())
        .to_ascii_uppercase();
    figlock_broker::http_cmd::run(
        &figlock_broker::http_cmd::HttpSpec {
            item: a.item,
            method,
            url: a.url,
            headers: a.headers,
            body,
            ttl: a.ttl.as_str().to_owned(),
            reason: a.reason,
            auth_header: Some(a.auth_header),
            auth_scheme: Some(a.auth_scheme),
            field: Some(a.field),
        },
        a.include,
        a.fail,
    )
}

#[cfg(not(unix))]
fn http_command(_: HttpArgs) -> error::Res<i32> {
    Err(error::Fail::new(
        "the broker is not available on this system yet",
    ))
}

#[cfg(not(unix))]
fn run_command(_: RunArgs) -> error::Res<i32> {
    Err(error::Fail::new(
        "the broker is not available on this system yet",
    ))
}

fn quick_command(action: &QuickCmd, json: bool) -> error::Res<()> {
    match action {
        QuickCmd::Enable => quick::enable(json),
        QuickCmd::Disable => quick::disable(json),
        QuickCmd::Status => quick::status(json),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    let ok = |r: error::Res<()>| r.map(|()| 0);
    let result = match cli.command {
        Command::Init(a) => ok(commands::init(a, json)),
        Command::Join(a) => ok(pairing::join(a, json)),
        Command::Open(a) => ok(pairing::open(a, json)),
        Command::Add(a) => ok(commands::add(a, json)),
        Command::Edit(a) => ok(commands::edit(a, json)),
        Command::Find(a) => ok(commands::find(a, json)),
        Command::Get(a) => ok(commands::get(a, json)),
        Command::Copy(a) => ok(commands::copy(a, json)),
        Command::Generate(a) => ok(commands::generate(a, json)),
        Command::Export(a) => ok(commands::export(a)),
        Command::Import(a) => ok(import::import(a, json)),
        Command::Broker { action } => ok(broker_command(action, json)),
        Command::Audit(a) => ok(audit_command(&a, json)),
        // These end with the exit code of the program or the request, not just success or failure.
        Command::Run(a) => run_command(a),
        Command::Http(a) => http_command(a),
        Command::QuickUnlock { action } => ok(quick_command(&action, json)),
        Command::RecoveryKey { action } => ok(match action {
            RecoveryCmd::Status => recovery::status(json),
            RecoveryCmd::Enable(a) => recovery::set(&a, false, json),
            RecoveryCmd::Replace(a) => recovery::set(&a, true, json),
            RecoveryCmd::Disable { yes } => recovery::disable(yes, json),
        }),
        Command::Forgot(a) => ok(recovery::forgot(a, json)),
        Command::Password { action } => ok(match action {
            PasswordCmd::Change => credentials::password_change(json),
        }),
        Command::SecretKey { action } => ok(match action {
            SecretKeyCmd::Rotate => credentials::secret_key_rotate(json),
            SecretKeyCmd::Show => credentials::secret_key_show(json),
            SecretKeyCmd::Kit(a) => credentials::secret_key_kit(&a.path, a.force, json),
            SecretKeyCmd::Forget => credentials::secret_key_forget(json),
            SecretKeyCmd::Keep => credentials::secret_key_keep(json),
        }),
        Command::Device { action } => ok(match action {
            DeviceCmd::List => credentials::device_list(json),
            DeviceCmd::Add(a) => pairing::device_add(&a, json),
            DeviceCmd::Revoke(a) => {
                credentials::device_revoke(&a.device, a.change_password, a.rotate_secret_key, json)
            }
        }),
        Command::Lock => ok(quick::lock(json)),
        Command::Drive { action } => ok(match action {
            DriveCmd::Login => drive::login(json),
            DriveCmd::Logout => drive::logout(json),
        }),
        Command::Sync(a) if a.reupload && a.action.is_some() => ok(Err(
            figlock_session::error::Fail::new("--reupload only applies to `figlock sync` itself"),
        )),
        Command::Sync(a) => ok(match a.action {
            None => sync_cmd::run(a.reupload, json),
            Some(SyncCmd::On(r)) => sync_cmd::on(&r, json),
            Some(SyncCmd::Off(r)) => sync_cmd::off(&r, json),
            Some(SyncCmd::Status) => sync_cmd::status(json),
            Some(SyncCmd::DeleteRemote { confirm }) => sync_cmd::delete_remote(confirm, json),
        }),
        Command::Skill { action } => ok(match action {
            SkillCmd::Install(a) => skill::install(&a, json),
            SkillCmd::Show => {
                skill::show();
                Ok(())
            }
        }),
    };
    match result {
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(e) => {
            eprintln!("error: {e}");
            if e.1 == figlock_session::error::FailKind::Unavailable {
                // `EX_UNAVAILABLE`: nothing could answer, and only a person can change that.
                ExitCode::from(69)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod help_tests {
    use clap::{Command, CommandFactory};

    use super::Cli;

    /// Every command and every argument a person can type has help text, and none of it uses an em dash.
    fn walk(cmd: &Command, path: &str, problems: &mut Vec<String>) {
        let here = format!("{path} {}", cmd.get_name());
        if cmd.get_name() != "help" {
            if cmd.get_about().is_none() && !path.is_empty() {
                problems.push(format!("{here}: no description"));
            }
            for arg in cmd.get_arguments() {
                let id = arg.get_id().as_str();
                if matches!(id, "help" | "version") || arg.is_hide_set() {
                    continue;
                }
                if arg.get_help().is_none() && arg.get_long_help().is_none() {
                    problems.push(format!("{here}: argument {id} has no help"));
                }
            }
        }
        for sub in cmd.get_subcommands() {
            walk(sub, &here, problems);
        }
    }

    #[test]
    fn every_command_and_argument_explains_itself() {
        let mut problems = Vec::new();
        walk(&Cli::command(), "", &mut problems);
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn rendered_help_never_uses_an_em_dash() {
        fn render(mut cmd: Command, out: &mut String) {
            out.push_str(&cmd.render_long_help().to_string());
            for sub in cmd.get_subcommands().cloned().collect::<Vec<_>>() {
                render(sub, out);
            }
        }
        let mut all = String::new();
        render(Cli::command(), &mut all);
        assert!(!all.contains('\u{2014}'), "em dash in help text");
    }
}
