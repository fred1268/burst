use crate::args::backup::BackupArgs;
use crate::cmds::command::{self, Command};
use crate::cmds::file::{Directory, File};
use crate::cmds::snapshot::Snapshot;
use crate::tools::cmderror::CmdError::{self, InvalidBackupDirectory, InvalidOption};
use crate::tools::cmderror::IoError;
use crate::tools::db::Database;
use crate::tools::fs::FileSystem;
use std::boxed::Box;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

#[derive(Default)]
struct Statistics {
    pub dirs: u64,
    pub files: u64,
    pub has_files: bool,
}

type ReadDirectoryResult = Result<(Directory, Statistics), CmdError>;

#[derive(Default)]
struct Todo {
    pub dir_added: Vec<Directory>,
    pub dir_deleted: Vec<Directory>,
    pub file_added: Vec<File>,
    pub file_deleted: Vec<File>,
    pub file_modified: Vec<(File, File)>,
}

impl Todo {
    pub fn display(&self) {
        if !self.dir_added.is_empty() {
            println!("Added directories:");
            for dir in &self.dir_added {
                println!("  {:?}", dir.name);
            }
        }
        if !self.dir_deleted.is_empty() {
            println!("Deleted directories:");
            for dir in &self.dir_deleted {
                println!("  {:?}", dir.name);
            }
        }
        if !self.file_added.is_empty() {
            println!("Added files:");
            for file in &self.file_added {
                println!("  {:?}", file.fullname());
            }
        }
        if !self.file_deleted.is_empty() {
            println!("Deleted files:");
            for file in &self.file_deleted {
                println!("  {:?}", file.fullname());
            }
        }
        if !self.file_modified.is_empty() {
            println!("Modified files:");
            for (prev, new) in &self.file_modified {
                println!("  {:?} -> {:?}", prev.fullname(), new.fullname());
            }
        }
    }
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

    fn run(&mut self) -> Result<(), CmdError> {
        let start = Instant::now();
        if self.args.verbose {
            println!("backup command started");
            println!("Running {}", self.args);
        }
        match command::start(&self.args.config.target) {
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
        self.args.config.read(&command::config_file(&self.args.config.target))?;
        let db = Database::open(&self.args.config.target)?;
        let mut snapshot = match self.args.dry_run {
            true => Some(Snapshot::default()),
            false => match self.args.cont {
                true => {
                    let inprogress = Snapshot::get_in_progress(&db)?;
                    if inprogress.is_none() {
                        return Err(InvalidOption(String::from("Cannot use --continue when no backup has been interrupted")));
                    }
                    inprogress
                }
                false => {
                    if let Some(snapshot_ip) = Snapshot::get_in_progress(&db)? {
                        return Err(InvalidOption(format!("A previous backup has been interrupted: use --continue to resume it.\nAlternatively, use the 'delete --snapshot {}' command to remove it", snapshot_ip.id)));
                    }
                    Snapshot::insert_next(&db)?
                }
            },
        }
        .unwrap_or_default();
        if self.args.dry_run {
            if let Some(ps) = Snapshot::get_latest(&db)? {
                self.previous_snapshot = ps;
            }
        } else if let Some(ps) = Snapshot::get_previous(&db, snapshot.id)? {
            self.previous_snapshot = ps;
        }

        if self.args.parallel {
            self.async_do_backup(&db, &mut snapshot)?;
        } else {
            self.sync_do_backup(&db, &mut snapshot)?;
        }

        if !self.args.dry_run && !self.args.config.incremental {
            File::delete_all(&db, &self.previous_snapshot)?;
            self.previous_snapshot.delete(&db)?;
        }
        snapshot.duration = start.elapsed();
        snapshot.status = String::from("success");
        if !self.args.dry_run {
            snapshot.mark_completed(&db)?;
        }
        if !self.args.config.incremental {
            File::insert_history_sync_mode(&db, &snapshot)?;
        }
        if !self.args.quiet {
            println!();
            Snapshot::header();
            println!("{}", snapshot);
        }
        command::stop(&self.args.config.target)
    }
}

impl BackupCommand {
    fn sync_do_backup(&self, db: &Database, snapshot: &mut Snapshot) -> Result<(), CmdError> {
        let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
        if !self.process_directory(db, &fs, snapshot, &self.args.config.source)? && !self.args.quiet {
            println!("Warning: backup is empty");
        }
        Ok(())
    }

    fn process_directory(&self, db: &Database, fs: &FileSystem, snapshot: &mut Snapshot, root: &Path) -> Result<bool, CmdError> {
        snapshot.dirs += 1;
        if !self.args.quiet && self.previous_snapshot.id == 0 {
            println!("Directory {:?}", root);
        }
        let mut previous_children = File::children_dirs(
            db,
            &self.previous_snapshot,
            root.strip_prefix(&self.args.config.source).map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", root)))?,
        )?;
        let mut children: Vec<PathBuf> = vec![];
        let mut files: Vec<File> = vec![];
        let entries = fs::read_dir(root).map_err(|err| CmdError::IoError(IoError::from_str("Cannot iterate entries", err)))?;
        let mut contains_files = false;
        for entry in entries {
            let entry = entry.map_err(|err| CmdError::IoError(IoError::from_str("Invalid entry", err)))?;
            let p = entry.path();
            let metadata = fs::symlink_metadata(&p).map_err(|err| CmdError::IoError(IoError::from(&p, err)))?;
            if !self.args.config.follow_symlinks && metadata.is_symlink() {
                if self.args.verbose {
                    println!("  Excluded symlink {:?}", entry.file_name())
                }
                continue;
            }
            if self.args.config.is_excluded(&p) {
                if metadata.is_dir() {
                    snapshot.excluded_dirs += 1;
                    if self.args.verbose {
                        println!("Excluded directory {:?}", p)
                    }
                } else {
                    snapshot.excluded_files += 1;
                    if self.args.verbose {
                        println!("  Excluded file {:?}", p.file_name().unwrap())
                    }
                }
                continue;
            }
            if p.is_dir() {
                let name = PathBuf::from(
                    p.strip_prefix(&self.args.config.source)
                        .map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", p)))?,
                );
                if previous_children.contains_key(&name) {
                    previous_children.remove(&name).unwrap();
                }
                children.push(p);
                continue;
            }
            contains_files = true;
            files.push(File::from_metadata(
                p.strip_prefix(&self.args.config.source).map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", p)))?,
                metadata,
            ));
        }
        for (_, mut child) in previous_children {
            self.process_deleted_dir(db, fs, snapshot, &mut child)?;
        }
        self.process_files(
            db,
            fs,
            snapshot,
            root.strip_prefix(&self.args.config.source).map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", root)))?,
            files,
        )?;
        let mut n = children.len();
        for child in &children {
            if self.process_directory(db, fs, snapshot, child)? {
                let name = child
                    .strip_prefix(&self.args.config.source)
                    .map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", child)))?;
                match File::find_entry(db, name)? {
                    Some(dir) => {
                        if !self.args.dry_run && (!self.args.cont || !dir.exists(db, snapshot)?) {
                            dir.insert_ref(db, snapshot)?;
                        }
                    }
                    None => {
                        let metadata = fs::symlink_metadata(child).map_err(|err| CmdError::IoError(IoError::from(child, err)))?;
                        let dir = &mut File::from_metadata(name, metadata);
                        if !self.args.dry_run && (!self.args.cont || !dir.exists(db, snapshot)?) {
                            dir.insert(db, snapshot)?;
                        }
                    }
                }
            } else {
                n -= 1;
            }
        }
        Ok(contains_files || n != 0)
    }

    fn process_files(&self, db: &Database, fs: &FileSystem, stats: &mut Snapshot, dir: &Path, files: Vec<File>) -> Result<(), CmdError> {
        let mut previous_files = File::children_files(db, &self.previous_snapshot, dir)?;
        for mut file in files {
            if let Some(previous_file) = previous_files.get_mut(&file.fullname()) {
                self.process_existing_file(db, fs, stats, previous_file, &mut file)?;
                previous_files.remove(&file.fullname());
                continue;
            }
            self.process_new_file(db, fs, stats, &mut file)?;
        }
        for mut file in previous_files.into_values() {
            self.process_deleted_file(db, fs, stats, &mut file)?;
        }
        Ok(())
    }

    fn process_new_file(&self, db: &Database, fs: &FileSystem, snapshot: &mut Snapshot, file: &mut File) -> Result<(), CmdError> {
        snapshot.count += 1;
        snapshot.size += file.size;
        snapshot.new_count += 1;
        snapshot.new_size += file.size;
        if self.args.verbose || self.previous_snapshot.id != 0 {
            println!("  New file {:?}", file.fullname())
        }
        if !self.args.dry_run && (!self.args.cont || !file.exists(db, snapshot)?) {
            self.insert_new_file(db, fs, snapshot, file)?;
        }
        Ok(())
    }

    fn process_existing_file(
        &self, db: &Database, fs: &FileSystem, snapshot: &mut Snapshot, previous_file: &mut File, file: &mut File,
    ) -> Result<(), CmdError> {
        snapshot.count += 1;
        snapshot.size += file.size;
        if file.size != previous_file.size || file.modified != previous_file.modified {
            snapshot.modified_count += 1;
            snapshot.modified_size += file.size;
            if self.args.verbose || self.previous_snapshot.id != 0 {
                println!("  Modified file {:?}", previous_file.fullname())
            }
            if !self.args.dry_run {
                if self.args.config.is_incremental(&file.fullname()) {
                    if !self.args.cont || !file.exists(db, snapshot)? {
                        self.archive_file(db, fs, snapshot, previous_file)?;
                        self.insert_new_file(db, fs, snapshot, file)?;
                    }
                } else if !self.args.cont || !file.exists(db, snapshot)? {
                    self.insert_new_file(db, fs, snapshot, file)?;
                    self.remove_previous_file(db, fs, previous_file, file)?;
                }
            }
        } else {
            snapshot.unchanged_count += 1;
            snapshot.unchanged_size += file.size;
            if !self.args.dry_run && (!self.args.cont || !previous_file.exists(db, snapshot)?) {
                self.insert_unchanged_file(db, fs, snapshot, previous_file)?;
            }
        }
        Ok(())
    }

    fn process_deleted_file(&self, db: &Database, fs: &FileSystem, snapshot: &mut Snapshot, file: &mut File) -> Result<(), CmdError> {
        snapshot.count += 1;
        snapshot.size += file.size;
        snapshot.deleted_count += 1;
        snapshot.deleted_size += file.size;
        file.deleted_sid = snapshot.id;
        if self.args.verbose || self.previous_snapshot.id != 0 {
            println!("  Deleted file {:?}", file.fullname())
        }
        if !self.args.dry_run {
            if self.args.config.is_incremental(&file.fullname()) {
                if !self.args.cont || !file.archive_exists(db, snapshot)? || !fs.exists(file, &self.args.config.target) {
                    self.archive_file(db, fs, snapshot, file)?;
                }
            } else if !self.args.cont || file.exists(db, snapshot)? {
                self.remove_file(db, fs, file)?;
            }
        }
        Ok(())
    }

    fn process_deleted_dir(&self, db: &Database, fs: &FileSystem, snapshot: &mut Snapshot, dir: &mut File) -> Result<(), CmdError> {
        if self.args.verbose || self.previous_snapshot.id != 0 {
            println!("Deleted dir {:?}", dir.fullname())
        }
        let files = File::children_files(db, &self.previous_snapshot, &dir.fullname())?;
        for (_, mut file) in files {
            self.process_deleted_file(db, fs, snapshot, &mut file)?;
        }
        let dirs = File::children_dirs(db, &self.previous_snapshot, &dir.fullname())?;
        for (_, mut dir) in dirs {
            self.process_deleted_dir(db, fs, snapshot, &mut dir)?;
        }
        if !self.args.dry_run {
            if self.args.config.is_incremental(&dir.fullname()) {
                if !self.args.cont || !dir.archive_exists(db, snapshot)? {
                    self.archive_dir(db, fs, snapshot, dir)?;
                }
            } else if !self.args.cont || dir.exists(db, snapshot)? {
                self.remove_dir(db, fs, dir)?;
            }
        }
        Ok(())
    }

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

    fn insert_new_file(&self, db: &Database, fs: &FileSystem, snapshot: &Snapshot, file: &mut File) -> Result<(), CmdError> {
        file.digest = fs.compute_digest(file, &self.args.config.source)?;
        fs.copy_new_file(file, self.args.config.hash_comparison)?;
        file.insert(db, snapshot)
    }

    fn insert_unchanged_file(&self, db: &Database, _fs: &FileSystem, snapshot: &Snapshot, file: &File) -> Result<(), CmdError> {
        file.insert_ref(db, snapshot)
    }

    fn archive_file(&self, db: &Database, fs: &FileSystem, snapshot: &Snapshot, file: &mut File) -> Result<(), CmdError> {
        file.archive(db, snapshot)?;
        fs.archive_file(snapshot.id, file)
    }

    fn archive_dir(&self, db: &Database, _fs: &FileSystem, snapshot: &Snapshot, dir: &mut File) -> Result<(), CmdError> {
        dir.archive(db, snapshot)
    }

    fn remove_previous_file(&self, db: &Database, _fs: &FileSystem, previous_file: &File, file: &File) -> Result<(), CmdError> {
        previous_file.update_ref(db, file)?;
        previous_file.delete(db)
    }

    fn remove_file(&self, db: &Database, fs: &FileSystem, file: &File) -> Result<(), CmdError> {
        file.delete_ref(db)?;
        file.delete(db)?;
        fs.remove_file(file)
    }

    fn remove_dir(&self, db: &Database, fs: &FileSystem, dir: &File) -> Result<(), CmdError> {
        dir.delete_ref(db)?;
        dir.delete(db)?;
        fs.remove_dir(dir)
    }

    fn async_do_backup(&self, db: &Database, snapshot: &mut Snapshot) -> Result<(), CmdError> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| CmdError::GenericError(e.to_string()))?;
        let (dir, statistics) =
            rt.block_on(Self::read_source(Arc::new(self.args.clone()), self.previous_snapshot.id, self.args.config.source.clone()))?;
        if !statistics.has_files && !self.args.quiet {
            println!("Warning: backup is empty");
        }
        snapshot.excluded_dirs = statistics.dirs;
        snapshot.excluded_files = statistics.files;
        println!("source");
        dir.display(0);
        println!();
        let prev_dir = self.read_previous_source(db)?;
        println!("previous");
        prev_dir.display(0);
        let todo = self.compare_tree(dir, prev_dir);
        println!("diff");
        todo.display();
        Ok(())
    }
    fn read_source(args: Arc<BackupArgs>, pid: u64, root: PathBuf) -> Pin<Box<dyn Future<Output = ReadDirectoryResult> + Send + 'static>> {
        Box::pin(async move {
            let mut statistics = Statistics::default();
            if !args.quiet && pid == 0 {
                println!("Directory {:?}", root);
            }
            let mut subdirs: Vec<PathBuf> = vec![];
            let mut files = HashSet::new();
            let mut entries = tokio::fs::read_dir(root.clone())
                .await
                .map_err(|err| CmdError::IoError(IoError::from_str("Cannot iterate entries", err)))?;
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
                        statistics.dirs += 1;
                        if args.verbose {
                            println!("Excluded directory {:?}", p)
                        }
                    } else {
                        statistics.files += 1;
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
                statistics.files += stats.files;
                statistics.has_files |= stats.has_files;
                children.insert(dir);
            }
            Ok((
                Directory::from_parts(
                    PathBuf::from(
                        root.strip_prefix(&args.config.source)
                            .map_err(|_| CmdError::GenericError(format!("Cannot strip prefix: {:?}", root)))?,
                    ),
                    files,
                    children,
                ),
                statistics,
            ))
        })
    }

    fn build_directory(&self, by_path: &mut HashMap<PathBuf, Vec<File>>, path: PathBuf) -> Directory {
        let entries = by_path.remove(&path).unwrap_or_default();
        let mut files = HashSet::new();
        let mut children = HashSet::new();
        for entry in entries {
            if entry.is_dir {
                children.insert(self.build_directory(by_path, entry.fullname()));
            } else {
                files.insert(entry);
            }
        }
        Directory { name: path, files, children }
    }

    fn read_previous_source(&self, db: &Database) -> Result<Directory, CmdError> {
        let entries = File::all_entries(db, &self.previous_snapshot)?;
        let mut by_path: HashMap<PathBuf, Vec<File>> = HashMap::new();
        for entry in entries {
            by_path.entry(entry.path.clone()).or_default().push(entry);
        }
        Ok(self.build_directory(&mut by_path, PathBuf::new()))
    }

    fn compare_tree(&self, src: Directory, prev: Directory) -> Todo {
        let mut todo = Todo::default();
        self.recurse_compare_tree(src, prev, &mut todo);
        todo
    }

    fn recurse_compare_tree(&self, src: Directory, mut prev: Directory, todo: &mut Todo) {
        for file in src.files {
            if let Some(prev_file) = prev.files.take(&file) {
                if prev_file != file {
                    todo.file_modified.push((file, prev_file));
                }
                continue;
            }
            todo.file_added.push(file);
        }
        todo.file_deleted.extend(prev.files.drain());
        for child in src.children {
            if let Some(prev_child) = prev.children.take(&child) {
                self.recurse_compare_tree(child, prev_child, todo);
                continue;
            }
            todo.dir_added.push(child);
        }
        todo.dir_deleted.extend(prev.children.drain());
    }
}
