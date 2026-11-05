use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use figlock_core::{ItemType, Vault, is_secret_field};
use zeroize::Zeroizing;

use crate::device::DeviceKeysHandle;
use crate::error::{FigLockError, Result, id_hex, parse_id};

/// What a listing shows of one item. Secret fields are named in `secret_fields` but their values stay in
/// the vault; read one with `VaultSession::reveal_field`.
#[derive(uniffi::Record)]
pub struct ItemSummary {
    /// 32 hex characters.
    pub id: String,
    /// `login`, `token`, `note`, `env` or `ssh`.
    pub item_type: String,
    pub vault: String,
    pub deleted: bool,
    /// Last update, milliseconds since the Unix epoch (hybrid logical clock wall time).
    pub updated_ms: u64,
    /// Non-secret fields only: title, username, the url fields, tags and so on.
    pub fields: HashMap<String, String>,
    /// Names of the secret fields this item has, so the UI can offer reveal and copy.
    pub secret_fields: Vec<String>,
}

#[derive(uniffi::Record)]
pub struct ItemListing {
    pub items: Vec<ItemSummary>,
    /// Ids of items that failed to decrypt. Shown as "damaged", never silently dropped.
    pub corrupt_ids: Vec<String>,
}

/// One field to set when creating an item. The value of a secret field crosses the boundary here, once.
#[derive(uniffi::Record)]
pub struct FieldInput {
    pub name: String,
    pub value: String,
}

#[derive(uniffi::Record)]
pub struct SearchHit {
    pub id: String,
    pub score: u32,
}

/// A file the caller must store: `path` is relative to the storage root.
#[derive(uniffi::Record)]
pub struct CommittedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

struct State {
    vault: Option<Vault>,
    /// Search index over non-secret fields; dropped whenever items change and rebuilt on the next search.
    index: Option<figlock_core::Index>,
}

/// An unlocked vault. All keys and plaintext stay behind this handle; locking drops them and every later
/// call fails with `Locked`. Methods are safe to call from any thread (calls take turns).
#[derive(uniffi::Object)]
pub struct VaultSession {
    state: Mutex<State>,
}

impl VaultSession {
    pub(crate) fn new(vault: Vault) -> Self {
        VaultSession {
            state: Mutex::new(State {
                vault: Some(vault),
                index: None,
            }),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, State>> {
        // A poisoned lock means an earlier call panicked mid-update; refuse rather than use that state.
        self.state.lock().map_err(|_| FigLockError::Failed)
    }

    fn with_vault<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T>) -> Result<T> {
        let mut st = self.state()?;
        let vault = st.vault.as_mut().ok_or(FigLockError::Locked)?;
        f(vault)
    }

    /// Like `with_vault`, for calls that change items: the search index is dropped afterwards.
    fn mutate<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T>) -> Result<T> {
        let mut st = self.state()?;
        st.index = None;
        let vault = st.vault.as_mut().ok_or(FigLockError::Locked)?;
        f(vault)
    }
}

#[uniffi::export]
impl VaultSession {
    /// Drop every key and plaintext. Idempotent; the handle is unusable afterwards.
    pub fn lock(&self) {
        // Recover a poisoned lock on purpose: locking must always succeed in clearing the keys.
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.index = None;
        if let Some(vault) = st.vault.take() {
            vault.lock();
        }
    }

    pub fn is_locked(&self) -> bool {
        self.state.lock().map(|s| s.vault.is_none()).unwrap_or(true)
    }

    pub fn vault_id(&self) -> Result<String> {
        self.with_vault(|v| Ok(id_hex(v.vault_id())))
    }

    pub fn device_id(&self) -> Result<String> {
        self.with_vault(|v| Ok(id_hex(v.device_id())))
    }

    pub fn epoch(&self) -> Result<u32> {
        self.with_vault(|v| Ok(v.epoch()))
    }

    /// Number of op files applied or committed so far; changes whenever the vault content changes.
    pub fn revision(&self) -> Result<u64> {
        self.with_vault(|v| Ok(v.revision()))
    }

    /// True when there are edits not yet written out by `commit`.
    pub fn has_pending(&self) -> Result<bool> {
        self.with_vault(|v| Ok(v.has_pending()))
    }

    /// List items. Secret values are not included.
    pub fn items(&self, include_deleted: bool) -> Result<ItemListing> {
        self.with_vault(|v| {
            let (views, corrupt) = v.items(include_deleted);
            let items = views
                .iter()
                .map(|view| {
                    let mut fields = HashMap::new();
                    let mut secret_fields = Vec::new();
                    for (name, value) in &view.fields {
                        if is_secret_field(name) {
                            secret_fields.push(name.clone());
                        } else {
                            fields.insert(name.clone(), value.to_string());
                        }
                    }
                    ItemSummary {
                        id: id_hex(&view.id),
                        item_type: view.item_type.as_str().to_owned(),
                        vault: view.vault.clone(),
                        deleted: view.deleted,
                        updated_ms: view.updated.wall,
                        fields,
                        secret_fields,
                    }
                })
                .collect();
            Ok(ItemListing {
                items,
                corrupt_ids: corrupt.iter().map(id_hex).collect(),
            })
        })
    }

    /// Read one secret (or any) field value. This is the only call that returns a stored secret; the
    /// caller must have the user's approval and should wipe the string after use.
    pub fn reveal_field(&self, item_id: String, field: String) -> Result<String> {
        let id = parse_id(&item_id)?;
        self.with_vault(|v| {
            let view = v.item(&id)?;
            let value = view.fields.get(&field).ok_or(FigLockError::Failed)?;
            Ok(value.to_string())
        })
    }

    /// Create an item. `item_type` is `login`, `token`, `note`, `env` or `ssh`. Returns the new id.
    pub fn add_item(
        &self,
        item_type: String,
        vault: String,
        fields: Vec<FieldInput>,
        now_ms: u64,
    ) -> Result<String> {
        let t = ItemType::parse(&item_type).map_err(|_| FigLockError::InvalidInput)?;
        // The values may be secrets: hold them in zeroizing buffers until the vault has sealed them.
        let fields: Vec<(String, Zeroizing<String>)> = fields
            .into_iter()
            .map(|f| (f.name, Zeroizing::new(f.value)))
            .collect();
        let pairs: Vec<(&str, &str)> = fields
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        self.mutate(|v| Ok(id_hex(&v.add_item(t, &vault, &pairs, now_ms)?)))
    }

    /// Set a field, or remove it with `None`.
    pub fn set_field(
        &self,
        item_id: String,
        field: String,
        value: Option<String>,
        now_ms: u64,
    ) -> Result<()> {
        let id = parse_id(&item_id)?;
        let value = value.map(Zeroizing::new);
        self.mutate(|v| Ok(v.set_field(&id, &field, value.as_ref().map(|s| s.as_str()), now_ms)?))
    }

    pub fn delete_item(&self, item_id: String, now_ms: u64) -> Result<()> {
        let id = parse_id(&item_id)?;
        self.mutate(|v| Ok(v.delete_item(&id, now_ms)?))
    }

    pub fn restore_item(&self, item_id: String, now_ms: u64) -> Result<()> {
        let id = parse_id(&item_id)?;
        self.mutate(|v| Ok(v.restore_item(&id, now_ms)?))
    }

    /// Turn pending edits into one op file for the caller to store. `None` when there is nothing to write.
    pub fn commit(&self) -> Result<Option<CommittedFile>> {
        self.with_vault(|v| {
            Ok(v.commit()?.map(|c| CommittedFile {
                path: c.path,
                bytes: c.bytes,
            }))
        })
    }

    /// Apply an op file read from storage. `device_id` and `seq` come from its path
    /// (`log/<device>/<seq>.op`); files must arrive in order per device.
    pub fn apply_file(
        &self,
        bytes: Vec<u8>,
        device_id: String,
        seq: u64,
        now_ms: u64,
    ) -> Result<()> {
        let device = parse_id(&device_id)?;
        self.mutate(|v| Ok(v.apply_file(&bytes, &device, seq, now_ms)?))
    }

    /// Add a device record read from `devices/` in storage.
    pub fn add_device_record(&self, bytes: Vec<u8>) -> Result<()> {
        self.mutate(|v| Ok(v.add_device_record(&bytes)?))
    }

    /// Search non-secret fields: typo-tolerant, domain-aware, with token prefixes. Secret values are never
    /// searchable. An empty query lists the most recent items.
    pub fn search(&self, query: String, now_ms: u64, limit: u32) -> Result<Vec<SearchHit>> {
        let mut st = self.state()?;
        let State { vault, index } = &mut *st;
        let vault = vault.as_mut().ok_or(FigLockError::Locked)?;
        if index.is_none() {
            let (views, _) = vault.items(false);
            *index = Some(figlock_core::Index::build(views.iter()));
        }
        let idx = index.as_ref().ok_or(FigLockError::Failed)?;
        Ok(idx
            .search(&query, now_ms, limit as usize)
            .into_iter()
            .map(|h| SearchHit {
                id: id_hex(&h.id),
                score: h.score,
            })
            .collect())
    }
}

/// Everything `unlock` returns.
#[derive(uniffi::Record)]
pub struct UnlockOutcome {
    pub session: Arc<VaultSession>,
    /// Set when this device had no record yet: store it at `devices/<device id>.json`.
    pub new_device_record: Option<Vec<u8>>,
}

/// Unlock with the master password and Secret Key text (`FL1-...`). `vault_json` and `device_records` are the
/// stored files. A wrong password, a wrong Secret Key and a tampered file all give the same `Failed`.
#[uniffi::export]
pub fn unlock_vault(
    vault_json: Vec<u8>,
    password: String,
    secret_key: String,
    device: Arc<DeviceKeysHandle>,
    device_name: String,
    device_records: Vec<Vec<u8>>,
    now_ms: u64,
) -> Result<UnlockOutcome> {
    let password = Zeroizing::new(password);
    let secret_key = Zeroizing::new(secret_key);
    let key =
        figlock_core::SecretKey::parse(&secret_key).map_err(|_| FigLockError::InvalidInput)?;
    let (vault, record) = Vault::unlock(
        &vault_json,
        &password,
        &key,
        device.to_core(),
        &device_name,
        &device_records,
        now_ms,
    )?;
    Ok(UnlockOutcome {
        session: Arc::new(VaultSession::new(vault)),
        new_device_record: record,
    })
}
