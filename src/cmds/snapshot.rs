use crate::tools::cmderror::CmdError;
use crate::tools::db::Database;
use crate::tools::fmt::{human_readable_duration, human_readable_size};
use chrono::{DateTime, Local};
use rusqlite::{Params, params};
use std::fmt;
use std::time::Duration;

const READ: &str = "SELECT id, created, duration, deleted, status, excluded_dirs, excluded_files, dirs, count, size, new_count, new_size, modified_count, modified_size, unchanged_count, unchanged_size, deleted_count, deleted_size FROM snapshots";

const READ_IN_PROGRESS: &str = "SELECT id, created, duration, deleted, status, excluded_dirs, excluded_files, dirs, count, size, new_count, new_size, modified_count, modified_size, unchanged_count, unchanged_size, deleted_count, deleted_size FROM snapshots JOIN inprogress ON id=snapshot_id ORDER BY snapshot_id DESC LIMIT 1";

const INSERT: &str = "INSERT INTO snapshots DEFAULT VALUES RETURNING id, created, duration, deleted, status, excluded_dirs, excluded_files, dirs, count, size, new_count, new_size, modified_count, modified_size, unchanged_count, unchanged_size, deleted_count, deleted_size";

const UPDATE: &str = "UPDATE snapshots SET duration=?2, status=?3, excluded_dirs=?4, excluded_files=?5, dirs=?6, count=?7, size=?8, new_count=?9, new_size=?10, modified_count=?11, modified_size=?12, unchanged_count=?13, unchanged_size=?14, deleted_count=?15, deleted_size=?16 WHERE id=?1";

const DELETE: &str = "UPDATE snapshots SET deleted=1";

const DELETE_IN_PROGRESS: &str = "DELETE FROM inprogress WHERE snapshot_id=?1";

const IP_INSERT: &str = "INSERT INTO inprogress (snapshot_id) VALUES (?1)";

#[derive(Default)]
pub struct Snapshot {
    pub id: u64,
    pub created: DateTime<Local>,
    pub duration: Duration,
    pub deleted: bool,
    pub status: String,
    pub excluded_dirs: u64,
    pub excluded_files: u64,
    pub dirs: u64,
    pub count: u64,
    pub size: u64,
    pub new_count: u64,
    pub new_size: u64,
    pub modified_count: u64,
    pub modified_size: u64,
    pub unchanged_count: u64,
    pub unchanged_size: u64,
    pub deleted_count: u64,
    pub deleted_size: u64,
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.deleted {
            true => write!(f, "{:06} ", self.id)?,
            false => write!(f, "{:>6} ", self.id)?,
        }
        write!(
            f,
            "{:>18} {:>8} {:>10} {:>10} {:>23} {:>18} {:>18} {:>18} {:>18}",
            self.created.format("%Y-%b-%d %H:%M"),
            self.status,
            human_readable_duration(self.duration.as_secs()),
            format!("{}/{}", self.excluded_dirs, self.excluded_files),
            format!("{}/{} ({})", self.dirs, self.count, human_readable_size(self.size)),
            format!("{} ({})", self.new_count, human_readable_size(self.new_size)),
            format!("{} ({})", self.modified_count, human_readable_size(self.modified_size)),
            format!("{} ({})", self.unchanged_count, human_readable_size(self.unchanged_size)),
            format!("{} ({})", self.deleted_count, human_readable_size(self.deleted_size))
        )
    }
}

impl Snapshot {
    pub fn header() {
        println!(
            "{:>6} {:>18} {:>8} {:>10} {:>10} {:>23} {:>18} {:>18} {:>18} {:>18}",
            "id", "date", "status", "duration", "excluded", "total", "new", "modified", "unchanged", "deleted"
        );
        //      date               status  duration  excluded  total\t\t\tnew\t\t\tmodified\t\tunchanged\t\tdeleted");
    }

    pub fn get_latest(db: &Database) -> Result<Option<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE deleted=0 ORDER BY created DESC LIMIT 1");
        Snapshot::one(db, &sql, params![])
    }

    pub fn get_previous(db: &Database, id: u64) -> Result<Option<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE deleted=0 AND id<?1 ORDER BY created DESC LIMIT 1");
        Snapshot::one(db, &sql, params![id])
    }

    pub fn get_previous_sync_mode(db: &Database, id: u64) -> Result<Option<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id<?1 ORDER BY created DESC LIMIT 1");
        Snapshot::one(db, &sql, params![id])
    }

    pub fn get(db: &Database, id: u64) -> Result<Option<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id=?1");
        Snapshot::one(db, &sql, params![id])
    }

    pub fn get_in_progress(db: &Database) -> Result<Option<Snapshot>, CmdError> {
        Snapshot::one(db, READ_IN_PROGRESS, params![])
    }

    pub fn get_last(db: &Database, limit: u64) -> Result<Vec<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id!=0 ORDER BY created DESC LIMIT ?1");
        Snapshot::list(db, &sql, params![limit])
    }

    pub fn get_except_last(db: &Database, limit: u64) -> Result<Vec<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id!=0 AND id NOT IN (SELECT id FROM snapshots ORDER BY created DESC LIMIT ?1) ORDER BY created DESC");
        Snapshot::list(db, &sql, params![limit])
    }

    pub fn get_before(db: &Database, date: DateTime<Local>) -> Result<Vec<Snapshot>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id!=0 AND id IN (SELECT id FROM snapshots WHERE created<?1) ORDER BY created DESC");
        Snapshot::list(db, &sql, params![date])
    }

    pub fn insert_next(db: &Database) -> Result<Option<Snapshot>, CmdError> {
        if let Some(snapshot) = Snapshot::one(db, INSERT, params![])? {
            db.execute(IP_INSERT, params![snapshot.id])?;
            return Ok(Some(snapshot));
        }
        Ok(None)
    }

    pub fn update(&self, db: &Database) -> Result<(), CmdError> {
        db.execute(
            UPDATE,
            params![
                self.id,
                self.duration.as_secs(),
                self.status,
                self.excluded_dirs,
                self.excluded_files,
                self.dirs,
                self.count,
                self.size,
                self.new_count,
                self.new_size,
                self.modified_count,
                self.modified_size,
                self.unchanged_count,
                self.unchanged_size,
                self.deleted_count,
                self.deleted_size
            ],
        )
    }

    pub fn mark_completed(&self, db: &Database) -> Result<(), CmdError> {
        self.update(db)?;
        db.execute(DELETE_IN_PROGRESS, params![self.id])
    }

    pub fn delete(&self, db: &Database) -> Result<(), CmdError> {
        Snapshot::delete_by_id(db, self.id)
    }

    pub fn delete_by_id(db: &Database, id: u64) -> Result<(), CmdError> {
        let mut sql = String::from(DELETE);
        sql.push_str(" WHERE id=?1");
        db.execute(&sql, params![id])?;
        db.execute(DELETE_IN_PROGRESS, params![id])
    }

    pub fn delete_all_except(&self, db: &Database) -> Result<(), CmdError> {
        let mut sql = String::from(DELETE);
        sql.push_str(" WHERE id!=?1");
        db.execute(&sql, params![self.id])
    }

    fn list<P>(db: &Database, sql: &str, params: P) -> Result<Vec<Snapshot>, CmdError>
    where
        P: Params,
    {
        db.query_list(sql, params, |row| {
            Ok(Snapshot {
                id: row.get(0)?,
                created: row.get(1)?,
                duration: Duration::from_secs(row.get(2)?),
                deleted: row.get(3)?,
                status: row.get(4)?,
                excluded_dirs: row.get(5)?,
                excluded_files: row.get(6)?,
                dirs: row.get(7)?,
                count: row.get(8)?,
                size: row.get(9)?,
                new_count: row.get(10)?,
                new_size: row.get(11)?,
                modified_count: row.get(12)?,
                modified_size: row.get(13)?,
                unchanged_count: row.get(14)?,
                unchanged_size: row.get(15)?,
                deleted_count: row.get(16)?,
                deleted_size: row.get(17)?,
            })
        })
    }

    fn one<P>(db: &Database, sql: &str, params: P) -> Result<Option<Snapshot>, CmdError>
    where
        P: Params,
    {
        db.query_one(sql, params, |row| {
            Ok(Snapshot {
                id: row.get(0)?,
                created: row.get(1)?,
                duration: Duration::from_secs(row.get(2)?),
                deleted: row.get(3)?,
                status: row.get(4)?,
                excluded_dirs: row.get(5)?,
                excluded_files: row.get(6)?,
                dirs: row.get(7)?,
                count: row.get(8)?,
                size: row.get(9)?,
                new_count: row.get(10)?,
                new_size: row.get(11)?,
                modified_count: row.get(12)?,
                modified_size: row.get(13)?,
                unchanged_count: row.get(14)?,
                unchanged_size: row.get(15)?,
                deleted_count: row.get(16)?,
                deleted_size: row.get(17)?,
            })
        })
    }
}
