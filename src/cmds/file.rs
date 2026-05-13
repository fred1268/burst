use crate::cmds::constants::BURST_VERSION_DIR;
use crate::tools::db::Database;
use crate::tools::error::Error;
use crate::tools::fmt::human_readable_size;
use chrono::{DateTime, Local};
use sqlx::query::Query;
use sqlx::sqlite::SqliteArguments;
use sqlx::{Row, Sqlite};
use std::collections::{HashMap, HashSet};
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

const UPDATE: &str = "UPDATE fileversions SET digest=?1, size=?2, modified=?3 WHERE id=?4";

const UPDATE_REF: &str = "UPDATE snapshotfiles SET version_id=?1 WHERE version_id=?2";

const DELETE: &str = "DELETE FROM fileversions WHERE id=?1";

const DELETE_REF: &str = "DELETE FROM snapshotfiles";

const DELETE_SYNC_MODE: &str = "DELETE FROM filehistory";

const DELETE_SNAPSHOT: &str = "DELETE FROM snapshotfiles WHERE snapshot_id=?1";

enum What {
    #[allow(dead_code)]
    All,
    Dirs,
    Files,
}

#[derive(Debug)]
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

impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.name == other.name
    }
}

impl Eq for File {}

impl std::hash::Hash for File {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.name.hash(state);
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

    pub async fn find_entry(db: &Database, name: &Path) -> Result<Option<File>, Error> {
        let path = Self::fqn(name);
        let mut sql = String::from(READ_NO_SNAPSHOT);
        sql.push_str(" WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name=?1");
        let mut files = File::list(db, &sql, |q| q.bind(path)).await?;
        if !files.is_empty() {
            return Ok(Some(files.swap_remove(0)));
        }
        Ok(None)
    }

    pub async fn exists(&self, db: &Database, sid: u64) -> Result<bool, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND path=?2 AND name=?3");
        db.exists(&sql, |q| {
            q.bind(sid as i64).bind(String::from(self.path.to_str().unwrap())).bind(String::from(self.name.to_str().unwrap()))
        })
        .await
    }

    pub async fn archive_exists(&self, db: &Database, sid: u64) -> Result<bool, Error> {
        let mut sql = String::from(READ_NO_SNAPSHOT);
        sql.push_str(" WHERE deleted_sid=?1 AND path=?2 AND name=?3");
        db.exists(&sql, |q| {
            q.bind(sid as i64).bind(String::from(self.path.to_str().unwrap())).bind(String::from(self.name.to_str().unwrap()))
        })
        .await
    }

    pub async fn history(db: &Database, name: &str) -> Result<Vec<File>, Error> {
        let path = Self::fqn(&PathBuf::from(name));
        let mut sql = String::from(READ);
        sql.push_str(" WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name LIKE ?1 ORDER BY path ASC, name ASC"); // TODO: restore REGEXP
        File::list(db, &sql, |q| q.bind(path)).await
    }

    pub async fn history_sync_mode(db: &Database, name: &str) -> Result<Vec<File>, Error> {
        let path = Self::fqn(&PathBuf::from(name));
        let mut sql = String::from(READ_SYNC_MODE);
        sql.push_str(" WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name LIKE ?1 ORDER BY path ASC, name ASC"); // TODO: restore REGEXP
        File::list(db, &sql, |q| q.bind(path)).await
    }

    pub async fn orphans(db: &Database) -> Result<Vec<File>, Error> {
        File::list(db, ORPHANS, |q| q).await
    }

    pub async fn distinct_entries(db: &Database) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ_NO_SNAPSHOT);
        sql.push_str(" GROUP BY path, name");
        File::list(db, &sql, |q| q).await
    }

    pub async fn deleted_files(db: &Database, psid: u64, sid: u64) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND deleted_sid=?2 AND fv.is_dir=0 ORDER BY fv.path ASC, fv.name ASC");
        File::list(db, &sql, |q| q.bind(psid as i64).bind(sid as i64)).await
    }

    pub async fn deleted_files_sync_mode(db: &Database, psid: u64, sid: u64) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ_SYNC_MODE);
        sql.push_str(" WHERE snapshot_id=?1 AND is_dir=0 AND path||name NOT IN (SELECT path||name FROM filehistory WHERE snapshot_id=?2) ORDER BY path ASC, name ASC");
        File::list(db, &sql, |q| q.bind(psid as i64).bind(sid as i64)).await
    }

    pub async fn filesystem_dirs(db: &Database) -> Result<Vec<File>, Error> {
        Self::filesystem(db, What::Dirs).await
    }

    pub async fn filesystem_entries(db: &Database) -> Result<Vec<File>, Error> {
        Self::filesystem(db, What::All).await
    }

    async fn filesystem(db: &Database, what: What) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ_NO_SNAPSHOT);
        match what {
            What::All => (),
            What::Dirs => sql.push_str(" WHERE is_dir=1"),
            What::Files => sql.push_str(" WHERE is_dir=0"),
        };
        sql.push_str(" ORDER BY path ASC, name ASC");
        File::list(db, &sql, |q| q).await
    }

    pub async fn children_dirs(db: &Database, sid: u64, path: &Path) -> Result<HashMap<PathBuf, File>, Error> {
        Self::children(db, sid, path, What::Dirs).await
    }

    pub async fn children_files(db: &Database, sid: u64, path: &Path) -> Result<HashMap<PathBuf, File>, Error> {
        Self::children(db, sid, path, What::Files).await
    }

    async fn children(db: &Database, sid: u64, path: &Path, what: What) -> Result<HashMap<PathBuf, File>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND fv.path=?2");
        Self::entry_type(&mut sql, what);
        File::list_by_path(db, &sql, |q| q.bind(sid as i64).bind(path.to_str().unwrap())).await
    }

    pub async fn all_entries(db: &Database, sid: u64) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1");
        sql.push_str(" ORDER BY fv.path ASC, fv.is_dir DESC, fv.name ASC");
        File::list(db, &sql, |q| q.bind(sid as i64)).await
    }

    pub async fn dirs(db: &Database, sid: u64) -> Result<Vec<File>, Error> {
        Self::entries(db, sid, What::Dirs).await
    }

    pub async fn files(db: &Database, sid: u64) -> Result<Vec<File>, Error> {
        Self::entries(db, sid, What::Files).await
    }

    async fn entries(db: &Database, sid: u64, what: What) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1");
        Self::entry_type(&mut sql, what);
        sql.push_str(" ORDER BY fv.path ASC, fv.name ASC");
        File::list(db, &sql, |q| q.bind(sid as i64)).await
    }

    pub async fn diff(db: &Database, psid: u64, sid: u64) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ);
        // in previous snapshot, but not in current
        sql.push_str(" WHERE sf.snapshot_id=?2 AND deleted_sid!=0 AND fv.id NOT IN (SELECT version_id FROM snapshotfiles WHERE snapshot_id=?1) UNION ");
        sql.push_str(READ);
        // in current snapshot, but not in previous
        sql.push_str(" WHERE sf.snapshot_id=?1 AND fv.id NOT IN (SELECT version_id FROM snapshotfiles WHERE snapshot_id=?2) ORDER BY fv.path ASC, fv.name");
        File::list(db, &sql, |q| q.bind(sid as i64).bind(psid as i64)).await
    }

    pub async fn diff_sync_mode(db: &Database, psid: u64, sid: u64) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ_SYNC_MODE);
        // in previous snapshot, but not in current
        sql.push_str(" WHERE snapshot_id=?2 AND path||name NOT IN (SELECT path||name FROM filehistory WHERE snapshot_id=?1) UNION ");
        sql.push_str(READ_SYNC_MODE);
        // in current snapshot, but not in previous
        sql.push_str(" WHERE snapshot_id=?1 AND path||name NOT IN (SELECT path||name FROM filehistory WHERE snapshot_id=?2) UNION ");
        sql.push_str(READ_SYNC_MODE);
        sql.push_str(" fh WHERE snapshot_id=?1 AND path||name IN (SELECT path||name FROM filehistory WHERE snapshot_id=?2 AND (fh.modified!= modified OR fh.size!= size)) ORDER BY path ASC, name");
        File::list(db, &sql, |q| q.bind(sid as i64).bind(psid as i64)).await
    }

    pub async fn dirs_matching(db: &Database, sid: u64, spec: &str) -> Result<Vec<File>, Error> {
        Self::matching(db, sid, spec, What::Dirs).await
    }

    pub async fn files_matching(db: &Database, sid: u64, spec: &str) -> Result<Vec<File>, Error> {
        Self::matching(db, sid, spec, What::Files).await
    }

    pub async fn entries_matching(db: &Database, sid: u64, spec: &str) -> Result<Vec<File>, Error> {
        Self::matching(db, sid, spec, What::All).await
    }

    async fn matching(db: &Database, sid: u64, spec: &str, what: What) -> Result<Vec<File>, Error> {
        let mut sql = String::from(READ);
        sql.push_str(" WHERE sf.snapshot_id=?1 AND ");
        Self::entry_type(&mut sql, what);
        sql.push_str("fv.path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||fv.name LIKE ?2"); // TODO: restore REGEXP
        File::list(db, &sql, |q| q.bind(sid as i64).bind(spec)).await
    }

    pub async fn insert(&mut self, db: &Database, sid: u64) -> Result<(), Error> {
        let id = db
            .query_one(
                INSERT,
                |q| {
                    q.bind(self.digest.clone())
                        .bind(self.path.to_str().unwrap())
                        .bind(self.archive.to_str().unwrap())
                        .bind(self.name.to_str().unwrap())
                        .bind(self.size as i64)
                        .bind(self.created)
                        .bind(self.modified)
                        .bind(self.deleted_sid as i64)
                        .bind(self.is_dir)
                },
                |row| row.try_get::<i64, _>(0),
            )
            .await?;
        match id {
            Some(id) => {
                self.id = id as u64;
                self.insert_ref(db, sid).await
            }
            None => Err(Error::GenericError(format!("Expecting id after {}", INSERT))),
        }
    }

    pub async fn insert_ref(&self, db: &Database, sid: u64) -> Result<(), Error> {
        db.execute(INSERT_REF, |q| q.bind(sid as i64).bind(self.id as i64)).await
    }

    pub async fn insert_history_sync_mode(db: &Database, sid: u64) -> Result<(), Error> {
        db.execute(INSERT_HISTORY_SYNC_MODE, |q| q.bind(sid as i64)).await
    }

    pub async fn archive(&mut self, db: &Database, sid: u64) -> Result<(), Error> {
        self.archive = PathBuf::from(BURST_VERSION_DIR).join(sid.to_string());
        db.execute(ARCHIVE, |q| q.bind(self.id as i64).bind(self.archive.to_str().unwrap()).bind(self.deleted_sid as i64)).await
    }

    pub async fn unarchive(&mut self, db: &Database) -> Result<(), Error> {
        db.execute(ARCHIVE, |q| q.bind(self.id as i64).bind(String::new()).bind(self.deleted_sid as i64)).await
    }

    pub async fn update(&self, db: &Database) -> Result<(), Error> {
        db.execute(UPDATE, |q| q.bind(self.digest.clone()).bind(self.size as i64).bind(self.modified).bind(self.id as i64)).await
    }

    pub async fn update_ref(&self, db: &Database, file: &File) -> Result<(), Error> {
        db.execute(UPDATE_REF, |q| q.bind(file.id as i64).bind(self.id as i64)).await
    }

    pub async fn delete(&self, db: &Database) -> Result<(), Error> {
        db.execute(DELETE, |q| q.bind(self.id as i64)).await
    }

    pub async fn delete_ref(&self, db: &Database) -> Result<(), Error> {
        let mut sql = String::from(DELETE_REF);
        sql.push_str(" WHERE version_id=?1");
        db.execute(&sql, |q| q.bind(self.id as i64)).await
    }

    pub async fn delete_sync_mode(db: &Database, sid: u64) -> Result<(), Error> {
        let mut sql = String::from(DELETE_SYNC_MODE);
        sql.push_str(" WHERE snapshot_id=?1");
        db.execute(&sql, |q| q.bind(sid as i64)).await
    }

    pub async fn delete_all_refs_keep_snapshot(db: &Database, sid: u64, path: &Path, name: &Path) -> Result<(), Error> {
        let mut sql = String::from(DELETE_REF);
        sql.push_str(" WHERE snapshot_id!=?1");
        if !path.as_os_str().is_empty() && !name.as_os_str().is_empty() {
            sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE path=?2 AND name=?3)");
            db.execute(&sql, |q| q.bind(sid as i64).bind(path.to_str().unwrap()).bind(name.to_str().unwrap())).await
        } else if !path.as_os_str().is_empty() {
            sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE path=?2)");
            db.execute(&sql, |q| q.bind(sid as i64).bind(path.to_str().unwrap())).await
        } else if !name.as_os_str().is_empty() {
            sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE name=?2)");
            db.execute(&sql, |q| q.bind(sid as i64).bind(name.to_str().unwrap())).await
        } else {
            db.execute(&sql, |q| q.bind(sid as i64)).await
        }
    }

    pub async fn delete_all(db: &Database, sid: u64) -> Result<(), Error> {
        File::delete_all_by_sid(db, sid).await
    }

    pub async fn delete_all_by_sid(db: &Database, sid: u64) -> Result<(), Error> {
        db.execute(DELETE_SNAPSHOT, |q| q.bind(sid as i64)).await
    }

    pub async fn delete_files(db: &Database, sid: u64, spec: &str) -> Result<(), Error> {
        let mut sql = String::from(DELETE_SNAPSHOT);
        sql.push_str(" AND version_id IN (SELECT id FROM fileversions WHERE path||'");
        sql.push(MAIN_SEPARATOR);
        sql.push_str("'||name LIKE ?2)"); // TODO: restore REGEXP
        db.execute(&sql, |q| q.bind(sid as i64).bind(spec)).await
    }

    async fn list_by_path<'a, B>(db: &Database, sql: &'a str, bind: B) -> Result<HashMap<PathBuf, File>, Error>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
    {
        let files = File::list(db, sql, bind).await?;
        let mut result: HashMap<PathBuf, File> = HashMap::new();
        for file in files {
            result.insert(file.fullname(), file);
        }
        Ok(result)
    }

    async fn list<'a, B>(db: &Database, sql: &'a str, bind: B) -> Result<Vec<File>, Error>
    where
        B: FnOnce(Query<'a, Sqlite, SqliteArguments<'a>>) -> Query<'a, Sqlite, SqliteArguments<'a>>,
    {
        db.query_list(sql, bind, |row| {
            let checksum: String = row.try_get(2)?;
            let path: String = row.try_get(3)?;
            let archive: String = row.try_get(4)?;
            let name: String = row.try_get(5)?;
            Ok(File {
                id: row.try_get::<i64, _>(0)? as u64,
                sid: row.try_get::<i64, _>(1)? as u64,
                digest: checksum,
                path: PathBuf::from(path),
                archive: PathBuf::from(archive),
                name: PathBuf::from(name),
                size: row.try_get::<i64, _>(6)? as u64,
                created: row.try_get(7)?,
                modified: row.try_get(8)?,
                deleted_sid: row.try_get::<i64, _>(9)? as u64,
                is_dir: row.try_get(10)?,
            })
        })
        .await
    }
}

#[derive(Debug)]
pub struct Directory {
    pub entry: File,
    pub files: HashSet<File>,
    pub children: HashSet<Directory>,
}

impl fmt::Display for Directory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.entry.fullname())
    }
}

impl PartialEq for Directory {
    fn eq(&self, other: &Self) -> bool {
        self.entry.name == other.entry.name
    }
}

impl Eq for Directory {}

impl std::hash::Hash for Directory {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.entry.name.hash(state);
    }
}

impl Directory {
    pub fn from_parts(entry: File, files: HashSet<File>, children: HashSet<Directory>) -> Self {
        Directory { entry, files, children }
    }
}
