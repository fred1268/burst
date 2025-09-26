use regex::Regex;

use crate::args::restore::RestoreArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::file::File;
use crate::cmds::snapshot::Snapshot;
use crate::tools::cmderror::CmdError::{self, InvalidBackupDirectory, InvalidOption};
use crate::tools::db::Database;
use crate::tools::fmt::human_readable_duration;
use crate::tools::fs::FileSystem;
use std::time::Instant;

pub struct RestoreCommand {
    args: RestoreArgs,
}

impl Default for RestoreCommand {
    fn default() -> Self {
        RestoreCommand::from(RestoreArgs::default())
    }
}

impl From<RestoreArgs> for RestoreCommand {
    fn from(args: RestoreArgs) -> Self {
        RestoreCommand { args }
    }
}

impl Command for RestoreCommand {
    fn validate(&mut self) -> Result<(), CmdError> {
        if !self.args.to.exists() {
            return Err(InvalidOption(String::from("Invalid restore path")));
        }
        if self.args.sid == 0 {
            return Err(InvalidOption(String::from("Missing snapshot id")));
        }
        Regex::new(&self.args.pattern).map_err(|_| InvalidOption(format!("Invalid pattern {}", self.args.pattern)))?;
        Ok(())
    }

    fn help(&self) {
        println!("Usage: {} restore [OPTIONS] <BACKUP_PATH> <RESTORE_PATH>", self.args.exe);
        println!();
        println!("Restores files from backup snapshot to specified location.");
        println!();
        println!("Options:");
        println!("\t-s, --snapshot <ID>\t\t\trestore from specific snapshot");
        println!("\t-p, --pattern <PATTERN>\t\t\tfiles or directories to restore");
        println!("\t-w, --overwrite\t\t\t\toverwrite existing files");
        println!("\t-t, --flatten\t\t\t\tignore original directory paths");
        println!("\t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)");
        println!("\t-v, --verbose\t\t\t\tdisplay more detailed information");
        println!("\t-n, --dry-run\t\t\t\tdon't actually touch the filesystem, do a dry run instead");
        println!();
        println!("Examples of patterns (regex):");
        println!("\t*.png:\t\t\t\t\t--pattern \".*\\.png\"");
        println!("\tPDFs inside any manuals folders:\t--pattern: \"/?manuals(/.*)?/.*\\.pdf$\"");
    }

    fn run(&mut self) -> Result<(), CmdError> {
        let start = Instant::now();
        if self.args.verbose {
            println!("restore command started");
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
        let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
        match Snapshot::get(&db, self.args.sid)? {
            Some(snapshot) => self.restore(&db, &fs, &snapshot)?,
            None => match Snapshot::get_latest(&db)? {
                Some(snapshot) => self.restore(&db, &fs, &snapshot)?,
                None => return Err(InvalidOption(String::from("Snapshot not found"))),
            },
        }
        if !self.args.quiet {
            println!(
                "Files successfully restored to {} in {}",
                String::from(self.args.to.to_str().unwrap()),
                human_readable_duration(start.elapsed().as_secs())
            )
        }
        command::stop(&self.args.config.target)
    }
}

impl RestoreCommand {
    fn restore(&self, db: &Database, fs: &FileSystem, snapshot: &Snapshot) -> Result<(), CmdError> {
        let files = File::entries_matching(db, snapshot, &self.args.pattern)?;
        for file in files {
            if self.args.verbose {
                println!("Restoring {}", file);
            }
            if !self.args.dry_run && fs.restore_file(&file, &self.args.to, self.args.flatten, self.args.overwrite)? && self.args.verbose {
                println!("Skipping existing file {}", file);
            }
        }
        Ok(())
    }
}
