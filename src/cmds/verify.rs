use crate::args::verify::VerifyArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::constants::{BURST_DIRECTORY, BURST_VERSION_DIR};
use crate::cmds::file::File;
use crate::tools::cmderror::CmdError::{self, InvalidBackupDirectory, InvalidOption};
use crate::tools::cmderror::IoError;
use crate::tools::db::Database;
use crate::tools::fmt::human_readable_duration;
use crate::tools::fs::FileSystem;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct VerifyCommand {
    args: VerifyArgs,
    warnings: u64,
}

impl Default for VerifyCommand {
    fn default() -> Self {
        VerifyCommand::from(VerifyArgs::default())
    }
}

impl From<VerifyArgs> for VerifyCommand {
    fn from(args: VerifyArgs) -> Self {
        VerifyCommand { args, warnings: 0 }
    }
}

impl Command for VerifyCommand {
    fn validate(&mut self) -> Result<(), CmdError> {
        match self.args.topic.as_str() {
            "hash" | "integrity" => (),
            _ => return Err(InvalidOption(String::from("Invalid parameter: expected 'hash' or 'integrity'"))),
        }
        Ok(())
    }

    fn help(&self) {
        println!("Usage: {} verify <WHAT> [OPTIONS] <BACKUP_PATH>", self.args.exe);
        println!();
        println!("Verifies backup integrity and files consistency.");
        println!();
        println!("What:");
        println!("\tintegrity\t\t\t\tcheck backup file system integrity");
        println!("\thash\t\t\t\t\tcheck file integrity by comparing hash");
        println!();
        println!("Options:");
        println!("\t    --fix\t\t\t\ttry to fix issues if any");
        println!("\t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)");
        println!("\t-v, --verbose\t\t\t\tdisplay more detailed information");
    }

    fn run(&mut self) -> Result<(), CmdError> {
        let start = Instant::now();
        if self.args.verbose {
            println!("verify command started");
            println!("Running {}", self.args);
        }
        match command::start(&self.args.config.target) {
            Ok(_) => (),
            Err(err) => match err {
                CmdError::NoRemote() => return Err(InvalidBackupDirectory()),
                _ => return Err(err),
            },
        };
        self.args.config.read(&command::config_file(&self.args.config.target))?;
        let db = Database::open(&self.args.config.target)?;
        let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
        match self.args.topic.as_str() {
            "hash" => self.hash(&db, &fs)?,
            "integrity" => self.integrity(&db, &fs)?,
            _ => (),
        }
        if !self.args.quiet {
            match self.warnings {
                0 => println!("Verification successfully completed in {}", human_readable_duration(start.elapsed().as_secs())),
                _ => println!(
                    "Verification completed in {} with {} warning",
                    human_readable_duration(start.elapsed().as_secs()),
                    self.warnings
                ),
            }
        }
        command::stop(&self.args.config.target)
    }
}

impl VerifyCommand {
    fn hash(&mut self, db: &Database, fs: &FileSystem) -> Result<(), CmdError> {
        if self.args.verbose {
            println!("Verifying hash")
        }
        let entries = File::filesystem_entries(db)?;
        for entry in &entries {
            if entry.is_dir {
                if self.args.verbose {
                    println!("  Directory {:?}", entry.fullname())
                }
                continue;
            }
            let digest = fs.compute_digest(entry, &self.args.config.target)?;
            if digest != entry.digest {
                self.warnings += 1;
                println!("Warning: hash mismatch for {:?}", entry.fullname());
            }
        }
        Ok(())
    }

    fn integrity(&mut self, db: &Database, fs: &FileSystem) -> Result<(), CmdError> {
        if !self.args.quiet {
            println!("Verifying integrity");
            println!("Phase #1: checking database");
        }
        let entries = File::filesystem_entries(db)?;
        for entry in &entries {
            if entry.is_dir && self.args.verbose {
                println!("  Directory {:?}", entry.fullname());
            }
            if !fs.exists(entry, &self.args.config.target) {
                self.warnings += 1;
                if entry.is_dir {
                    println!("  Warning: directory {:?} referenced in DB does not exist in the file system", entry.fullname());
                } else {
                    println!("  Warning: file {:?} referenced in DB does not exist in the file system", entry.fullname());
                }
                if self.args.fix {
                    entry.delete_ref(db)?;
                    entry.delete(db)?;
                }
            }
        }
        if !self.args.quiet {
            println!("Phase #2: checking file system");
        }
        self.read_directory(db, &self.args.config.target.clone())?;
        Ok(())
    }

    fn read_directory(&mut self, db: &Database, dir: &Path) -> Result<(), CmdError> {
        if self.args.verbose {
            println!("  Directory {:?}", dir);
        }
        let mut dirs: Vec<PathBuf> = vec![];
        let entries = fs::read_dir(dir).map_err(|err| CmdError::IoError(IoError::from_str("Cannot iterate entries", err)))?;
        for entry in entries {
            let entry = entry.map_err(|err| CmdError::IoError(IoError::from_str("Invalid entry", err)))?;
            let p = entry.path();
            if p.ends_with(BURST_DIRECTORY) || p.ends_with(BURST_VERSION_DIR) {
                continue;
            }
            match File::find_entry(
                db,
                &PathBuf::from(
                    p.strip_prefix(&self.args.config.target).map_err(|_| CmdError::GenericError(String::from("Cannot strip prefix")))?,
                ),
            )? {
                Some(_) => {
                    if p.is_dir() {
                        dirs.push(p);
                    }
                }
                None => {
                    self.warnings += 1;
                    if p.is_dir() {
                        println!("  Warning: directory {:?} does not exist in the database", &p);
                    } else {
                        println!("  Warning: file {:?} does not exist in the database", &p);
                    }
                }
            }
        }
        for dir in dirs {
            self.read_directory(db, &dir)?;
        }
        Ok(())
    }
}
