//! Durable, user-wide request accounting for the polling detector.
//!
//! The exclusive owner is held for the repository lifetime. A transaction is
//! committed before dispatch; recovery conservatively retains a reservation
//! for any request whose response may have been lost. No evidence or key is
//! accepted by the repository API.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_platform::file_lock::ExclusiveFileLock;
use ilium_platform::secure_fs::{self, PrivateFileMode};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("polling ledger is already owned by another detector")]
    AlreadyOwned,
    #[error("polling detector lease no longer names the held owner")]
    LostOwnership,
    #[error("monthly polling budget reached")]
    BudgetReached,
    #[error("this activity window already has an attempt")]
    DuplicateWindow,
    #[error("polling history changed; restart pagination")]
    StaleCursor,
    #[error("invalid polling ledger input or state")]
    Invalid,
    #[error("polling ledger filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("polling ledger database error: {0}")]
    Database(#[from] rusqlite::Error),
}

pub struct NewAttempt {
    pub request_id: String,
    pub window_id: String,
    pub month: String,
    /// Local calendar day, determined by the coordinator at dispatch time.
    pub day: String,
    pub created_utc: String,
    pub project_id: String,
    pub model: String,
    pub reserved_nanodollars: i64,
}

/// Actual provider usage and the immutable rate snapshot used to estimate its
/// cost. Missing usage must take the `unknown` path instead of using zeros.
pub struct Completion {
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub cache_write_tokens: i64,
    pub pricing_version: String,
    pub uncached_rate: i64,
    pub cached_rate: i64,
    pub cache_write_rate: i64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LedgerSummary {
    pub attempts: i64,
    pub unknown_attempts: i64,
    pub known_cost_nanodollars: i64,
    pub reserved_nanodollars: i64,
    pub known_input_tokens: i64,
}

/// Safe metadata for a paged accounting view; this is not transcript content.
#[derive(Debug, PartialEq, Eq)]
pub struct LedgerRecord {
    pub sequence: i64,
    pub request_id: String,
    pub created_utc: String,
    pub project_id: String,
    pub model: String,
    pub state: String,
    pub input_tokens: Option<i64>,
    pub cost_nanodollars: Option<i64>,
    pub reserved_nanodollars: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerCursor {
    pub revision: u64,
    pub after_sequence: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LedgerPage {
    pub revision: u64,
    pub records: Vec<LedgerRecord>,
    pub next: Option<LedgerCursor>,
}

/// Process-wide lease custody, independent of any SQLite connection.
/// Keep this owner in every physical I/O job until the connection is closed.
pub struct LedgerOwner {
    _lease: ExclusiveFileLock,
    leader_epoch: String,
    lease_path: PathBuf,
    lease_generation: (u64, u64),
    database_path: PathBuf,
}

impl LedgerOwner {
    /// The published lifetime of this exclusive detector owner.
    pub fn leader_epoch(&self) -> &str {
        &self.leader_epoch
    }

    fn ensure_ownership(&self) -> Result<(), LedgerError> {
        verify_lease(&self.lease_path, self.lease_generation, &self.leader_epoch)
    }
}

pub struct Ledger {
    connection: Connection,
    owner: std::sync::Arc<LedgerOwner>,
}

impl Ledger {
    /// Close this worker's SQLite connection without ending leadership.
    pub fn into_owner(self) -> std::sync::Arc<LedgerOwner> {
        let Self { connection, owner } = self;
        drop(connection);
        owner
    }

    /// Open a worker-local connection under existing leadership. Recovery is
    /// performed only by `open`, never at each connection boundary.
    pub fn connect(owner: std::sync::Arc<LedgerOwner>) -> Result<Self, LedgerError> {
        owner.ensure_ownership()?;
        drop(secure_fs::open_private_regular_file(
            &owner.database_path,
            PrivateFileMode::Read,
        )?);
        let connection = Connection::open_with_flags(
            &owner.database_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        configure_connection(&connection)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != 4 {
            return Err(LedgerError::Invalid);
        }
        owner.ensure_ownership()?;
        Ok(Self { connection, owner })
    }

    pub fn open(directory: &Path) -> Result<Self, LedgerError> {
        secure_fs::create_private_directory(directory)?;
        let lease_path = directory.join("polling-detector.lock");
        let mut lease = secure_fs::private_open_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lease_path)?;
        secure_fs::restrict_open_file_to_owner(&lease)?;
        let owner = ExclusiveFileLock::try_acquire_opened(lease.try_clone()?)?
            .ok_or(LedgerError::AlreadyOwned)?;
        // Publish only through the admitted, held inode. Reopening the path
        // could stamp a replacement file instead of the actual writer lease.
        let lease_generation = secure_fs::file_generation(&lease)?;
        let leader_epoch = uuid::Uuid::new_v4().to_string();
        lease.set_len(0)?;
        lease.write_all(leader_epoch.as_bytes())?;
        lease.sync_all()?;
        verify_lease(&lease_path, lease_generation, &leader_epoch)?;
        let path = directory.join("polling-ledger.sqlite3");
        // Admit/create a private regular file before SQLite opens it. Its parent
        // is owner-only and contains the journal alongside the database.
        drop(secure_fs::open_private_regular_file(
            &path,
            PrivateFileMode::Append,
        )?);
        let mut connection = Connection::open(&path)?;
        configure_connection(&connection)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 4 {
            return Err(LedgerError::Invalid);
        }
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS attempts (
            request_id TEXT PRIMARY KEY, window_id TEXT NOT NULL UNIQUE,
            month TEXT NOT NULL, created_utc TEXT NOT NULL, project_id TEXT NOT NULL,
            model TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('prepared','dispatched','unknown','cancelled','completed')),
            reserved_nanodollars INTEGER NOT NULL CHECK(reserved_nanodollars >= 0),
            input_tokens INTEGER, cached_tokens INTEGER, cache_write_tokens INTEGER,
            cost_nanodollars INTEGER, pricing_version TEXT, uncached_rate INTEGER,
            cached_rate INTEGER, cache_write_rate INTEGER,
            CHECK(state != 'completed' OR (input_tokens IS NOT NULL AND cached_tokens IS NOT NULL
                AND cache_write_tokens IS NOT NULL AND cost_nanodollars IS NOT NULL
                AND pricing_version IS NOT NULL AND uncached_rate IS NOT NULL
                AND cached_rate IS NOT NULL AND cache_write_rate IS NOT NULL))
        );
        CREATE INDEX IF NOT EXISTS attempts_month ON attempts(month, created_utc, request_id);")?;
        if version < 2 {
            // Existing records predate a local-day bucket. Leave them NULL:
            // their local dates cannot be inferred safely from UTC alone.
            transaction.execute("ALTER TABLE attempts ADD COLUMN day TEXT", [])?;
        }
        transaction.execute_batch("CREATE INDEX IF NOT EXISTS attempts_day ON attempts(day);")?;
        if version < 3 {
            transaction.execute_batch(include_str!("polling_ledger_totals.sql"))?;
        }
        if version < 4 {
            transaction.execute_batch(include_str!("polling_ledger_history.sql"))?;
        }
        transaction.execute_batch("PRAGMA user_version=4;")?;
        transaction.execute(
            "UPDATE attempts SET state='cancelled' WHERE state='prepared'",
            [],
        )?;
        transaction.execute(
            "UPDATE attempts SET state='unknown' WHERE state='dispatched'",
            [],
        )?;
        transaction.commit()?;
        // Verify again after migration/recovery before publishing the owner.
        verify_lease(&lease_path, lease_generation, &leader_epoch)?;
        Ok(Self {
            connection,
            owner: std::sync::Arc::new(LedgerOwner {
                _lease: owner,
                leader_epoch,
                lease_path,
                lease_generation,
                database_path: path,
            }),
        })
    }

    /// The published lifetime of this exclusive detector owner.
    pub fn leader_epoch(&self) -> &str {
        &self.owner.leader_epoch
    }

    fn ensure_ownership(&self) -> Result<(), LedgerError> {
        self.owner.ensure_ownership()
    }

    pub fn prepare(
        &mut self,
        attempt: &NewAttempt,
        limit_nanodollars: Option<i64>,
    ) -> Result<(), LedgerError> {
        self.ensure_ownership()?;
        if limit_nanodollars == Some(0) {
            return Err(LedgerError::BudgetReached);
        }
        if attempt.reserved_nanodollars <= 0
            || limit_nanodollars.is_some_and(|limit| limit < 0)
            || !valid_month(&attempt.month)
            || !valid_day(&attempt.day)
            || !attempt.day.starts_with(&attempt.month)
            || !plausible_local_day(&attempt.created_utc, &attempt.day)
            || [
                &attempt.request_id,
                &attempt.window_id,
                &attempt.month,
                &attempt.created_utc,
                &attempt.project_id,
                &attempt.model,
            ]
            .iter()
            .any(|value| value.is_empty() || value.len() > 256)
        {
            return Err(LedgerError::Invalid);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM attempts WHERE window_id=?1",
                [&attempt.window_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_some() {
            return Err(LedgerError::DuplicateWindow);
        }
        let totals: Option<(i64, i64)> = transaction.query_row(
            "SELECT known_cost_nanodollars,reserved_nanodollars FROM polling_totals WHERE bucket=?1",
            [format!("month:{}", attempt.month)], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let (known, reserved) = totals.unwrap_or_default();
        let used = known.checked_add(reserved).ok_or(LedgerError::Invalid)?;
        let projected = used
            .checked_add(attempt.reserved_nanodollars)
            .ok_or(LedgerError::Invalid)?;
        if limit_nanodollars.is_some_and(|limit| projected > limit) {
            return Err(LedgerError::BudgetReached);
        }
        transaction.execute("INSERT INTO attempts(request_id,window_id,month,day,created_utc,project_id,model,state,reserved_nanodollars) VALUES(?1,?2,?3,?4,?5,?6,?7,'prepared',?8)",
            params![attempt.request_id, attempt.window_id, attempt.month, attempt.day, attempt.created_utc, attempt.project_id, attempt.model, attempt.reserved_nanodollars])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_dispatched(&mut self, request_id: &str) -> Result<(), LedgerError> {
        self.transition(request_id, "prepared", "dispatched")
    }

    pub fn cancel_prepared(&mut self, request_id: &str) -> Result<(), LedgerError> {
        self.transition(request_id, "prepared", "cancelled")
    }

    pub fn mark_unknown(&mut self, request_id: &str) -> Result<(), LedgerError> {
        self.transition(request_id, "dispatched", "unknown")
    }

    fn transition(&mut self, request_id: &str, from: &str, to: &str) -> Result<(), LedgerError> {
        self.ensure_ownership()?;
        let changed = self.connection.execute(
            "UPDATE attempts SET state=?1 WHERE request_id=?2 AND state=?3",
            params![to, request_id, from],
        )?;
        if changed != 1 {
            return Err(LedgerError::Invalid);
        }
        Ok(())
    }

    pub fn complete(
        &mut self,
        request_id: &str,
        completion: &Completion,
    ) -> Result<(), LedgerError> {
        self.ensure_ownership()?;
        if [
            completion.input_tokens,
            completion.cached_tokens,
            completion.cache_write_tokens,
            completion.uncached_rate,
            completion.cached_rate,
            completion.cache_write_rate,
        ]
        .iter()
        .any(|value| *value < 0)
            || completion
                .cached_tokens
                .checked_add(completion.cache_write_tokens)
                .is_none_or(|total| total > completion.input_tokens)
            || completion.pricing_version.is_empty()
            || completion.pricing_version.len() > 256
        {
            return Err(LedgerError::Invalid);
        }
        let usage = ilium_core::polling_detection::TokenUsage {
            input: completion.input_tokens as u64,
            cache_read: completion.cached_tokens as u64,
            cache_write: completion.cache_write_tokens as u64,
        };
        let rates = ilium_core::polling_detection::TokenRates {
            uncached_nanos_per_million: completion.uncached_rate as u64,
            cache_read_nanos_per_million: completion.cached_rate as u64,
            cache_write_nanos_per_million: completion.cache_write_rate as u64,
        };
        let cost = ilium_core::polling_detection::estimate_cost_nanodollars(usage, rates)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(LedgerError::Invalid)?;
        let changed = self.connection.execute("UPDATE attempts SET state='completed',input_tokens=?1,cached_tokens=?2,cache_write_tokens=?3,cost_nanodollars=?4,pricing_version=?5,uncached_rate=?6,cached_rate=?7,cache_write_rate=?8 WHERE request_id=?9 AND state='dispatched'", params![completion.input_tokens, completion.cached_tokens, completion.cache_write_tokens, cost, completion.pricing_version, completion.uncached_rate, completion.cached_rate, completion.cache_write_rate, request_id])?;
        if changed != 1 {
            return Err(LedgerError::Invalid);
        }
        Ok(())
    }

    pub fn summary(&self, month: &str) -> Result<LedgerSummary, LedgerError> {
        if !valid_month(month) {
            return Err(LedgerError::Invalid);
        }
        self.summary_bucket(&format!("month:{month}"))
    }

    /// `None` means legacy rows in the month have no reliable local day.
    pub fn summary_day(&self, day: &str) -> Result<Option<LedgerSummary>, LedgerError> {
        if !valid_day(day) {
            return Err(LedgerError::Invalid);
        }
        let unknown_days: Option<i64> = self
            .connection
            .query_row(
                "SELECT unknown_days FROM polling_totals WHERE bucket=?1",
                [format!("month:{}", &day[..7])],
                |row| row.get(0),
            )
            .optional()?;
        if unknown_days.unwrap_or_default() > 0 {
            return Ok(None);
        }
        self.summary_bucket(&format!("day:{day}")).map(Some)
    }

    pub fn summary_all_time(&self) -> Result<LedgerSummary, LedgerError> {
        self.summary_bucket("all")
    }

    fn summary_bucket(&self, bucket: &str) -> Result<LedgerSummary, LedgerError> {
        let summary = self.connection.query_row(
            "SELECT attempts,unknown_attempts,known_cost_nanodollars,reserved_nanodollars,known_input_tokens FROM polling_totals WHERE bucket=?1",
            [bucket], |row| Ok(LedgerSummary {
                attempts: row.get(0)?, unknown_attempts: row.get(1)?,
                known_cost_nanodollars: row.get(2)?, reserved_nanodollars: row.get(3)?,
                known_input_tokens: row.get(4)?,
            })).optional()?;
        Ok(summary.unwrap_or_default())
    }

    /// The revision and rows share one read transaction; a continuation never
    /// silently mixes rows from before and after accounting or crash recovery.
    pub fn history_page(
        &mut self,
        cursor: Option<LedgerCursor>,
        limit: u8,
    ) -> Result<LedgerPage, LedgerError> {
        if !(1..=100).contains(&limit) || cursor.is_some_and(|cursor| cursor.after_sequence < 0) {
            return Err(LedgerError::Invalid);
        }
        let transaction = self.connection.transaction()?;
        let revision: u64 = transaction.query_row(
            "SELECT revision FROM polling_history_revision WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if cursor.is_some_and(|cursor| cursor.revision != revision) {
            return Err(LedgerError::StaleCursor);
        }
        let after = cursor.map_or(0, |cursor| cursor.after_sequence);
        let mut records = read_records(&transaction, after, usize::from(limit) + 1)?;
        let next = if records.len() > usize::from(limit) {
            records.truncate(usize::from(limit));
            records.last().map(|record| LedgerCursor {
                revision,
                after_sequence: record.sequence,
            })
        } else {
            None
        };
        transaction.commit()?;
        Ok(LedgerPage {
            revision,
            records,
            next,
        })
    }
}

fn read_records(
    connection: &Connection,
    sequence: i64,
    limit: usize,
) -> Result<Vec<LedgerRecord>, LedgerError> {
    let mut statement = connection.prepare("SELECT rowid,request_id,created_utc,project_id,model,state,input_tokens,cost_nanodollars,reserved_nanodollars FROM attempts WHERE rowid > ?1 ORDER BY rowid LIMIT ?2")?;
    let rows = statement.query_map(params![sequence, limit], |row| {
        Ok(LedgerRecord {
            sequence: row.get(0)?,
            request_id: row.get(1)?,
            created_utc: row.get(2)?,
            project_id: row.get(3)?,
            model: row.get(4)?,
            state: row.get(5)?,
            input_tokens: row.get(6)?,
            cost_nanodollars: row.get(7)?,
            reserved_nanodollars: row.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn valid_month(month: &str) -> bool {
    let bytes = month.as_bytes();
    if bytes.len() != 7
        || bytes[4] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let number = (bytes[5] - b'0') * 10 + bytes[6] - b'0';
    (1..=12).contains(&number)
}

fn valid_day(day: &str) -> bool {
    day.len() == 10
        && chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .is_ok_and(|parsed| parsed.format("%Y-%m-%d").to_string() == day)
}

fn plausible_local_day(created_utc: &str, day: &str) -> bool {
    let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(created_utc) else {
        return false;
    };
    if timestamp.offset().local_minus_utc() != 0 {
        return false;
    }
    let Ok(local_day) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return false;
    };
    local_day
        .signed_duration_since(timestamp.date_naive())
        .num_days()
        .abs()
        <= 1
}

/// The guard keeps the admitted inode alive; an equal epoch on a replacement
/// inode cannot transfer that ownership. Queries remain available for diagnosis.
fn configure_connection(connection: &Connection) -> Result<(), LedgerError> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
    )?;
    Ok(())
}

fn verify_lease(path: &Path, generation: (u64, u64), epoch: &str) -> Result<(), LedgerError> {
    let file = match secure_fs::open_private_regular_file(path, PrivateFileMode::Read) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(LedgerError::LostOwnership)
        }
        Err(error) => return Err(error.into()),
    };
    if secure_fs::file_generation(&file)? != generation {
        return Err(LedgerError::LostOwnership);
    }
    let mut published = String::new();
    file.take(257).read_to_string(&mut published)?;
    if published != epoch {
        return Err(LedgerError::LostOwnership);
    }
    Ok(())
}
