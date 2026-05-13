use crate::args::backup::BackupArgs;
use crate::cmds::command::{self, Command};
use crate::cmds::file::{Directory, File};
use crate::cmds::snapshot::Snapshot;
use crate::tools::cmderror::CmdError::{self, InvalidBackupDirectory, InvalidOption};
use crate::tools::cmderror::IoError;
use crate::tools::db::Database;
use crate::tools::fs::FileSystem;
use chrono::{DateTime, Local};
use std::boxed::Box;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

#[derive(Default)]
struct Statistics {
    pub dirs: u64,
    pub excl_dirs: u64,
    pub excl_files: u64,
    pub has_files: bool,
}

type ReadDirectoryResult = Result<(Directory, Statistics), CmdError>;

#[derive(Default)]
struct TreeDiff {
    pub dir_added: Vec<Directory>,
    pub dir_deleted: Vec<Directory>,
    pub dir_unchanged: Vec<File>,
    pub file_added: Vec<File>,
    pub file_deleted: Vec<File>,
    pub file_modified: Vec<(File, File)>,
    pub file_unchanged: Vec<File>,
}

pub struct BackupCommand {
    args: BackupArgs,
    previous_snapshot: Snapshot,
}

impl Default for BackupCommand {
    fn default() -> Self {
        BackupCommand::from(BackupArgs::default())
    }
}

impl From<BackupArgs> for BackupCommand {
    fn from(args: BackupArgs) -> Self {
        BackupCommand { args, previous_snapshot: Snapshot::default() }
    }
}

impl Command for BackupCommand {
    fn validate(&mut self) -> Result<(), CmdError> {
        Ok(())
    }

    fn help(&self) {
        println!("Usage: {} backup [OPTIONS] <BACKUP_PATH>", self.args.exe);
        println!();
        println!("Backup files to specified directory using this directory's configuration.");
        println!();
        println!("Options:");
        println!("\t-c, --continue\t\t\t\tcontinue an interrupted backup process");
        println!("\t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)");
        println!("\t-v, --verbose\t\t\t\tdisplay more detailed information");
        println!("\t-n, --dry-run\t\t\t\tdon't actually touch the filesystem, do a dry run instead");
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + '_>> {
        Box::pin(async move {
            let start = Instant::now();
            if self.args.verbose {
                println!("backup command started");
                println!("Running {}", self.args);
            }
            match command::start(&self.args.config.target).await {
                Ok(_) => (),
                Err(err) => match err {
                    CmdError::NoRemote() => {
                        if !self.args.dry_run {
                            return Err(InvalidBackupDirectory());
                        }
                    }
                    _ => return Err(err),
                },
            };
            self.args.config.read(&command::config_file(&self.args.config.target)).await?;
            let db = Database::open(&self.args.config.target).await?;
            let mut snapshot = match self.args.dry_run {
            true => Some(Snapshot::default()),
            false => match self.args.cont {
                true => {
                    let inprogress = Snapshot::get_in_progress(&db).await?;
                    if inprogress.is_none() {
                        return Err(InvalidOption(String::from("Cannot use --continue when no backup has been interrupted")));
                    }
                    inprogress
                }
                false => {
                    if let Some(snapshot_ip) = Snapshot::get_in_progress(&db).await? {
                        return Err(InvalidOption(format!("A previous backup has been interrupted: use --continue to resume it.\nAlternatively, use the 'delete --snapshot {}' command to remove it", snapshot_ip.id)));
                    }
                    Snapshot::insert_next(&db).await?
                }
            },
        }
        .unwrap_or_default();
            if self.args.dry_run {
                if let Some(ps) = Snapshot::get_latest(&db).await? {
                    self.previous_snapshot = ps;
                }
            } else if let Some(ps) = Snapshot::get_previous(&db, snapshot.id).await? {
                self.previous_snapshot = ps;
            }

            self.do_backup(&db, &mut snapshot).await?;

            if !self.args.dry_run && !self.args.config.incremental {
                File::delete_all(&db, self.previous_snapshot.id).await?;
                self.previous_snapshot.delete(&db).await?;
            }
            snapshot.duration = start.elapsed();
            snapshot.status = String::from("success");
            if !self.args.dry_run {
                snapshot.mark_completed(&db).await?;
            }
            if !self.args.config.incremental {
                File::insert_history_sync_mode(&db, snapshot.id).await?;
            }
            if !self.args.quiet {
                println!();
                Snapshot::header();
                println!("{}", snapshot);
            }
            command::stop(&self.args.config.target).await
        })
    }
}

impl BackupCommand {
    // Operation order and failure analysis (--continue recovery):
    //
    // | Operation             | Order       | DB fails                          | FS fails                                     |
    // |-----------------------|-------------|-----------------------------------|----------------------------------------------|
    // | insert_unchanged_file | DB only     | nothing done → retries ✓          | —                                            |
    // | archive_dir           | DB only     | nothing done → retries ✓          | —                                            |
    // | remove_previous_file  | DB only     | nothing done → retries ✓          | —                                            |
    // | archive_file          | DB → FS     | nothing done → retries ✓          | DB updated, rename not done → --continue     |
    // |                       |             |                                   | checks fs.exists() at archive path: false →  |
    // |                       |             |                                   | retries (DB update idempotent, rename ok) ✓  |
    // | insert_new_file       | FS → DB     | orphan file on disk → retries ✓   | nothing done → retries ✓                     |
    // | remove_file           | DB → FS     | orphan file on disk, backup ok ✓  | —                                            |
    // | remove_dir            | DB → FS     | orphan dir on disk, backup ok ✓   | —                                            |
    //
    // insert_new_file is intentionally FS-first (digest → copy → insert) so that a DB failure
    // leaves an orphan file on disk rather than a phantom DB entry. The orphan is harmless:
    // --continue sees file.exists()=false and retries, re-copying over the orphan.
    //
    // archive_file uses DB-first so a DB failure leaves nothing done (clean retry). The FS failure
    // case (rename failed after DB update) is recovered by --continue via an extra fs.exists() check
    // in process_deleted_file: archive_exists()=true but fs.exists()=false → retries the rename.

    async fn insert_new_file(&self, db: &Database, fs: &FileSystem, sid: u64, file: &mut File) -> Result<(), CmdError> {
        file.digest = fs.compute_digest(file, &self.args.config.source).await?;
        fs.copy_new_file(file, self.args.config.hash_comparison).await?;
        file.insert(db, sid).await
    }

    async fn insert_unchanged_file(&self, db: &Database, _fs: &FileSystem, sid: u64, file: &File) -> Result<(), CmdError> {
        file.insert_ref(db, sid).await
    }

    async fn archive_file(&self, db: &Database, fs: &FileSystem, sid: u64, file: &mut File) -> Result<(), CmdError> {
        file.archive(db, sid).await?;
        fs.archive_file(sid, file).await
    }

    async fn archive_dir(&self, db: &Database, _fs: &FileSystem, sid: u64, dir: &mut File) -> Result<(), CmdError> {
        dir.archive(db, sid).await
    }

    async fn remove_previous_file(&self, db: &Database, _fs: &FileSystem, previous_file: &File, file: &File) -> Result<(), CmdError> {
        previous_file.update_ref(db, file).await?;
        previous_file.delete(db).await
    }

    async fn remove_file(&self, db: &Database, fs: &FileSystem, file: &File) -> Result<(), CmdError> {
        file.delete_ref(db).await?;
        file.delete(db).await?;
        fs.remove_file(file).await
    }

    async fn remove_dir(&self, db: &Database, fs: &FileSystem, dir: &File) -> Result<(), CmdError> {
        dir.delete_ref(db).await?;
        dir.delete(db).await?;
        fs.remove_dir(dir).await
    }

    async fn do_backup(&self, db: &Database, snapshot: &mut Snapshot) -> Result<(), CmdError> {
        let (dir, statistics) =
            Self::read_source(Arc::new(self.args.clone()), self.previous_snapshot.id, self.args.config.source.clone()).await?;
        if !statistics.has_files && !self.args.quiet {
            println!("Warning: backup is empty");
        }
        snapshot.dirs = statistics.dirs;
        snapshot.excluded_dirs = statistics.excl_dirs;
        snapshot.excluded_files = statistics.excl_files;
        let prev_dir = self.read_previous_source(db).await?;
        let diff = self.compare_tree(dir, prev_dir);
        self.compute_statistics(snapshot, &diff);
        self.compute_tree(db, snapshot.id, diff).await?;
        Ok(())
    }

    fn read_source(args: Arc<BackupArgs>, pid: u64, root: PathBuf) -> Pin<Box<dyn Future<Output = ReadDirectoryResult> + Send + 'static>> {
        Box::pin(async move {
            let mut statistics = Statistics::default();
            statistics.dirs += 1;
            if !args.quiet && pid == 0 {
                println!("Directory {:?}", root);
            }
            let mut subdirs: Vec<PathBuf> = vec![];
            let mut files = HashSet::new();
            let mut entries = tokio::fs::read_dir(root.clone())
                .await
                .map_err(|err| CmdError::IoError(IoError::from_str("Cannot iterate entries", err)))?;
            let root_metadata = tokio::fs::symlink_metadata(&root).await.map_err(|err| CmdError::IoError(IoError::from(&root, err)))?;
            while let Some(entry) = entries.next_entry().await.map_err(|err| CmdError::IoError(IoError::from_str("Invalid entry", err)))? {
                let p = entry.path();
                let metadata = tokio::fs::symlink_metadata(&p).await.map_err(|err| CmdError::IoError(IoError::from(&p, err)))?;
                if !args.config.follow_symlinks && metadata.is_symlink() {
                    if args.verbose {
                        println!("  Excluded symlink {:?}", entry.file_name())
                    }
                    continue;
                }
                if args.config.is_excluded(&p) {
                    if metadata.is_dir() {
                        statistics.excl_dirs += 1;
                        if args.verbose {
                            println!("Excluded directory {:?}", p)
                        }
                    } else {
                        statistics.excl_files += 1;
                        if args.verbose {
                            println!("  Excluded file {:?}", p.file_name().unwrap())
                        }
                    }
                    continue;
                }
                if p.is_dir() {
                    subdirs.push(p);
                    continue;
                }
                statistics.has_files = true;
                files.insert(File::from_metadata(
                    p.strip_prefix(&args.config.source).map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", p)))?,
                    metadata,
                ));
            }
            let mut set = JoinSet::new();
            for subdir in subdirs {
                set.spawn(Self::read_source(Arc::clone(&args), pid, subdir));
            }
            let mut children = HashSet::new();
            while let Some(dir) = set.join_next().await {
                let (dir, stats) = dir.map_err(|_| CmdError::GenericError(String::from("Cannot join tokio tasks")))??;
                statistics.dirs += stats.dirs;
                statistics.excl_dirs += stats.excl_dirs;
                statistics.excl_files += stats.excl_files;
                statistics.has_files |= stats.has_files;
                children.insert(dir);
            }
            Ok((
                Directory::from_parts(
                    File::from_metadata(
                        root.strip_prefix(&args.config.source)
                            .map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", root)))?,
                        root_metadata,
                    ),
                    files,
                    children,
                ),
                statistics,
            ))
        })
    }

    fn recurse_build_directory(&self, by_path: &mut HashMap<PathBuf, Vec<File>>, entry: File) -> Directory {
        let entries = by_path.remove(&entry.fullname()).unwrap_or_default();
        let mut files = HashSet::new();
        let mut children = HashSet::new();
        for e in entries {
            if e.is_dir {
                children.insert(self.recurse_build_directory(by_path, e));
            } else {
                files.insert(e);
            }
        }
        Directory { entry, files, children }
    }

    fn build_directory(&self, by_path: &mut HashMap<PathBuf, Vec<File>>) -> Directory {
        let date = DateTime::from_timestamp_secs(0).unwrap();
        let file = File {
            id: 0,
            sid: 0,
            digest: String::new(),
            path: PathBuf::new(),
            archive: PathBuf::new(),
            name: PathBuf::new(),
            size: 0,
            created: DateTime::with_timezone(&date, &Local),
            modified: DateTime::with_timezone(&date, &Local),
            deleted_sid: 0,
            is_dir: true,
        };
        self.recurse_build_directory(by_path, file)
    }

    async fn read_previous_source(&self, db: &Database) -> Result<Directory, CmdError> {
        let entries = File::all_entries(db, self.previous_snapshot.id).await?;
        let mut by_path: HashMap<PathBuf, Vec<File>> = HashMap::new();
        for entry in entries {
            by_path.entry(entry.path.clone()).or_default().push(entry);
        }
        Ok(self.build_directory(&mut by_path))
    }

    fn recurse_compare_tree(
        &self, src: Directory, mut prev_files: HashSet<File>, mut prev_children: HashSet<Directory>, diff: &mut TreeDiff,
    ) {
        for file in src.files {
            if let Some(prev_file) = prev_files.take(&file) {
                if prev_file.size != file.size || prev_file.modified != file.modified {
                    diff.file_modified.push((prev_file, file));
                } else {
                    diff.file_unchanged.push(prev_file);
                }
                continue;
            }
            diff.file_added.push(file);
        }
        diff.file_deleted.extend(prev_files.drain());
        for child in src.children {
            if let Some(Directory { entry: prev_entry, files: prev_files, children: prev_children }) = prev_children.take(&child) {
                self.recurse_compare_tree(child, prev_files, prev_children, diff);
                diff.dir_unchanged.push(prev_entry);
                continue;
            }
            diff.dir_added.push(child);
        }
        diff.dir_deleted.extend(prev_children.drain());
    }

    fn compare_tree(&self, src: Directory, prev: Directory) -> TreeDiff {
        let mut diff = TreeDiff::default();
        let Directory { entry: _, files: prev_files, children: prev_children } = prev;
        self.recurse_compare_tree(src, prev_files, prev_children, &mut diff);
        diff
    }

    async fn process_new_files(&self, db: &Database, fs: &FileSystem, sid: u64, files: Vec<File>) -> Result<(), CmdError> {
        for mut file in files {
            if self.args.verbose || self.previous_snapshot.id != 0 {
                println!("  New file {:?}", file.fullname())
            }
            if !self.args.dry_run && (!self.args.cont || !file.exists(db, sid).await?) {
                self.insert_new_file(db, fs, sid, &mut file).await?;
            }
        }
        Ok(())
    }

    async fn process_deleted_files(&self, db: &Database, fs: &FileSystem, sid: u64, files: Vec<File>) -> Result<(), CmdError> {
        for mut file in files {
            file.deleted_sid = sid;
            if self.args.verbose || self.previous_snapshot.id != 0 {
                println!("  Deleted file {:?}", file.fullname())
            }
            if !self.args.dry_run {
                if self.args.config.is_incremental(&file.fullname()) {
                    if !self.args.cont || !file.archive_exists(db, sid).await? || !fs.exists(&file, &self.args.config.target).await? {
                        self.archive_file(db, fs, sid, &mut file).await?;
                    }
                } else if !self.args.cont || file.exists(db, sid).await? {
                    self.remove_file(db, fs, &file).await?;
                }
            }
        }
        Ok(())
    }

    fn process_new_dirs<'a>(
        &'a self, db: &'a Database, fs: &'a FileSystem, sid: u64, mut dir: Directory,
    ) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + Send + '_>> {
        Box::pin(async move {
            if !self.args.dry_run && (!self.args.cont || !dir.entry.exists(db, sid).await?) {
                dir.entry.insert(db, sid).await?;
            }
            let files: Vec<File> = dir.files.into_iter().collect();
            self.process_new_files(db, fs, sid, files).await?;
            let dirs: Vec<Directory> = dir.children.into_iter().collect();
            for dir in dirs {
                self.process_new_dirs(db, fs, sid, dir).await?;
            }
            Ok(())
        })
    }

    fn process_deleted_dirs<'a>(
        &'a self, db: &'a Database, fs: &'a FileSystem, sid: u64, mut dir: Directory,
    ) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + Send + '_>> {
        Box::pin(async move {
            if self.args.verbose || self.previous_snapshot.id != 0 {
                println!("Deleted dir {:?}", dir.entry.fullname())
            }
            let files: Vec<File> = dir.files.into_iter().collect();
            self.process_deleted_files(db, fs, sid, files).await?;
            let dirs: Vec<Directory> = dir.children.into_iter().collect();
            for dir in dirs {
                self.process_deleted_dirs(db, fs, sid, dir).await?;
            }
            if !self.args.dry_run {
                if self.args.config.is_incremental(&dir.entry.fullname()) {
                    if !self.args.cont || !dir.entry.archive_exists(db, sid).await? {
                        self.archive_dir(db, fs, sid, &mut dir.entry).await?;
                    }
                } else if !self.args.cont || dir.entry.exists(db, sid).await? {
                    self.remove_dir(db, fs, &dir.entry).await?;
                }
            }
            Ok(())
        })
    }

    fn compute_statistics(&self, snapshot: &mut Snapshot, diff: &TreeDiff) {
        snapshot.count += diff.file_added.len() as u64;
        snapshot.new_count += diff.file_added.len() as u64;
        for file in &diff.file_added {
            snapshot.size += file.size;
            snapshot.new_size += file.size;
        }
        snapshot.count += diff.file_modified.len() as u64;
        snapshot.modified_count += diff.file_modified.len() as u64;
        for (_, file) in &diff.file_modified {
            snapshot.size += file.size;
            snapshot.modified_size += file.size;
        }
        snapshot.count += diff.file_unchanged.len() as u64;
        snapshot.unchanged_count += diff.file_unchanged.len() as u64;
        for previous_file in &diff.file_unchanged {
            snapshot.size += previous_file.size;
            snapshot.unchanged_size += previous_file.size;
        }
        snapshot.count += diff.file_deleted.len() as u64;
        snapshot.deleted_count += diff.file_deleted.len() as u64;
        for file in &diff.file_deleted {
            snapshot.size += file.size;
            snapshot.deleted_size += file.size;
        }
    }

    async fn compute_tree(&self, db: &Database, sid: u64, diff: TreeDiff) -> Result<(), CmdError> {
        let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
        self.process_new_files(db, &fs, sid, diff.file_added).await?;
        for (mut previous_file, mut file) in diff.file_modified {
            if self.args.verbose || self.previous_snapshot.id != 0 {
                println!("  Modified file {:?}", previous_file.fullname())
            }
            if !self.args.dry_run {
                if self.args.config.is_incremental(&file.fullname()) {
                    if !self.args.cont || !file.exists(db, sid).await? {
                        self.archive_file(db, &fs, sid, &mut previous_file).await?;
                        self.insert_new_file(db, &fs, sid, &mut file).await?;
                    }
                } else if !self.args.cont || !file.exists(db, sid).await? {
                    self.insert_new_file(db, &fs, sid, &mut file).await?;
                    self.remove_previous_file(db, &fs, &previous_file, &file).await?;
                }
            }
        }
        for previous_file in diff.file_unchanged {
            if !self.args.dry_run && (!self.args.cont || !previous_file.exists(db, sid).await?) {
                self.insert_unchanged_file(db, &fs, sid, &previous_file).await?;
            }
        }
        self.process_deleted_files(db, &fs, sid, diff.file_deleted).await?;

        for dir in diff.dir_unchanged {
            if !self.args.dry_run && (!self.args.cont || !dir.exists(db, sid).await?) {
                dir.insert_ref(db, sid).await?;
            }
        }
        for dir in diff.dir_added {
            self.process_new_dirs(db, &fs, sid, dir).await?;
        }
        for dir in diff.dir_deleted {
            self.process_deleted_dirs(db, &fs, sid, dir).await?;
        }
        Ok(())
    }
}
