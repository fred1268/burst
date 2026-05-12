use crate::args::delete::DeleteArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::file::File;
use crate::cmds::snapshot::Snapshot;
use crate::tools::cmderror::CmdError::{self, InvalidBackupDirectory, InvalidOption};
use crate::tools::db::Database;
use crate::tools::fmt::human_readable_duration;
use crate::tools::fs::FileSystem;
use chrono::Datelike;
use regex::Regex;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

pub struct DeleteCommand {
    args: DeleteArgs,
}

impl Default for DeleteCommand {
    fn default() -> Self {
        DeleteCommand::from(DeleteArgs::default())
    }
}

impl From<DeleteArgs> for DeleteCommand {
    fn from(args: DeleteArgs) -> Self {
        DeleteCommand { args }
    }
}

impl Command for DeleteCommand {
    fn validate(&mut self) -> Result<(), CmdError> {
        if self.args.sids.is_empty() && self.args.keep_last == 0 && self.args.older_than.year() != 1970 {
            return Err(InvalidOption(String::from("Missing snapshot selector")));
        }
        Regex::new(&self.args.pattern).map_err(|_| InvalidOption(format!("Invalid pattern {}", self.args.pattern)))?;
        Ok(())
    }

    fn help(&self) {
        println!("Usage: {} delete <SELECTOR> [OPTIONS] <BACKUP_PATH>", self.args.exe);
        println!();
        println!("Removes backup history selectively to manage storage space and retention policies.");
        println!();
        println!("Selector:");
        println!("\t-s, --snapshot <SPEC>\t\t\tdelete snapshots (single: 5, list: 1,3,7, range: 1-5)");
        println!("\t-o, --older-than <DATE>\t\t\tdelete history older than specified date (yyyy-mm-dd)");
        println!("\t-l, --keep-last <COUNT>\t\t\tkeep only the last N versions of each file");
        println!();
        println!("Options:");
        println!("\t-p, --pattern <PATTERN>\t\t\tfiles or directories to delete (relative to backup root)");
        println!("\t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)");
        println!("\t-v, --verbose\t\t\t\tdisplay more detailed information");
        println!("\t-n, --dry-run\t\t\t\tdon't actually touch the filesystem, do a dry run instead");
        println!();
        println!("Examples of patterns (regex):");
        println!("\t*.png:\t\t\t\t\t--pattern \".*\\.png$\"");
        println!("\toffice folder and its content:\t\t--pattern: \"/?office(/.*)?$\"");
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + '_>> {
        Box::pin(async move {
            let start = Instant::now();
            if self.args.verbose {
                println!("delete command started");
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
            let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
            if self.args.keep_last != 0 {
                let snapshots = Snapshot::get_except_last(&db, self.args.keep_last).await?;
                for snapshot in snapshots {
                    if !self.args.sids.contains(&snapshot.id) {
                        self.args.sids.push(snapshot.id);
                    }
                }
            } else if self.args.older_than.year() != 1970 {
                let snapshots = Snapshot::get_before(&db, self.args.older_than).await?;
                for snapshot in snapshots {
                    if !self.args.sids.contains(&snapshot.id) {
                        self.args.sids.push(snapshot.id);
                    }
                }
            }
            match self.args.config.incremental {
                true => self.delete_history_incremental(&db, &fs).await?,
                false => self.delete_history_non_incremental(&db, &fs).await?,
            }
            self.clean_up(&db, &fs).await?;
            if !self.args.quiet {
                println!("Files successfully deleted from history in {}", human_readable_duration(start.elapsed().as_secs()));
            }
            command::stop(&self.args.config.target).await
        })
    }
}

impl DeleteCommand {
    async fn unarchive_file(&self, db: &Database, fs: &FileSystem, snapshot: &Snapshot, file: &mut File) -> Result<(), CmdError> {
        file.deleted_sid = 0;
        file.unarchive(db).await?;
        fs.unarchive_file(snapshot.id, file).await
    }

    async fn unarchive_dir(&self, db: &Database, fs: &FileSystem, dir: &mut File) -> Result<(), CmdError> {
        dir.deleted_sid = 0;
        dir.unarchive(db).await?;
        fs.remove_archive_dir(dir).await
    }

    async fn clean_up(&self, db: &Database, fs: &FileSystem) -> Result<(), CmdError> {
        let dirs = File::filesystem_dirs(db).await?;
        for dir in &dirs {
            if !fs.exists(dir, &self.args.config.target).await? {
                if self.args.verbose {
                    println!("  Removing directory {}", &dir);
                }
                dir.delete_ref(db).await?;
                dir.delete(db).await?;
            }
        }
        Ok(())
    }

    async fn delete_history_incremental(&self, db: &Database, fs: &FileSystem) -> Result<(), CmdError> {
        for sid in &self.args.sids {
            if !self.args.quiet {
                println!("Deleting snapshot {} files", *sid);
            }
            if !self.args.dry_run {
                if !self.args.pattern.is_empty() {
                    File::delete_files(db, *sid, &self.args.pattern).await?;
                } else {
                    File::delete_all_by_sid(db, *sid).await?;
                    Snapshot::delete_by_id(db, *sid).await?;
                }
            }
        }
        let files = File::orphans(db).await?;
        for file in files {
            if self.args.verbose {
                println!("  Removing file {}", file);
            }
            if !self.args.dry_run {
                file.delete(db).await?;
                fs.remove_file(&file).await?;
            }
        }
        if let Some(snapshot) = Snapshot::get_latest(db).await? {
            let files = File::files(db, snapshot.id).await?;
            for mut file in files {
                if file.is_archived() {
                    if self.args.verbose {
                        println!("  Unarchiving file {}", &file);
                    }
                    if !self.args.dry_run {
                        self.unarchive_file(db, fs, &snapshot, &mut file).await?;
                    }
                }
            }
            let dirs = File::dirs(db, snapshot.id).await?;
            for mut dir in dirs {
                if dir.is_archived() {
                    if self.args.verbose {
                        println!("  Unarchiving directory {}", &dir);
                    }
                    if !self.args.dry_run {
                        self.unarchive_dir(db, fs, &mut dir).await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn delete_history_non_incremental(&self, db: &Database, _fs: &FileSystem) -> Result<(), CmdError> {
        for sid in &self.args.sids {
            if !self.args.quiet {
                println!("Deleting snapshot {} files", *sid);
            }
            if !self.args.dry_run {
                File::delete_sync_mode(db, *sid).await?;
            }
        }
        Ok(())
    }
}
