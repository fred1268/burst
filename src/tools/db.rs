use crate::cmds::constants::BURST_METADATA_FILE;
use crate::tools::error::{DbError, Error, Result};
use crate::tools::fs::FileSystem;
use crate::tools::version::VERSION_1_1;
use sqlx::query::Query;
use sqlx::sqlite::{SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqliteRow};
use sqlx::{Connection, Row, Sqlite};
use sqlx::sqlite::SqliteConnection;
use std::path::Path;
use std::time::Duration;
use tokio::sync::Mutex;

const TABLE_EXISTS: &str = "SELECT name FROM sqlite_master WHERE type='table' AND name=?";

const IP_TABLE: &str = "CREATE TABLE inprogress (snapshot_id INTEGER, FOREIGN KEY (snapshot_id) REFERENCES snapshots(id))";

const SS_TABLE: &str = "CREATE TABLE snapshots (id INTEGER PRIMARY KEY, created DATETIME DEFAULT current_timestamp, duration INTEGER DEFAULT 0, deleted INTEGER DEFAULT 0, status TEXT DEFAULT 'running', excluded_dirs INTEGER DEFAULT 0, excluded_files INTEGER DEFAULT 0, dirs INTEGER DEFAULT 0, count INTEGER DEFAULT 0, size INTEGER DEFAULT 0, new_count INTEGER DEFAULT 0, new_size INTEGER DEFAULT 0, modified_count INTEGER DEFAULT 0, modified_size INTEGER DEFAULT 0, unchanged_count INTEGER DEFAULT 0, unchanged_size INTEGER DEFAULT 0, deleted_count INTEGER DEFAULT 0, deleted_size INTEGER DEFAULT 0, failed_count INTEGER DEFAULT 0, failed_size INTEGER DEFAULT 0, compressed_size INTEGER DEFAULT 0, compression_ratio REAL DEFAULT 0.0)";

const SS_ZERO: &str = "INSERT INTO snapshots (id) VALUES (0)";

const FV_TABLE: &str = "CREATE TABLE fileversions (id INTEGER PRIMARY KEY, digest TEXT DEFAULT '', path TEXT, archive TEXT, name TEXT, size INTEGER, compressed_size INTEGER DEFAULT 0, encrypted BOOLEAN DEFAULT false, created DATETIME, modified DATETIME, deleted_sid INTEGER, is_dir INTEGER DEFAULT 0, FOREIGN KEY (deleted_sid) REFERENCES snapshots(id))";

const SF_TABLE: &str = "CREATE TABLE snapshotfiles (snapshot_id INTEGER, version_id INTEGER, PRIMARY KEY (snapshot_id, version_id), FOREIGN KEY (snapshot_id) REFERENCES snapshots(id), FOREIGN KEY (version_id) REFERENCES fileversions(id))";

const FH_TABLE: &str = "CREATE TABLE filehistory (id INTEGER, snapshot_id INTEGER, digest TEXT DEFAULT '', path TEXT, archive TEXT, name TEXT, size INTEGER, compressed_size INTEGER DEFAULT 0, encrypted BOOLEAN DEFAULT false, created DATETIME, modified DATETIME, deleted_sid INTEGER, is_dir INTEGER DEFAULT 0, FOREIGN KEY (deleted_sid) REFERENCES snapshots(id), FOREIGN KEY (snapshot_id) REFERENCES snapshots(id))";

const VER_TABLE: &str = "CREATE TABLE version (id TEXT)";
const VER_ZERO: &str = "INSERT INTO version (id) VALUES ('1.0')";
const VER_UPDATE: &str = "UPDATE version SET id=?";
const VER_READ: &str = "SELECT id FROM version";

const INDEX_FV: &str = "CREATE INDEX idx_fileversions ON fileversions(path, name)";
const INDEX_SF1: &str = "CREATE INDEX idx_snapshotfiles_snapshot ON snapshotfiles(snapshot_id)";
const INDEX_SF2: &str = "CREATE INDEX idx_snapshotfiles_version ON snapshotfiles(version_id)";

pub const MODE_WRITE: u8 = 0b0000_0001;
pub const MODE_LOCAL: u8 = 0b0000_0010;

pub struct Database {
    conn: Mutex<SqliteConnection>,
}

impl Database {
    pub async fn open(target: &Path) -> Result<Database> {
        let home_dir = FileSystem::home_backup_dir(target);
        let db_file = home_dir.join(BURST_METADATA_FILE);
        let opts =
            SqliteConnectOptions::new().filename(&db_file).journal_mode(SqliteJournalMode::Wal).busy_timeout(Duration::from_millis(5000));
        let conn = SqliteConnection::connect_with(&opts)
            .await
            .map_err(|err| Error::DbError(DbError::from("Cannot open database", err)))?;
        let db = Database { conn: Mutex::new(conn) };
        db.check_tables().await?;
        db.upgrade().await?;
        Ok(db)
    }

    async fn check_tables(&self) -> Result<()> {
        if !self.exists(TABLE_EXISTS, |q| q.bind("inprogress")).await? {
            self.execute(IP_TABLE, |q| q).await?;
        }
        if !self.exists(TABLE_EXISTS, |q| q.bind("snapshots")).await? {
            self.execute(SS_TABLE, |q| q).await?;
            self.execute(SS_ZERO, |q| q).await?;
        }
        if !self.exists(TABLE_EXISTS, |q| q.bind("fileversions")).await? {
            self.execute(FV_TABLE, |q| q).await?;
            self.execute(INDEX_FV, |q| q).await?;
        }
        if !self.exists(TABLE_EXISTS, |q| q.bind("snapshotfiles")).await? {
            self.execute(SF_TABLE, |q| q).await?;
            self.execute(INDEX_SF1, |q| q).await?;
            self.execute(INDEX_SF2, |q| q).await?;
        }
        if !self.exists(TABLE_EXISTS, |q| q.bind("version")).await? {
            self.execute(VER_TABLE, |q| q).await?;
            self.execute(VER_ZERO, |q| q).await?;
        }
        Ok(())
    }

    pub async fn exists<'a, B>(&self, sql: &'a str, bind: B) -> Result<bool>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
    {
        let mut conn = self.conn.lock().await;
        let row = bind(sqlx::query(sql)).fetch_optional(&mut *conn).await.map_err(|err| Error::DbError(DbError::from(sql, err)))?;
        Ok(row.is_some())
    }

    pub async fn execute<'a, B>(&self, sql: &'a str, bind: B) -> Result<()>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
    {
        let mut conn = self.conn.lock().await;
        bind(sqlx::query(sql)).execute(&mut *conn).await.map_err(|err| Error::DbError(DbError::from(sql, err)))?;
        Ok(())
    }

    pub async fn query_one<'a, T, B, F>(&self, sql: &'a str, bind: B, f: F) -> Result<Option<T>>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
        F: FnOnce(SqliteRow) -> sqlx::Result<T, sqlx::Error>,
    {
        let mut conn = self.conn.lock().await;
        let row = bind(sqlx::query(sql)).fetch_optional(&mut *conn).await.map_err(|err| Error::DbError(DbError::from(sql, err)))?;
        match row {
            Some(r) => Ok(Some(f(r).map_err(|err| Error::DbError(DbError::from(sql, err)))?)),
            None => Ok(None),
        }
    }

    pub async fn query_list<'a, T, B, F>(&self, sql: &'a str, bind: B, f: F) -> Result<Vec<T>>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
        F: Fn(SqliteRow) -> sqlx::Result<T, sqlx::Error>,
    {
        let mut conn = self.conn.lock().await;
        let rows = bind(sqlx::query(sql)).fetch_all(&mut *conn).await.map_err(|err| Error::DbError(DbError::from(sql, err)))?;
        rows.into_iter().map(|r| f(r).map_err(|err| Error::DbError(DbError::from(sql, err)))).collect()
    }

    async fn upgrade(&self) -> Result<()> {
        loop {
            let v: Option<String> = self.query_one(VER_READ, |q| q, |row| row.try_get(0)).await?;
            let version = match v {
                Some(v) => v,
                None => {
                    return Err(Error::GenericError(String::from("Cannot upgrade database: version not found")));
                }
            };
            match version.as_str() {
                "1.0" => self.upgrade_to_1_1().await?,
                "x.y" => self.upgrade_to_x_y().await?,
                _ => break,
            };
        }
        Ok(())
    }

    async fn upgrade_to_1_1(&self) -> Result<()> {
        self.execute(FH_TABLE, |q| q).await?;
        self.execute("INSERT INTO filehistory SELECT id, snapshot_id, digest, path, archive, name, size, compressed_size, encrypted, created, modified, deleted_sid, is_dir FROM fileversions fv JOIN snapshotfiles sf ON fv.id=sf.version_id WHERE sf.snapshot_id IN (SELECT id FROM snapshots ORDER BY created DESC LIMIT 1)", |q| q).await?;
        self.execute(VER_UPDATE, |q| q.bind(VERSION_1_1)).await
    }

    async fn upgrade_to_x_y(&self) -> Result<()> {
        // upgrade code here

        // self.execute(sqlx::query(VER_UPDATE).bind(VERSION_x_y)).await
        Ok(())
    }
}
