use crate::cmds::constants::BURST_METADATA_FILE;
use crate::tools::cmderror::CmdError::{self, DbError};
use crate::tools::fs::FileSystem;
use crate::tools::version::VERSION_1_1;
use rusqlite::{Connection, Params, Result, Row, params};
use rusqlite_regex;
use std::path::Path;

const TABLE_EXISTS: &str = "SELECT name FROM sqlite_master WHERE type='table' AND name=?1";

const IP_TABLE: &str = "CREATE TABLE inprogress (snapshot_id INTEGER, FOREIGN KEY (snapshot_id) REFERENCES snapshots(id))";

const SS_TABLE: &str = "CREATE TABLE snapshots (id INTEGER PRIMARY KEY, created DATETIME DEFAULT current_timestamp, duration INTEGER DEFAULT 0, deleted INTEGER DEFAULT 0, status TEXT DEFAULT 'running', excluded_dirs INTEGER DEFAULT 0, excluded_files INTEGER DEFAULT 0, dirs INTEGER DEFAULT 0, count INTEGER DEFAULT 0, size INTEGER DEFAULT 0, new_count INTEGER DEFAULT 0, new_size INTEGER DEFAULT 0, modified_count INTEGER DEFAULT 0, modified_size INTEGER DEFAULT 0, unchanged_count INTEGER DEFAULT 0, unchanged_size INTEGER DEFAULT 0, deleted_count INTEGER DEFAULT 0, deleted_size INTEGER DEFAULT 0, failed_count INTEGER DEFAULT 0, failed_size INTEGER DEFAULT 0, compressed_size INTEGER DEFAULT 0, compression_ratio REAL DEFAULT 0.0)";

const SS_ZERO: &str = "INSERT INTO snapshots (id) VALUES (0)";

const FV_TABLE: &str = "CREATE TABLE fileversions (id INTEGER PRIMARY KEY, digest TEXT DEFAULT '', path TEXT, archive TEXT, name TEXT, size INTEGER, compressed_size INTEGER DEFAULT 0, encrypted BOOLEAN DEFAULT false, created DATETIME, modified DATETIME, deleted_sid INTEGER, is_dir INTEGER DEFAULT 0, FOREIGN KEY (deleted_sid) REFERENCES snapshots(id))";

const SF_TABLE: &str = "CREATE TABLE snapshotfiles (snapshot_id INTEGER, version_id INTEGER, PRIMARY KEY (snapshot_id, version_id), FOREIGN KEY (snapshot_id) REFERENCES snapshots(id), FOREIGN KEY (version_id) REFERENCES fileversions(id))";

const FH_TABLE: &str = "CREATE TABLE filehistory (id INTEGER, snapshot_id INTEGER, digest TEXT DEFAULT '', path TEXT, archive TEXT, name TEXT, size INTEGER, compressed_size INTEGER DEFAULT 0, encrypted BOOLEAN DEFAULT false, created DATETIME, modified DATETIME, deleted_sid INTEGER, is_dir INTEGER DEFAULT 0, FOREIGN KEY (deleted_sid) REFERENCES snapshots(id), FOREIGN KEY (snapshot_id) REFERENCES snapshots(id))";

const VER_TABLE: &str = "CREATE TABLE version (id TEXT)";
const VER_ZERO: &str = "INSERT INTO version (id) VALUES ('1.0')";
const VER_UPDATE: &str = "UPDATE version SET id=?1";
const VER_READ: &str = "SELECT id FROM version";

const INDEX_FV: &str = "CREATE INDEX idx_fileversions ON fileversions(path, name)";
const INDEX_SF1: &str = "CREATE INDEX idx_snapshotfiles_snapshot ON snapshotfiles(snapshot_id)";
const INDEX_SF2: &str = "CREATE INDEX idx_snapshotfiles_version ON snapshotfiles(version_id)";

pub const MODE_WRITE: u8 = 0b0000_0001;
pub const MODE_LOCAL: u8 = 0b0000_0010;

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(target: &Path) -> Result<Database, CmdError> {
        rusqlite_regex::enable_auto_extension().map_err(|err| DbError(String::from("Cannot open database"), err.to_string()))?;
        let home_dir = FileSystem::home_backup_dir(target);
        let db_file = home_dir.join(BURST_METADATA_FILE);
        let db = Database {
            connection: Connection::open(&db_file).map_err(|err| DbError(String::from("Cannot open database"), err.to_string()))?,
        };
        db.check_tables()?;
        db.upgrade()?;
        Ok(db)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    fn check_tables(&self) -> Result<(), CmdError> {
        if !self.exists(TABLE_EXISTS, params!["inprogress"])? {
            self.execute(IP_TABLE, ())?;
        }
        if !self.exists(TABLE_EXISTS, params!["snapshots"])? {
            self.execute(SS_TABLE, ())?;
            self.execute(SS_ZERO, ())?;
        }
        if !self.exists(TABLE_EXISTS, params!["fileversions"])? {
            self.execute(FV_TABLE, ())?;
            self.execute(INDEX_FV, ())?;
        }
        if !self.exists(TABLE_EXISTS, params!["snapshotfiles"])? {
            self.execute(SF_TABLE, ())?;
            self.execute(INDEX_SF1, ())?;
            self.execute(INDEX_SF2, ())?;
        }
        if !self.exists(TABLE_EXISTS, params!["version"])? {
            self.execute(VER_TABLE, ())?;
            self.execute(VER_ZERO, ())?;
        }
        Ok(())
    }

    pub fn exists<P>(&self, sql: &str, params: P) -> Result<bool, CmdError>
    where
        P: Params,
    {
        let mut stmt = self.connection().prepare(sql).map_err(|err| DbError(String::from(sql), err.to_string()))?;
        stmt.exists(params).map_err(|err| DbError(String::from(sql), err.to_string()))
    }

    pub fn execute<P>(&self, sql: &str, params: P) -> Result<(), CmdError>
    where
        P: Params,
    {
        let mut stmt = self.connection().prepare(sql).map_err(|err| DbError(String::from(sql), err.to_string()))?;
        stmt.execute(params).map_err(|err| DbError(String::from(sql), err.to_string()))?;
        Ok(())
    }

    pub fn query_one<T, P, F>(&self, sql: &str, params: P, f: F) -> Result<Option<T>, CmdError>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> Result<T>,
    {
        let mut stmt = self.connection().prepare(sql).map_err(|err| DbError(String::from(sql), err.to_string()))?;
        match stmt.query_one(params, f) {
            Ok(one) => Ok(Some(one)),
            Err(err) => match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                _ => Err(CmdError::DbError(String::from(sql), err.to_string())),
            },
        }
    }

    pub fn query_list<T, P, F>(&self, sql: &str, params: P, f: F) -> Result<Vec<T>, CmdError>
    where
        P: Params,
        F: FnMut(&Row<'_>) -> Result<T>,
    {
        let mut stmt = self.connection().prepare(sql).map_err(|err| DbError(String::from(sql), err.to_string()))?;
        let list = stmt.query_map(params, f).map_err(|err| DbError(String::from(sql), err.to_string()))?;
        let mut result: Vec<T> = Vec::new();
        for one in list {
            let one = one.map_err(|err| DbError(String::from(sql), err.to_string()))?;
            result.push(one);
        }
        Ok(result)
    }

    fn upgrade(&self) -> Result<(), CmdError> {
        loop {
            let v: Option<String> = self.query_one(VER_READ, params![], |row| row.get(0))?;
            let version = match v {
                Some(v) => v,
                None => return Err(DbError(String::from("Version"), String::from("Not found"))),
            };
            match version.as_str() {
                "1.0" => self.upgrade_to_1_1()?,
                "x.y" => self.upgrade_to_x_y()?,
                _ => break,
            };
        }
        Ok(())
    }

    fn upgrade_to_1_1(&self) -> Result<(), CmdError> {
        self.execute(FH_TABLE, ())?;
        self.execute("INSERT INTO filehistory SELECT id, snapshot_id, digest, path, archive, name, size, compressed_size, encrypted, created, modified, deleted_sid, is_dir FROM fileversions fv JOIN snapshotfiles sf ON fv.id=sf.snapshot_id WHERE sf.snapshot_id IN (SELECT id FROM snapshots ORDER BY created DESC LIMIT 1)", ())?;
        self.execute(VER_UPDATE, params![VERSION_1_1])
    }

    fn upgrade_to_x_y(&self) -> Result<(), CmdError> {
        // upgrade code here

        // self.execute(VER_UPDATE, params![VERSION_x_y])
        Ok(())
    }
}
