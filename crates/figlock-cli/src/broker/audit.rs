//! The broker's audit log file (`docs/spec/broker.md` 5): append-only, written before the effect it
//! describes, and never extended when it cannot be verified.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use figlock_core::{Vault, audit_frame, audit_split};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use figlock_session::config;
use figlock_session::error::{Fail, Res};

/// Where records go. A file in production; tests swap in one that fails.
pub trait Sink: Send {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn truncate(&mut self, len: u64) -> io::Result<()>;
}

struct FileSink(File);

impl Sink for FileSink {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)?;
        self.0.sync_data()
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }
}

pub struct AuditWriter {
    sink: Box<dyn Sink>,
    len: u64,
    seq: u64,
    prev: [u8; 32],
    /// Set when a failed write could not be undone: nothing more is ever written.
    broken: bool,
}

pub fn path_for(vault: &Vault) -> Res<PathBuf> {
    Ok(config::home()?
        .join("audit")
        .join(format!("{}.log", hex::encode(vault.vault_id()))))
}

impl AuditWriter {
    /// Open (creating it private) and verify the whole log. A damaged final record, such as one cut short
    /// by a crash, is dropped; any whole record that fails verification stops the broker from starting.
    pub fn open(path: &Path, vault: &Vault) -> Res<AuditWriter> {
        if let Some(dir) = path.parent() {
            config::private_dir(dir)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .mode(0o600)
            .open(path)?;
        // The file is figlock's own; tighten it even when it already existed.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let (chain, _) = vault.read_audit_log(&bytes);
        if let Some(seq) = chain.first_bad {
            return Err(Fail::new(format!(
                "the audit log failed verification at record {seq}; move it aside to start a new one, then run again: {}",
                path.display()
            )));
        }
        let used = audit_split(&bytes).1;
        let mut sink = FileSink(file);
        if used < bytes.len() {
            sink.truncate(used as u64)?;
        }
        Ok(AuditWriter::with_sink(
            Box::new(sink),
            used as u64,
            chain.records.len() as u64,
            chain.head,
        ))
    }

    pub fn with_sink(sink: Box<dyn Sink>, len: u64, seq: u64, prev: [u8; 32]) -> AuditWriter {
        AuditWriter {
            sink,
            len,
            seq,
            prev,
            broken: false,
        }
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn head(&self) -> [u8; 32] {
        self.prev
    }

    /// Seal, frame, write and flush one record. State advances only when the bytes are on disk; on a
    /// failed write the partial record is cut off again, or, if that fails too, the writer refuses forever.
    pub fn append(&mut self, vault: &Vault, t: u64, ev: &str, d: Value) -> Result<(), ()> {
        if self.broken {
            return Err(());
        }
        let d: Map<String, Value> = match d {
            Value::Object(m) => m,
            _ => return Err(()),
        };
        let seq = self.seq + 1;
        let record = vault
            .seal_audit(seq, &self.prev, t, ev, &d)
            .map_err(|_| ())?;
        let framed = audit_frame(&record).map_err(|_| ())?;
        if self.sink.append(&framed).is_err() {
            self.broken = self.sink.truncate(self.len).is_err();
            return Err(());
        }
        self.len += framed.len() as u64;
        self.seq = seq;
        self.prev = Sha256::digest(&record).into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use figlock_core::{KdfParams, ZERO_PREV};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    const T: u64 = 1_700_000_000_000;

    fn vault() -> Vault {
        let mut pw = [0u8; 8];
        getrandom::fill(&mut pw).unwrap();
        Vault::create(&hex::encode(pw), "test", KdfParams::FLOOR, T)
            .unwrap()
            .0
    }

    #[derive(Default)]
    struct Flaky {
        fail: Arc<AtomicBool>,
        undo_fails: Arc<AtomicBool>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Sink for Flaky {
        fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
            if self.fail.load(Ordering::SeqCst) {
                // A partial write, as a full disk would leave.
                self.written.lock().unwrap().extend_from_slice(&bytes[..3]);
                return Err(io::Error::other("disk full"));
            }
            self.written.lock().unwrap().extend_from_slice(bytes);
            Ok(())
        }

        fn truncate(&mut self, len: u64) -> io::Result<()> {
            if self.undo_fails.load(Ordering::SeqCst) {
                return Err(io::Error::other("cannot truncate"));
            }
            self.written
                .lock()
                .unwrap()
                .truncate(usize::try_from(len).unwrap());
            Ok(())
        }
    }

    #[test]
    fn records_are_written_flushed_and_chained_across_reopen() {
        let v = vault();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit").join("v.log");
        {
            let mut w = AuditWriter::open(&path, &v).unwrap();
            assert_eq!((w.seq(), w.head()), (0, ZERO_PREV));
            w.append(&v, T, "broker.start", json!({})).unwrap();
            w.append(&v, T + 1, "grant.denied", json!({"code": "denied"}))
                .unwrap();
            assert_eq!(w.seq(), 2);
        }
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let parent = path.parent().unwrap();
        let dir_mode = fs::metadata(parent).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let mut w = AuditWriter::open(&path, &v).unwrap();
        assert_eq!(w.seq(), 2);
        w.append(&v, T + 2, "broker.lock", json!({})).unwrap();
        let (chain, damaged) = v.read_audit_log(&fs::read(&path).unwrap());
        assert_eq!(
            (chain.records.len(), chain.first_bad, damaged),
            (3, None, false)
        );
        assert_eq!(chain.head, w.head());
    }

    #[test]
    fn an_existing_loose_file_is_tightened() {
        let v = vault();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.log");
        fs::write(&path, b"").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        AuditWriter::open(&path, &v).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_cut_short_tail_is_dropped_but_a_bad_record_stops_the_broker() {
        let v = vault();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.log");
        let mut w = AuditWriter::open(&path, &v).unwrap();
        w.append(&v, T, "broker.start", json!({})).unwrap();
        w.append(&v, T + 1, "broker.lock", json!({})).unwrap();
        drop(w);
        let good = fs::read(&path).unwrap();

        // Crash during a write: half of a record at the end.
        let mut cut = good.clone();
        cut.extend_from_slice(&[0, 0, 1, 0, 9, 9, 9]);
        fs::write(&path, &cut).unwrap();
        let w = AuditWriter::open(&path, &v).unwrap();
        assert_eq!(w.seq(), 2);
        assert_eq!(fs::read(&path).unwrap(), good, "tail removed");
        drop(w);

        // A flipped bit inside a whole record: refuse, and leave the file alone.
        let mut bad = good.clone();
        bad[40] ^= 1;
        fs::write(&path, &bad).unwrap();
        let e = AuditWriter::open(&path, &v).err().unwrap();
        assert!(e.0.contains("failed verification at record 1"), "{e}");
        assert_eq!(fs::read(&path).unwrap(), bad);

        // Another vault's log.
        fs::write(&path, &good).unwrap();
        assert!(AuditWriter::open(&path, &vault()).is_err());
    }

    #[test]
    fn a_failed_write_leaves_no_partial_record_and_fails_closed_if_it_cannot_be_undone() {
        let v = vault();
        let sink = Flaky::default();
        let (fail, undo_fails, written) = (
            sink.fail.clone(),
            sink.undo_fails.clone(),
            sink.written.clone(),
        );
        let mut w = AuditWriter::with_sink(Box::new(sink), 0, 0, ZERO_PREV);
        w.append(&v, T, "broker.start", json!({})).unwrap();
        let before = written.lock().unwrap().clone();

        fail.store(true, Ordering::SeqCst);
        assert!(w.append(&v, T + 1, "broker.lock", json!({})).is_err());
        assert_eq!(*written.lock().unwrap(), before, "partial write removed");
        assert_eq!(w.seq(), 1, "state did not advance");

        fail.store(false, Ordering::SeqCst);
        w.append(&v, T + 2, "broker.lock", json!({})).unwrap();
        let (chain, damaged) = v.read_audit_log(&written.lock().unwrap());
        assert_eq!(
            (chain.records.len(), chain.first_bad, damaged),
            (2, None, false)
        );

        fail.store(true, Ordering::SeqCst);
        undo_fails.store(true, Ordering::SeqCst);
        assert!(w.append(&v, T + 3, "broker.lock", json!({})).is_err());
        fail.store(false, Ordering::SeqCst);
        assert!(
            w.append(&v, T + 4, "broker.lock", json!({})).is_err(),
            "a writer that could not undo a partial record never writes again"
        );
    }

    #[test]
    fn unknown_events_and_members_are_not_written() {
        let v = vault();
        let sink = Flaky::default();
        let written = sink.written.clone();
        let mut w = AuditWriter::with_sink(Box::new(sink), 0, 0, ZERO_PREV);
        assert!(w.append(&v, T, "made.up", json!({})).is_err());
        assert!(
            w.append(&v, T, "use.allowed", json!({"value": "x"}))
                .is_err()
        );
        assert!(w.append(&v, T, "use.allowed", json!(["a", "b"])).is_err());
        assert!(written.lock().unwrap().is_empty());
        assert_eq!(w.seq(), 0);
    }
}
