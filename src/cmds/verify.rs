use crate::args::verify::VerifyArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::constants::{BURST_DIRECTORY, BURST_VERSION_DIR};
use crate::cmds::file::File;
use crate::tools::db::Database;
use crate::tools::error::Error::{self, InvalidBackupDirectory, InvalidOption};
use crate::tools::error::IoError;
use crate::tools::fmt::human_readable_duration;
use crate::tools::fs::FileSystem;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
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
    fn validate(&mut self) -> Result<(), Error> {
        match self.args.topic.as_str() {
            "hash" | "integrity" => (),
            _ => return Err(InvalidOption(String::from("Invalid parameter: expected 'hash' or 'integrity'"))),
        }
        Ok(())
    }

    fn help(&self) {
        println!(
            "Usage: {} verify <WHAT> [OPTIONS] <BACKUP_PATH>\n\n\
        Verifies backup integrity and files consistency.\n\n\
        What:\n\
        \tintegrity\t\t\t\tcheck backup file system integrity\n\
        \thash\t\t\t\t\tcheck file integrity by comparing hash\n\n\
        Options:\n\
        \t    --fix\t\t\t\ttry to fix issues if any\n\
        \t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)\n\
        \t-v, --verbose\t\t\t\tdisplay more detailed information",
            self.args.exe
        );
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + '_>> {
        Box::pin(async move {
            let start = Instant::now();
            if self.args.verbose {
                println!("verify command started");
                println!("Running {}", self.args);
            }
            if let Err(err) = command::start(&self.args.config.target).await {
                match err {
                    Error::NoRemote() => return Err(InvalidBackupDirectory()),
                    _ => return Err(err),
                }
            };
            self.args.config.read(&command::config_file(&self.args.config.target)).await?;
            let db = Database::open(&self.args.config.target).await?;
            let fs = FileSystem::new(&self.args.config.source, &self.args.config.target);
            match self.args.topic.as_str() {
                "hash" => self.hash(&db, &fs).await?,
                "integrity" => self.integrity(&db, &fs).await?,
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
            command::stop(&self.args.config.target).await
        })
    }
}

impl VerifyCommand {
    async fn hash(&mut self, db: &Database, fs: &FileSystem) -> Result<(), Error> {
        if self.args.verbose {
            println!("Verifying hash")
        }
        let mut entries = File::filesystem_entries(db).await?;
        for entry in &mut entries {
            if entry.is_dir {
                if self.args.verbose {
                    println!("  Directory {:?}", entry.fullname())
                }
                continue;
            }
            let digest = fs.compute_digest(entry, &self.args.config.target).await?;
            if digest != entry.digest {
                self.warnings += 1;
                println!("Warning: hash mismatch for {:?}", entry.fullname());
                if self.args.fix {
                    entry.digest = digest;
                    entry.update(db).await?;
                }
            }
        }
        Ok(())
    }

    async fn integrity(&mut self, db: &Database, fs: &FileSystem) -> Result<(), Error> {
        if !self.args.quiet {
            println!("Verifying integrity");
            println!("Phase #1: checking database");
        }
        let entries = File::filesystem_entries(db).await?;
        for entry in &entries {
            if entry.is_dir && self.args.verbose {
                println!("  Directory {:?}", entry.fullname());
            }
            if !fs.exists(entry, &self.args.config.target).await? {
                self.warnings += 1;
                if entry.is_dir {
                    println!("  Warning: directory {:?} referenced in DB does not exist in the file system", entry.fullname());
                } else {
                    println!("  Warning: file {:?} referenced in DB does not exist in the file system", entry.fullname());
                }
                if self.args.fix {
                    entry.delete_ref(db).await?;
                    entry.delete(db).await?;
                }
            }
        }
        if !self.args.quiet {
            println!("Phase #2: checking file system");
        }
        self.read_directory(db, &self.args.config.target.clone()).await?;
        Ok(())
    }

    fn read_directory<'a>(&'a mut self, db: &'a Database, dir: &'a Path) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'a>> {
        Box::pin(async move {
            if self.args.verbose {
                println!("  Directory {:?}", dir);
            }
            let mut dirs: Vec<PathBuf> = vec![];
            let entries = fs::read_dir(dir).map_err(|err| Error::IoError(IoError::from_str("Cannot iterate entries", err)))?;
            for entry in entries {
                let entry = entry.map_err(|err| Error::IoError(IoError::from_str("Invalid entry", err)))?;
                let p = entry.path();
                if p.ends_with(BURST_DIRECTORY) || p.ends_with(BURST_VERSION_DIR) {
                    continue;
                }
                match File::find_entry(
                    db,
                    &PathBuf::from(
                        p.strip_prefix(&self.args.config.target)
                            .map_err(|_| Error::GenericError(format!("Cannot strip prefix: {:?}", p)))?,
                    ),
                )
                .await?
                {
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
                self.read_directory(db, &dir).await?;
            }
            Ok(())
        })
    }
}
