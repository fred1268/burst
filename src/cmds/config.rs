use crate::args::backupconfig::BackupConfig;
use crate::args::config::ConfigArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::file::File;
use crate::cmds::snapshot::Snapshot;
use crate::tools::db::Database;
use crate::tools::error::Error::{self, InvalidBackupDirectory, InvalidOption, InvalidSourceDirectory};
use crate::tools::fs::FileSystem;
use regex::Regex;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub struct ConfigCommand {
    args: ConfigArgs,
}

impl Default for ConfigCommand {
    fn default() -> Self {
        ConfigCommand::from(ConfigArgs::default())
    }
}

impl From<ConfigArgs> for ConfigCommand {
    fn from(args: ConfigArgs) -> Self {
        ConfigCommand { args }
    }
}

impl Command for ConfigCommand {
    fn validate(&mut self) -> Result<(), Error> {
        match self.args.subcommand.as_str() {
            "show" => {
                if !self.args.key.is_empty() || !self.args.value.is_empty() {
                    return Err(InvalidOption(String::from("Both key and value must be empty")));
                }
            }
            "get" => {
                if self.args.key.is_empty() {
                    return Err(InvalidOption(String::from("Key is empty")));
                }
                if !self.args.value.is_empty() {
                    return Err(InvalidOption(format!("value is not empty: {}", self.args.value)));
                }
            }
            "convert" => {
                if self.args.key != "sync" && self.args.key != "incremental" {
                    return Err(InvalidOption(String::from("Unexpected mode: expecting 'sync' or 'incremental'")));
                }
                if !self.args.value.is_empty() {
                    return Err(InvalidOption(format!("Value is not empty: {}", self.args.value)));
                }
            }
            "set" | "add" => {
                if self.args.key.is_empty() || self.args.value.is_empty() {
                    return Err(InvalidOption(String::from("Key or value is empty")));
                }
            }
            "remove" => {
                if self.args.key.is_empty() {
                    return Err(InvalidOption(String::from("Key is empty")));
                }
            }
            _ => (),
        }
        match self.args.subcommand.as_str() {
            "show" | "get" | "set" | "add" | "remove" | "convert" => (),
            _ => return Err(InvalidOption(format!("Invalid subcommand: {}", self.args.subcommand))),
        }
        Ok(())
    }

    fn help(&self) {
        println!(
            "Usage: {} config <SUBCOMMAND> [OPTIONS] <BACKUP_PATH>\n\n\
        Manages backup repository configuration and performs configuration-related operations.\n\
        Includes both safe configuration changes and destructive repository modifications.\n\n\
        Subcommands:\n\
        \tshow\t\t\t\t\tdisplay current configuration\n\
        \tget <KEY>\t\t\t\tget specific configuration value\n\
        \tset <KEY> <VALUE>\t\t\tset configuration value (source|hash_comparison|follow_symlinks)\n\
        \tadd <KEY> <PATTERN>\t\t\tadd value to multi-value keys (exclude|no_history)\n\
        \tremove <KEY> <PATTERN>\t\t\tRemove specific value OR entire key (exclude|no_history)\n\
        \tconvert <MODE>\t\t\t\tconvert repository mode (sync|incremental)\n\n\
        Options:\n\
        \t    --fix-history\t\t\tremove previously backed up versions (destructive)\n\
        \t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)\n\
        \t-v, --verbose\t\t\t\tdisplay more detailed information\n\
        \t-n, --dry-run\t\t\t\tdon't actually touch the filesystem, do a dry run instead\n\n\
        Examples of patterns (regex):\n\
        \t*.tmp:\t\t\t\t\tadd exclude \".*\\.tmp\"\n\
        \tmacOS trash:\t\t\t\tadd exclude: \".*/\\.DS_Store$\"",
            self.args.exe
        );
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + '_>> {
        Box::pin(async move {
            if self.args.verbose {
                println!("config command started");
                println!("Running {}", self.args);
            }
            match command::start(&self.args.config.target).await {
                Ok(_) => (),
                Err(err) => match err {
                    Error::NoRemote() => match self.args.subcommand.as_str() {
                        "show" | "get" => (),
                        _ => {
                            if !self.args.dry_run {
                                return Err(InvalidBackupDirectory());
                            }
                        }
                    },
                    _ => return Err(err),
                },
            };
            self.args.config.read(&command::config_file(&self.args.config.target)).await?;
            let db = Database::open(&self.args.config.target).await?;
            let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
            match self.args.subcommand.as_str() {
                "show" => self.show(&db, &fs).await?,
                "get" => self.get(&db, &fs).await?,
                "set" => self.set(&db, &fs).await?,
                "add" => self.add(&db, &fs).await?,
                "remove" => self.remove(&db, &fs).await?,
                "convert" => self.convert(&db, &fs).await?,
                _ => (),
            }
            if !self.args.quiet {
                println!("Configuration updated");
            }
            command::stop(&self.args.config.target).await
        })
    }
}

impl ConfigCommand {
    async fn show(&self, _db: &Database, _fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            println!("Showing config")
        }
        println!("{}", self.args.config.as_str());
        Ok(())
    }

    async fn get(&self, _db: &Database, _fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            println!("Retrieving {}", self.args.key)
        }
        match self.args.key.as_str() {
            "source" => println!("source: {:?}", self.args.config.source),
            "incremental" => println!("incremental: {}", self.args.config.incremental),
            "hash_comparison" => println!("hash_comparison: {}", self.args.config.hash_comparison),
            "follow_symlinks" => println!("follow_symlinks: {}", self.args.config.follow_symlinks),
            "exclude" => println!("exclude: {:?}", self.args.config.exclude),
            "no_history" => println!("no_history: {:?}", self.args.config.no_history),
            _ => return Err(InvalidOption(format!("Invalid key {}", self.args.key))),
        }
        Ok(())
    }

    async fn set(&mut self, _db: &Database, _fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            println!("Setting {} to {}", self.args.key, self.args.value)
        }
        match self.args.key.as_str() {
            "source" => {
                self.args.config.source = PathBuf::from(&self.args.value).canonicalize().map_err(|_| InvalidSourceDirectory())?;
            }
            "hash_comparison" => {
                self.args.config.hash_comparison =
                    self.args.value.parse::<bool>().map_err(|_| Error::InvalidOption(format!("Cannot parse {}", &self.args.value)))?
            }
            "follow_symlinks" => {
                self.args.config.follow_symlinks =
                    self.args.value.parse::<bool>().map_err(|_| Error::InvalidOption(format!("Cannot parse {}", &self.args.value)))?
            }
            _ => return Err(InvalidOption(format!("Invalid key {}", self.args.key))),
        }
        if !self.args.dry_run {
            self.args.config.write().await?;
        }
        Ok(())
    }

    async fn add(&mut self, db: &Database, fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            println!("Adding {} to {}", self.args.value, self.args.key)
        }
        match self.args.key.as_str() {
            "exclude" => {
                let mut added: Vec<Regex> = vec![];
                for value in self.args.value.split(',') {
                    if !value.is_empty() {
                        let re = Regex::new(value).map_err(|_| InvalidOption(format!("Invalid regex {}", value)))?;
                        self.args.config.exclude.push(String::from(value));
                        added.push(re);
                    }
                }
                self.fix_history(db, fs, &added, 0).await?;
            }
            "no_history" => {
                let mut added: Vec<Regex> = vec![];
                for value in self.args.value.split(',') {
                    if !value.is_empty() {
                        let re = Regex::new(value).map_err(|_| InvalidOption(format!("Invalid regex {}", value)))?;
                        self.args.config.no_history.push(String::from(value));
                        added.push(re);
                    }
                }
                if let Some(latest_snapshot) = Snapshot::get_latest(db).await? {
                    self.fix_history(db, fs, &added, latest_snapshot.id).await?;
                }
            }
            _ => return Err(InvalidOption(format!("Invalid key {}", self.args.key))),
        }
        if !self.args.dry_run {
            self.args.config.write().await?;
        }
        Ok(())
    }

    async fn remove(&mut self, _db: &Database, _fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            if self.args.value.is_empty() {
                if self.args.verbose {
                    println!("Removing {}", self.args.key);
                }
                match self.args.key.as_str() {
                    "exclude" => {
                        self.args.config.exclude.clear();
                        self.args.config.re_excl.clear();
                    }
                    "no_history" => {
                        self.args.config.no_history.clear();
                        self.args.config.re_hist.clear();
                    }
                    _ => return Err(InvalidOption(format!("Invalid key {}", self.args.key))),
                }
            } else {
                if self.args.verbose {
                    println!("Removing {} from {}", self.args.value, self.args.key);
                }
                match self.args.key.as_str() {
                    "exclude" => {
                        if self.args.config.exclude.contains(&self.args.value) {
                            let n = self.args.config.exclude.iter().position(|e| *e == self.args.value).unwrap();
                            self.args.config.exclude.remove(n);
                        }
                    }
                    "no_history" => {
                        if self.args.config.no_history.contains(&self.args.value) {
                            let n = self.args.config.no_history.iter().position(|e| *e == self.args.value).unwrap();
                            self.args.config.no_history.remove(n);
                        }
                    }
                    _ => return Err(InvalidOption(format!("Invalid key {}", self.args.key))),
                }
            }
        }
        if !self.args.dry_run {
            self.args.config.write().await?;
        }
        Ok(())
    }

    async fn convert(&mut self, db: &Database, fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            println!("Converting backup to {}", self.args.key)
        }
        if self.args.key == "sync" {
            self.args.config.incremental = false;
        } else if self.args.key == "incremental" {
            self.args.config.incremental = true;
        }
        if !self.args.dry_run {
            self.args.config.write().await?;
        }
        if !self.args.fix_history || self.args.config.incremental {
            return Ok(());
        }
        if !self.args.quiet {
            println!("Cleaning up previous snapshots")
        }
        if let Some(latest_snapshot) = Snapshot::get_latest(db).await?
            && !self.args.dry_run
        {
            File::delete_all_refs_keep_snapshot(db, latest_snapshot.id, &PathBuf::new(), &PathBuf::new()).await?;
            self.remove_orphans(db, fs).await?;
            latest_snapshot.delete_all_except(db).await?;
            File::insert_history_sync_mode(db, latest_snapshot.id).await?;
        }
        Ok(())
    }

    async fn fix_history(&self, db: &Database, fs: &FileSystem, added: &[Regex], keep_sid: u64) -> Result<(), Error> {
        if !self.args.fix_history {
            return Ok(());
        }
        if added.is_empty() {
            if !self.args.quiet {
                println!("No newly excluded files or directories to clean up");
            }
            return Ok(());
        }
        if !self.args.quiet {
            println!("Cleaning up newly excluded files and directories");
        }
        let entries = File::distinct_entries(db).await?;
        for re in added {
            for entry in &entries {
                if !BackupConfig::matches(re, &entry.fullname()) {
                    continue;
                }
                if self.args.verbose {
                    println!("  Deleting {}", entry);
                }
                if !self.args.dry_run {
                    if entry.is_dir {
                        File::delete_all_refs_keep_snapshot(db, keep_sid, &entry.fullname(), &PathBuf::new()).await?;
                    }
                    File::delete_all_refs_keep_snapshot(db, keep_sid, &entry.path, &entry.name).await?;
                }
            }
        }
        self.remove_orphans(db, fs).await
    }

    async fn remove_orphans(&self, db: &Database, fs: &FileSystem) -> Result<(), Error> {
        let entries = File::orphans(db).await?;
        for entry in entries {
            if self.args.verbose {
                println!("  Removing {}", entry);
            }
            if !self.args.dry_run {
                entry.delete(db).await?;
                match entry.is_dir {
                    true => fs.remove_dir(&entry).await?,
                    false => fs.remove_file(&entry).await?,
                }
            }
        }
        Ok(())
    }
}
