use regex::Regex;

use crate::args::list::ListArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::file::File;
use crate::cmds::snapshot::Snapshot;
use crate::tools::db::Database;
use crate::tools::error::Error::{self, InvalidOption};
use std::future::Future;
use std::pin::Pin;

pub struct ListCommand {
    args: ListArgs,
}

impl Default for ListCommand {
    fn default() -> Self {
        ListCommand::from(ListArgs::default())
    }
}

impl From<ListArgs> for ListCommand {
    fn from(args: ListArgs) -> Self {
        ListCommand { args }
    }
}

impl Command for ListCommand {
    fn validate(&mut self) -> Result<(), Error> {
        if (self.args.sid != 0 || self.args.deleted || self.args.diff_sid != 0) && !self.args.pattern.is_empty() {
            return Err(InvalidOption(String::from("--pattern does not work with --snapshot, --deleted or --diff-with")));
        }
        if self.args.diff_sid != 0 && self.args.sid == 0 {
            return Err(InvalidOption(String::from("-diff-with requires a valid --snapshot")));
        }
        Regex::new(&self.args.pattern).map_err(|_| InvalidOption(format!("Invalid pattern {}", self.args.pattern)))?;
        Ok(())
    }

    fn help(&self) {
        println!(
            "Usage: {} list [OPTIONS] <BACKUP_PATH>\n\n\
        Shows files in a snapshot or the specified files history.\n\n\
        Options:\n\
        \t-s, --snapshot <ID>\t\t\tList the content of a specific snapshot\n\
        \t-i, --diff-with\t\t\t\tShow differences with the specified snapshot\n\
        \t-p, --pattern <PATTERN>\t\t\tShow all historical versions for specified files\n\
        \t-d, --deleted\t\t\t\tShow only deleted files in the specified snapshot\n\
        \t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)\n\
        \t-v, --verbose\t\t\t\tdisplay more detailed information",
            self.args.exe
        );
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + '_>> {
        Box::pin(async move {
            if self.args.verbose {
                println!("list command started");
                println!("Running {}", self.args);
            }
            if let Err(err) = command::start(&self.args.config.target).await {
                match err {
                    Error::NoRemote() => (),
                    _ => return Err(err),
                }
            };
            self.args.config.read(&command::config_file(&self.args.config.target)).await?;
            let db = Database::open(&self.args.config.target).await?;
            if let Some(mut snapshot) = Snapshot::get_latest(&db).await? {
                if !self.args.pattern.is_empty() {
                    self.list_file(&db, snapshot).await?;
                } else {
                    if self.args.sid == 0 {
                        self.args.sid = snapshot.id;
                    } else if let Some(s) = Snapshot::get(&db, self.args.sid).await? {
                        snapshot = s;
                    } else {
                        return Ok(());
                    }
                    self.list_snapshot(&db, &snapshot).await?;
                }
            }
            command::stop(&self.args.config.target).await
        })
    }
}

impl ListCommand {
    async fn list_snapshot(&self, db: &Database, snapshot: &Snapshot) -> Result<(), Error> {
        let files = match self.args.deleted {
            true => {
                if let Some(previous_snapshot) = Snapshot::get_previous(db, snapshot.id).await? {
                    File::deleted_files(db, previous_snapshot.id, snapshot.id).await?
                } else if !self.args.config.incremental {
                    if let Some(previous_snapshot) = Snapshot::get_previous_sync_mode(db, snapshot.id).await? {
                        File::deleted_files_sync_mode(db, previous_snapshot.id, snapshot.id).await?
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
            false => match self.args.diff_sid {
                0 => File::files(db, snapshot.id).await?,
                _ => {
                    if let Some(diff_snapshot) = Snapshot::get(db, self.args.diff_sid).await? {
                        match self.args.config.incremental {
                            true => File::diff(db, diff_snapshot.id, snapshot.id).await?,
                            false => File::diff_sync_mode(db, diff_snapshot.id, snapshot.id).await?,
                        }
                    } else {
                        vec![]
                    }
                }
            },
        };
        if !self.args.quiet {
            Snapshot::header();
        }
        println!("{}\n", snapshot);
        if !self.args.quiet {
            File::header();
        }
        for file in files {
            println!("{}", file);
        }
        Ok(())
    }

    async fn list_file(&self, db: &Database, _snapshot: Snapshot) -> Result<(), Error> {
        let files = match self.args.config.incremental {
            true => File::history(db, &self.args.pattern).await?,
            false => File::history_sync_mode(db, &self.args.pattern).await?,
        };
        if !self.args.quiet {
            File::header();
        }
        let mut prev: Option<File> = None;
        for file in files {
            if let Some(p) = prev
                && p.fullname() != file.fullname()
            {
                println!()
            }
            println!("{}", file);
            prev = Some(file);
        }
        Ok(())
    }
}
