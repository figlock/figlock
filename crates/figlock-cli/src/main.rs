//! The `figlock` command line: create a vault in a folder you own and add, find, read, copy and export items.
//!
//! Secrets are never taken from arguments: master password, Secret Key and secret field values come from a
//! hidden prompt (or stdin for field values). `get` and `export` hide secret values unless asked.

#[cfg(unix)]
mod b64;
#[cfg(unix)]
mod broker;
mod clipboard;
mod commands;
mod config;
mod drive;
mod error;
mod generate;
#[cfg(unix)]
mod http;
mod import;
mod prompt;
mod quick;
mod render;
#[cfg(unix)]
mod run;
#[cfg(unix)]
mod scrub;
mod session;
mod skill;
mod store;

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
    /// Run a program with secrets in its environment, after the broker's approval. Its output is scrubbed.
    Run(RunArgs),
    /// Make an HTTP request through the broker, which adds the secret itself. You never see the secret.
    Http(HttpArgs),
    /// Unlock with Touch ID or the device password instead of the master password and Secret Key.
    QuickUnlock {
        #[command(subcommand)]
        action: QuickCmd,
    },
    /// Lock the vault: the next command asks for the full unlock (or Touch ID again if quick unlock is on).
    Lock,
    /// Sign this device in to Google Drive, where a vault can be stored.
    Drive {
        #[command(subcommand)]
        action: DriveCmd,
    },
    /// The Claude skill that teaches agents to use secrets through the broker.
    Skill {
        #[command(subcommand)]
        action: SkillCmd,
    },
}

#[derive(Subcommand)]
enum QuickCmd {
    /// Turn it on. Asks for the master password and Secret Key once.
    Enable,
    /// Turn it off and delete the kept key.
    Disable,
    /// Show whether it is on.
    Status,
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
    #[value(name = "once")]
    Once,
    #[value(name = "15m")]
    Min15,
    #[value(name = "1d")]
    Day,
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
    #[arg(long, conflicts_with = "drive", required_unless_present = "drive")]
    pub store: Option<PathBuf>,
    /// Keep the encrypted vault in your Google Drive instead (run `figlock drive login` first).
    #[arg(long)]
    pub drive: bool,
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

#[cfg(unix)]
fn run_command(a: RunArgs) -> error::Res<i32> {
    run::run(&run::RunSpec {
        item: a.item.or(a.bundle).unwrap_or_default(),
        vars: a.env,
        ttl: a.ttl.as_str().to_owned(),
        reason: a.reason,
        command: a.command,
    })
}

#[cfg(unix)]
fn http_command(a: HttpArgs) -> error::Res<i32> {
    let body = a.data.as_deref().map(http::read_body).transpose()?;
    let method = a
        .method
        .unwrap_or_else(|| if body.is_some() { "POST" } else { "GET" }.to_owned())
        .to_ascii_uppercase();
    http::run(
        &http::HttpSpec {
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
        Command::Lock => ok(quick::lock(json)),
        Command::Drive { action } => ok(match action {
            DriveCmd::Login => drive::login(json),
            DriveCmd::Logout => drive::logout(json),
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
            ExitCode::FAILURE
        }
    }
}
