//! The `figlock` command line: create a vault in a folder you own and add, find, read, copy and export items.
//!
//! Secrets are never taken from arguments: master password, Secret Key and secret field values come from a
//! hidden prompt (or stdin for field values). `get` and `export` hide secret values unless asked.

#[cfg(unix)]
mod broker;
mod clipboard;
mod commands;
mod config;
mod error;
mod generate;
mod import;
mod prompt;
mod render;
mod session;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use figlock_core::{ItemType, Source};

#[derive(Parser)]
#[command(
    name = "figlock",
    version,
    about = "Headless, agent-aware password manager"
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
}

#[derive(Subcommand)]
enum BrokerCmd {
    /// Unlock the vault and serve requests until `broker lock`. Runs in the foreground.
    Start {
        /// Never ask; deny every request. For tests and scripts that only need the socket.
        #[arg(long, hide = true)]
        deny_all: bool,
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

#[derive(Args)]
pub struct InitArgs {
    /// Folder that will hold the encrypted vault (a synced folder works).
    #[arg(long)]
    pub store: PathBuf,
    /// Name shown for this device.
    #[arg(long, default_value = "cli")]
    pub device_name: String,
    #[arg(long, value_enum, default_value = "default", hide = true)]
    pub kdf: KdfProfile,
}

fn parse_type(s: &str) -> Result<ItemType, String> {
    ItemType::parse(s).map_err(|_| "expected login, token, note, env or ssh".to_owned())
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .ok_or_else(|| "expected field=value".to_owned())
}

#[derive(Args)]
pub struct AddArgs {
    /// login, token, note, env or ssh.
    #[arg(long = "type", short = 't', value_parser = parse_type)]
    pub item_type: ItemType,
    #[arg(long)]
    pub title: String,
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
    #[arg(long = "set", value_name = "FIELD=VALUE", value_parser = parse_kv)]
    pub set: Vec<(String, String)>,
    #[arg(long = "secret", value_name = "FIELD")]
    pub secret: Vec<String>,
    /// Remove a field.
    #[arg(long = "unset", value_name = "FIELD")]
    pub unset: Vec<String>,
    /// Move the item to the trash.
    #[arg(long, conflicts_with = "restore")]
    pub delete: bool,
    /// Bring a trashed item back.
    #[arg(long)]
    pub restore: bool,
}

#[derive(Args)]
pub struct FindArgs {
    pub query: Vec<String>,
    #[arg(long = "type", short = 't', value_parser = parse_type)]
    pub item_type: Option<ItemType>,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args)]
pub struct GetArgs {
    pub item: String,
    /// Print only this field's value.
    #[arg(long, short = 'f')]
    pub field: Option<String>,
    /// Show secret values.
    #[arg(long)]
    pub reveal: bool,
}

#[derive(Args)]
pub struct CopyArgs {
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
    #[arg(long, short = 'l', default_value_t = 20)]
    pub length: usize,
    /// Letters and digits only.
    #[arg(long)]
    pub no_symbols: bool,
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
    Chrome,
    Apple,
    Bitwarden,
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
    #[arg(long, value_enum)]
    pub from: ImportSource,
    /// Show what would be imported without asking for the password or writing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[cfg(unix)]
fn broker_command(action: BrokerCmd, json: bool) -> error::Res<()> {
    match action {
        BrokerCmd::Start { deny_all } => broker::cli::start(deny_all),
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    let result = match cli.command {
        Command::Init(a) => commands::init(a, json),
        Command::Add(a) => commands::add(a, json),
        Command::Edit(a) => commands::edit(a, json),
        Command::Find(a) => commands::find(a, json),
        Command::Get(a) => commands::get(a, json),
        Command::Copy(a) => commands::copy(a, json),
        Command::Generate(a) => commands::generate(a, json),
        Command::Export(a) => commands::export(a),
        Command::Import(a) => import::import(a, json),
        Command::Broker { action } => broker_command(action, json),
        Command::Audit(a) => audit_command(&a, json),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
