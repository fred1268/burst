use crate::cmds::constants::BURST_VERSION_DIR;
use crate::cmds::snapshot::Snapshot;
use crate::tools::cmderror::CmdError::{self};
use crate::tools::db::Database;
use crate::tools::fmt::human_readable_size;
use chrono::{DateTime, Local};
use rusqlite::{Params, params};
use std::collections::HashMap;
use std::fmt;
use std::fs::Metadata;
#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_family = "windows")]
use std::os::windows::fs::MetadataExt;
use std::path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path, PathBuf};

const READ: &str = "SELECT fv.id, sf.snapshot_id, fv.digest, fv.path, fv.archive, fv.name, fv.size, fv.created, fv.modified, deleted_sid, fv.is_dir FROM fileversions fv JOIN snapshotfiles sf ON fv.id=sf.version_id";

const READ_SYNC_MODE: &str =
    "SELECT id, snapshot_id, digest, path, archive, name, size, created, modified, deleted_sid, is_dir FROM filehistory";

const READ_NO_SNAPSHOT: &str = "SELECT id, 0, digest, path, archive, name, size, created, modified, deleted_sid, is_dir FROM fileversions";

const ORPHANS: &str = "SELECT id, 0, digest, path, archive, name, size, created, modified, deleted_sid, is_dir FROM fileversions WHERE id NOT IN (SELECT version_id FROM snapshotfiles) ORDER BY is_dir ASC, path DESC, name DESC";

const INSERT: &str = "INSERT INTO fileversions (digest, path, archive, name, size, created, modified, deleted_sid, is_dir) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id";

const INSERT_REF: &str = "INSERT INTO snapshotfiles (snapshot_id, version_id) VALUES (?1, ?2)";

const INSERT_HISTORY_SYNC_MODE: &str = "INSERT INTO filehistory SELECT id, ?1, digest, path, archive, name, size, compressed_size, encrypted, created, modified, deleted_sid, is_dir FROM fileversions fv JOIN snapshotfiles sf ON fv.id=sf.version_id WHERE sf.snapshot_id=?1";

const ARCHIVE: &str = "UPDATE fileversions SET archive=?2, deleted_sid=?3 WHERE id=?1";

const UPDATE_REF: &str = "UPDATE snapshotfiles SET version_id=?1 WHERE version_id=?2";

const DELETE: &str = "DELETE FROM fileversions WHERE id=?1";

const DELETE_REF: &str = "DELETE FROM snapshotfiles";

const DELETE_SNAPSHOT: &str = "DELETE FROM snapshotfiles WHERE snapshot_id=?1";

enum What {
    #[allow(dead_code)]
    All,
    Dirs,
    Files,
}

#[derive(PartialEq, Eq, Hash, Debug)]
pub struct File {
    pub id: u64,
    pub sid: u64,
    pub digest: String,
    pub path: PathBuf,
    pub archive: PathBuf,
    pub name: PathBuf,
    pub size: u64,
    pub created: DateTime<Local>,
    pub modified: DateTime<Local>,
    pub deleted_sid: u64,
    pub is_dir: bool,
}

impl fmt::Display for File {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let id = match self.deleted_sid {
            0 => format!("{:>5}", self.sid),
            _ => format!("{:>5}", -(self.deleted_sid as i64)),
        };
        write!(
            f,
            "{:>6} {:>18} {:>18}    {:?} ({})",
            id,
            self.created.format("%Y-%b-%d %H:%M"),
            self.modified.format("%Y-%b-%d %H:%M"),
            self.fullname(),
            human_readable_size(self.size),
        )
    }
}

impl File {
    pub fn from_metadata(fullname: &Path, meta: Metadata) -> Self {
        let name = PathBuf::from(fullname.file_name().unwrap_or(PathBuf::new().as_os_str()));
        let path = PathBuf::from(fullname.parent().unwrap_or(&PathBuf::new()));
        let (ctime, mtime, size) = Self::extract_from(&meta);
        let created = DateTime::from_timestamp_secs(ctime).unwrap().with_timezone(&Local);
        let modified = DateTime::from_timestamp_secs(mtime).unwrap().with_timezone(&Local);
        File {
            id: 0,
            sid: 0,
            digest: String::new(),
            path,
            archive: PathBuf::new(),
            name,
            size,
            created,
            modified,
            deleted_sid: 0,
            is_dir: meta.is_dir(),
        }
    }

    #[cfg(target_family = "unix")]
    fn extract_from(meta: &Metadata) -> (i64, i64, u64) {
        let ctime = meta.ctime();
        let mtime = meta.mtime();
        let size = meta.size();
        (ctime, mtime, size)
    }

    #[cfg(target_family = "windows")]
    fn extract_from(meta: &Metadata) -> (i64, i64, u64) {
        let ctime = (meta.creation_time() as i64 - 116444736000000000 as i64) / 10000000 as i64;
        let mtime = (meta.last_write_time() as i64 - 116444736000000000 as i64) / 10000000 as i64;
        let size = meta.file_size();
        (ctime, mtime, size)
    }

    pub fn fullname(&self) -> PathBuf {
        PathBuf::from(&self.path).join(&self.name)
    }

    pub fn source_name(&self, dir: &Path) -> PathBuf {
        PathBuf::from(dir).join(&self.path).join(&self.name)
    }

    pub fn archive_dir(&self, dir: &Path) -> PathBuf {
        PathBuf::from(dir).join(&self.path).join(&self.archive)
    }

    pub fn archive_name(&self, file: &Path) -> PathBuf {
        match self.is_dir {
            true => PathBuf::from(file).join(&self.path).join(&self.name).join(&self.archive),
            false => self.archive_dir(file).join(&self.name),
        }
    }

    pub fn is_archived(&self) -> bool {
        !self.archive.as_os_str().is_empty()
    }

    pub fn header() {
        println!("{:>6} {:>18} {:>18}    {:?}", "sid", "created", "modified", "name (size)")
    }

    fn entry_type(sql: &mut String, what: What) {
        match what {
            What::All => (),
            What::Dirs => sql.push_str(" AND is_dir=1"),
            What::Files => sql.push_str(" AND is_dir=0"),
        }
    }

    fn fqn(name: &Path) -> String {
        match name.components().count() {
            1 => {
                let mut p = String::from(MAIN_SEPARATOR_STR);
                p.push_str(name.to_str().unwrap());
                p
            }
            _ => String::from(name.to_str().unwrap()),
        }
    }

    pub fn find_entry(db: &Database, name: &Path) -> Result<Option<File>, CmdError> {
        let path = Self::fqn(name);
        let mut sql = String::from(READ_NO_SNAPSHOT);
        sql.push_str(" WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name=?1");
        let mut files = File::list(db, &sql, params![path])?;
        if !files.is_empty() {
            return Ok(Some(files.swap_remove(0)));
        }
        Ok(None)
    }

    pub fn exists(&self, db: &Database, snapshot: &Snapshot) -> Result<bool, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND path=?2 AND name=?3");
        db.exists(&sql, params![snapshot.id, String::from(self.path.to_str().unwrap()), String::from(self.name.to_str().unwrap())])
    }

    pub fn archive_exists(&self, db: &Database, snapshot: &Snapshot) -> Result<bool, CmdError> {
        let mut sql = String::from(READ_NO_SNAPSHOT);
        sql.push_str(" WHERE deleted_sid=?1 AND path=?2 AND name=?3");
        db.exists(&sql, params![snapshot.id, String::from(self.path.to_str().unwrap()), String::from(self.name.to_str().unwrap())])
    }

    pub fn history(db: &Database, name: &str) -> Result<Vec<File>, CmdError> {
        let path = Self::fqn(&PathBuf::from(name));
        let mut sql = String::from(READ);
        sql.push_str(" WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name REGEXP ?1 ORDER BY path ASC, name ASC");
        File::list(db, &sql, params![path])
    }

    pub fn history_sync_mode(db: &Database, name: &str) -> Result<Vec<File>, CmdError> {
        let path = Self::fqn(&PathBuf::from(name));
        let mut sql = String::from(READ_SYNC_MODE);
        sql.push_str(" WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name REGEXP ?1 ORDER BY path ASC, name ASC");
        File::list(db, &sql, params![path])
    }

    pub fn orphans(db: &Database) -> Result<Vec<File>, CmdError> {
        File::list(db, ORPHANS, params![])
    }

    pub fn distinct_entries(db: &Database) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ_NO_SNAPSHOT);
        sql.push_str(" GROUP BY path, name");
        File::list(db, &sql, params![])
    }

    pub fn deleted_files(db: &Database, previous_snapshot: &Snapshot, snapshot: &Snapshot) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND deleted_sid=?2 AND fv.is_dir=0 ORDER BY fv.path ASC, fv.name ASC");
        File::list(db, &sql, params![previous_snapshot.id, snapshot.id])
    }

    pub fn deleted_files_sync_mode(db: &Database, previous_snapshot: &Snapshot, snapshot: &Snapshot) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ_SYNC_MODE);
        sql.push_str(" WHERE snapshot_id=?1 AND is_dir=0 AND path||name NOT IN (SELECT path||name FROM filehistory WHERE snapshot_id=?2) ORDER BY path ASC, name ASC");
        File::list(db, &sql, params![previous_snapshot.id, snapshot.id])
    }

    pub fn filesystem_dirs(db: &Database) -> Result<Vec<File>, CmdError> {
        Self::filesystem(db, What::Dirs)
    }

    pub fn filesystem_entries(db: &Database) -> Result<Vec<File>, CmdError> {
        Self::filesystem(db, What::All)
    }

    fn filesystem(db: &Database, what: What) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ_NO_SNAPSHOT);
        match what {
            What::All => (),
            What::Dirs => sql.push_str(" WHERE is_dir=1"),
            What::Files => sql.push_str(" WHERE is_dir=0"),
        };
        sql.push_str(" ORDER BY path ASC, name ASC");
        File::list(db, &sql, params![])
    }

    pub fn children_dirs(db: &Database, snapshot: &Snapshot, path: &Path) -> Result<HashMap<PathBuf, File>, CmdError> {
        Self::children(db, snapshot, path, What::Dirs)
    }

    pub fn children_files(db: &Database, snapshot: &Snapshot, path: &Path) -> Result<HashMap<PathBuf, File>, CmdError> {
        Self::children(db, snapshot, path, What::Files)
    }

    fn children(db: &Database, snapshot: &Snapshot, path: &Path, what: What) -> Result<HashMap<PathBuf, File>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND fv.path=?2");
        Self::entry_type(&mut sql, what);
        File::list_by_path(db, &sql, params![snapshot.id, path.to_str().unwrap()])
    }

    pub fn dirs(db: &Database, snapshot: &Snapshot) -> Result<Vec<File>, CmdError> {
        Self::entries(db, snapshot, What::Dirs)
    }

    pub fn files(db: &Database, snapshot: &Snapshot) -> Result<Vec<File>, CmdError> {
        Self::entries(db, snapshot, What::Files)
    }

    fn entries(db: &Database, snapshot: &Snapshot, what: What) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1");
        Self::entry_type(&mut sql, what);
        sql.push_str(" ORDER BY fv.path ASC, fv.name ASC");
        File::list(db, &sql, params![snapshot.id])
    }

    pub fn diff(db: &Database, snapshot: &Snapshot, previous_snapshot: &Snapshot) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ);
        // in previous snapshot, but not in current
        sql.push_str(" WHERE sf.snapshot_id=?2 AND deleted_sid!=0 AND fv.id NOT IN (SELECT version_id FROM snapshotfiles WHERE snapshot_id=?1) UNION ");
        sql.push_str(READ);
        // in current snapshot, but not in previous
        sql.push_str(" WHERE sf.snapshot_id=?1 AND fv.id NOT IN (SELECT version_id FROM snapshotfiles WHERE snapshot_id=?2) ORDER BY fv.path ASC, fv.name");
        File::list(db, &sql, params![snapshot.id, previous_snapshot.id])
    }

    pub fn diff_sync_mode(db: &Database, snapshot: &Snapshot, previous_snapshot: &Snapshot) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ_SYNC_MODE);
        // in previous snapshot, but not in current
        sql.push_str(" WHERE snapshot_id=?2 AND path||name NOT IN (SELECT path||name FROM filehistory WHERE snapshot_id=?1) UNION ");
        sql.push_str(READ_SYNC_MODE);
        // in current snapshot, but not in previous
        sql.push_str(" WHERE snapshot_id=?1 AND path||name NOT IN (SELECT path||name FROM filehistory WHERE snapshot_id=?2) UNION ");
        sql.push_str(READ_SYNC_MODE);
        sql.push_str(" fh WHERE snapshot_id=?1 AND path||name IN (SELECT path||name FROM filehistory WHERE snapshot_id=?2 AND (fh.modified!= modified OR fh.size!= size)) ORDER BY path ASC, name");
        File::list(db, &sql, params![snapshot.id, previous_snapshot.id])
    }

    pub fn dirs_matching(db: &Database, snapshot: &Snapshot, spec: &str) -> Result<Vec<File>, CmdError> {
        Self::matching(db, snapshot.id, spec, What::Dirs)
    }

    pub fn files_matching(db: &Database, snapshot: &Snapshot, spec: &str) -> Result<Vec<File>, CmdError> {
        Self::matching(db, snapshot.id, spec, What::Files)
    }

    pub fn entries_matching(db: &Database, snapshot: &Snapshot, spec: &str) -> Result<Vec<File>, CmdError> {
        Self::matching(db, snapshot.id, spec, What::All)
    }

    fn matching(db: &Database, sid: u64, spec: &str, what: What) -> Result<Vec<File>, CmdError> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND ");
        Self::entry_type(&mut sql, what);
        sql.push_str("fv.path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||fv.name REGEXP ?2");
        File::list(db, &sql, params![sid, spec])
    }

    pub fn insert(&mut self, db: &Database, snapshot: &Snapshot) -> Result<(), CmdError> {
        let id = db.query_one(
            INSERT,
            params![
                self.digest,
                self.path.to_str().unwrap(),
                self.archive.to_str().unwrap(),
                self.name.to_str().unwrap(),
                self.size,
                self.created,
                self.modified,
                self.deleted_sid,
                self.is_dir,
            ],
            |row| row.get(0),
        )?;
        match id {
            Some(id) => {
                self.id = id;
                self.insert_ref(db, snapshot)
            }
            None => Err(CmdError::DbError(String::from(INSERT), String::from("expecting id"))),
        }
    }

    pub fn insert_ref(&self, db: &Database, snapshot: &Snapshot) -> Result<(), CmdError> {
        db.execute(INSERT_REF, params![snapshot.id, self.id])
    }

    pub fn insert_history_sync_mode(db: &Database, snapshot: &Snapshot) -> Result<(), CmdError> {
        db.execute(INSERT_HISTORY_SYNC_MODE, params![snapshot.id])
    }

    pub fn archive(&mut self, db: &Database, snapshot: &Snapshot) -> Result<(), CmdError> {
        self.archive = PathBuf::from(BURST_VERSION_DIR).join(snapshot.id.to_string());
        db.execute(ARCHIVE, params![self.id, self.archive.to_str().unwrap(), self.deleted_sid])
    }

    pub fn unarchive(&mut self, db: &Database) -> Result<(), CmdError> {
        db.execute(ARCHIVE, params![self.id, String::new(), self.deleted_sid])
    }

    pub fn update_ref(&self, db: &Database, file: &File) -> Result<(), CmdError> {
        db.execute(UPDATE_REF, params![file.id, self.id])
    }

    pub fn delete(&self, db: &Database) -> Result<(), CmdError> {
        db.execute(DELETE, params![self.id])
    }

    pub fn delete_ref(&self, db: &Database) -> Result<(), CmdError> {
        let mut sql = String::from(DELETE_REF);
        sql.push_str(" WHERE version_id=?1");
        db.execute(&sql, params![self.id])
    }

    pub fn delete_all_refs_keep_snapshot(db: &Database, sid: u64, path: &Path, name: &Path) -> Result<(), CmdError> {
        let mut sql = String::from(DELETE_REF);
        sql.push_str(" WHERE snapshot_id!=?1");
        if !path.as_os_str().is_empty() && !name.as_os_str().is_empty() {
            sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE path=?2 AND name=?3)");
            db.execute(&sql, params![sid, path.to_str().unwrap(), name.to_str().unwrap()])
        } else if !path.as_os_str().is_empty() {
            sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE path=?2)");
            db.execute(&sql, params![sid, path.to_str().unwrap()])
        } else if !name.as_os_str().is_empty() {
            sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE name=?2)");
            db.execute(&sql, params![sid, name.to_str().unwrap()])
        } else {
            db.execute(&sql, params![sid])
        }
    }

    pub fn delete_all(db: &Database, snapshot: &Snapshot) -> Result<(), CmdError> {
        File::delete_all_by_sid(db, snapshot.id)
    }

    pub fn delete_all_by_sid(db: &Database, sid: u64) -> Result<(), CmdError> {
        db.execute(DELETE_SNAPSHOT, params![sid])
    }

    pub fn delete_files(db: &Database, sid: u64, spec: &str) -> Result<(), CmdError> {
        let mut sql = String::from(DELETE_SNAPSHOT);
        sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name REGEXP ?2)");
        db.execute(&sql, params![sid, spec])
    }

    fn list_by_path<P>(db: &Database, sql: &str, params: P) -> Result<HashMap<PathBuf, File>, CmdError>
    where
        P: Params,
    {
        let files = File::list(db, sql, params)?;
        let mut result: HashMap<PathBuf, File> = HashMap::new();
        for file in files {
            result.insert(file.fullname(), file);
        }
        Ok(result)
    }

    fn list<P>(db: &Database, sql: &str, params: P) -> Result<Vec<File>, CmdError>
    where
        P: Params,
    {
        db.query_list(sql, params, |row| {
            let checksum: String = row.get(2)?;
            let path: String = row.get(3)?;
            let archive: String = row.get(4)?;
            let name: String = row.get(5)?;
            Ok(File {
                id: row.get(0)?,
                sid: row.get(1)?,
                digest: checksum,
                path: PathBuf::from(path),
                archive: PathBuf::from(archive),
                name: PathBuf::from(name),
                size: row.get(6)?,
                created: row.get(7)?,
                modified: row.get(8)?,
                deleted_sid: row.get(9)?,
                is_dir: row.get(10)?,
            })
        })
    }
}
