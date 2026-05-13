use crate::tools::db::Database;
use crate::tools::error::Error;
use crate::tools::fmt::{human_readable_duration, human_readable_size};
use chrono::{DateTime, Local};
use sqlx::query::Query;
use sqlx::sqlite::SqliteArguments;
use sqlx::{Row, Sqlite};
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

    pub async fn get_latest(db: &Database) -> Result<Option<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE deleted=0 ORDER BY created DESC LIMIT 1");
        Snapshot::one(db, &sql, |q| q).await
    }

    pub async fn get_previous(db: &Database, id: u64) -> Result<Option<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE deleted=0 AND id<?1 ORDER BY created DESC LIMIT 1");
        Snapshot::one(db, &sql, |q| q.bind(id as i64)).await
    }

    pub async fn get_previous_sync_mode(db: &Database, id: u64) -> Result<Option<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id<?1 ORDER BY created DESC LIMIT 1");
        Snapshot::one(db, &sql, |q| q.bind(id as i64)).await
    }

    pub async fn get(db: &Database, id: u64) -> Result<Option<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id=?1");
        Snapshot::one(db, &sql, |q| q.bind(id as i64)).await
    }

    pub async fn get_in_progress(db: &Database) -> Result<Option<Snapshot>, Error> {
        Snapshot::one(db, READ_IN_PROGRESS, |q| q).await
    }

    pub async fn get_last(db: &Database, limit: u64) -> Result<Vec<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id!=0 ORDER BY created DESC LIMIT ?1");
        Snapshot::list(db, &sql, |q| q.bind(limit as i64)).await
    }

    pub async fn get_except_last(db: &Database, limit: u64) -> Result<Vec<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id!=0 AND id NOT IN (SELECT id FROM snapshots ORDER BY created DESC LIMIT ?1) ORDER BY created DESC");
        Snapshot::list(db, &sql, |q| q.bind(limit as i64)).await
    }

    pub async fn get_before(db: &Database, date: DateTime<Local>) -> Result<Vec<Snapshot>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE id!=0 AND id IN (SELECT id FROM snapshots WHERE created<?1) ORDER BY created DESC");
        Snapshot::list(db, &sql, |q| q.bind(date)).await
    }

    pub async fn insert_next(db: &Database) -> Result<Option<Snapshot>, Error> {
        if let Some(snapshot) = Snapshot::one(db, INSERT, |q| q).await? {
            db.execute(IP_INSERT, |q| q.bind(snapshot.id as i64)).await?;
            return Ok(Some(snapshot));
        }
        Ok(None)
    }

    pub async fn update(&self, db: &Database) -> Result<(), Error> {
        db.execute(UPDATE, |q| {
            q.bind(self.id as i64)
                .bind(self.duration.as_secs() as i64)
                .bind(self.status.clone())
                .bind(self.excluded_dirs as i64)
                .bind(self.excluded_files as i64)
                .bind(self.dirs as i64)
                .bind(self.count as i64)
                .bind(self.size as i64)
                .bind(self.new_count as i64)
                .bind(self.new_size as i64)
                .bind(self.modified_count as i64)
                .bind(self.modified_size as i64)
                .bind(self.unchanged_count as i64)
                .bind(self.unchanged_size as i64)
                .bind(self.deleted_count as i64)
                .bind(self.deleted_size as i64)
        })
        .await
    }

    pub async fn mark_completed(&self, db: &Database) -> Result<(), Error> {
        self.update(db).await?;
        db.execute(DELETE_IN_PROGRESS, |q| q.bind(self.id as i64)).await
    }

    pub async fn delete(&self, db: &Database) -> Result<(), Error> {
        Snapshot::delete_by_id(db, self.id).await
    }

    pub async fn delete_by_id(db: &Database, id: u64) -> Result<(), Error> {
        let mut sql = String::from(DELETE);
        sql.push_str(" WHERE id=?1");
        db.execute(&sql, |q| q.bind(id as i64)).await?;
        db.execute(DELETE_IN_PROGRESS, |q| q.bind(id as i64)).await
    }

    pub async fn delete_all_except(&self, db: &Database) -> Result<(), Error> {
        let mut sql = String::from(DELETE);
        sql.push_str(" WHERE id!=?1");
        db.execute(&sql, |q| q.bind(self.id as i64)).await
    }

    async fn list<'a, B>(db: &Database, sql: &'a str, bind: B) -> Result<Vec<Snapshot>, Error>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
    {
        db.query_list(sql, bind, |row| {
            Ok(Snapshot {
                id: row.try_get::<i64, _>(0)? as u64,
                created: row.try_get(1)?,
                duration: Duration::from_secs(row.try_get::<i64, _>(2)? as u64),
                deleted: row.try_get(3)?,
                status: row.try_get(4)?,
                excluded_dirs: row.try_get::<i64, _>(5)? as u64,
                excluded_files: row.try_get::<i64, _>(6)? as u64,
                dirs: row.try_get::<i64, _>(7)? as u64,
                count: row.try_get::<i64, _>(8)? as u64,
                size: row.try_get::<i64, _>(9)? as u64,
                new_count: row.try_get::<i64, _>(10)? as u64,
                new_size: row.try_get::<i64, _>(11)? as u64,
                modified_count: row.try_get::<i64, _>(12)? as u64,
                modified_size: row.try_get::<i64, _>(13)? as u64,
                unchanged_count: row.try_get::<i64, _>(14)? as u64,
                unchanged_size: row.try_get::<i64, _>(15)? as u64,
                deleted_count: row.try_get::<i64, _>(16)? as u64,
                deleted_size: row.try_get::<i64, _>(17)? as u64,
            })
        })
        .await
    }

    async fn one<'a, B>(db: &Database, sql: &'a str, bind: B) -> Result<Option<Snapshot>, Error>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
    {
        db.query_one(sql, bind, |row| {
            Ok(Snapshot {
                id: row.try_get::<i64, _>(0)? as u64,
                created: row.try_get(1)?,
                duration: Duration::from_secs(row.try_get::<i64, _>(2)? as u64),
                deleted: row.try_get(3)?,
                status: row.try_get(4)?,
                excluded_dirs: row.try_get::<i64, _>(5)? as u64,
                excluded_files: row.try_get::<i64, _>(6)? as u64,
                dirs: row.try_get::<i64, _>(7)? as u64,
                count: row.try_get::<i64, _>(8)? as u64,
                size: row.try_get::<i64, _>(9)? as u64,
                new_count: row.try_get::<i64, _>(10)? as u64,
                new_size: row.try_get::<i64, _>(11)? as u64,
                modified_count: row.try_get::<i64, _>(12)? as u64,
                modified_size: row.try_get::<i64, _>(13)? as u64,
                unchanged_count: row.try_get::<i64, _>(14)? as u64,
                unchanged_size: row.try_get::<i64, _>(15)? as u64,
                deleted_count: row.try_get::<i64, _>(16)? as u64,
                deleted_size: row.try_get::<i64, _>(17)? as u64,
            })
        })
        .await
    }
}
