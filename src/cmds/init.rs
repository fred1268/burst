use crate::args::init::InitArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::tools::db::Database;
use crate::tools::error::Error::{self, InvalidBackupDirectory, InvalidSourceDirectory};
use crate::tools::error::IoError;
use crate::tools::fs::FileSystem;
use std::future::Future;
use std::pin::Pin;

pub struct InitCommand {
    args: InitArgs,
}

impl Default for InitCommand {
    fn default() -> Self {
        InitCommand::from(InitArgs::default())
    }
}

impl From<InitArgs> for InitCommand {
    fn from(args: InitArgs) -> Self {
        InitCommand { args }
    }
}

impl Command for InitCommand {
    fn validate(&mut self) -> Result<(), Error> {
        let mut exist = self.args.config.source.try_exists().map_err(|err| Error::IoError(IoError::from(&self.args.config.source, err)))?;
        if !exist {
            return Err(InvalidSourceDirectory());
        }
        exist = self.args.config.target.try_exists().map_err(|err| Error::IoError(IoError::from(&self.args.config.target, err)))?;
        if !exist {
            return Err(InvalidBackupDirectory());
        }
        Ok(())
    }

    fn help(&self) {
        println!(
            "Usage: {} init [OPTIONS] <SOURCE> <BACKUP_PATH>\n\n\
        Initialize a new directory as backup target.\n\n\
        Options:\n\
        \t-c, --config\t\t\t\tyaml configuration file to use\n\
        \t-e, --exclude <PATTERN>\t\t\tlist of patterns to exclude from the backup\n\
        \t-t, --no-history <PATTERN>\t\tlist of patterns for which history won't be kept\n\
        \t-i, --incremental\t\t\tincremental mode, keep versions (default)\n\
        \t    --no-incremental\t\t\tnon incremental mode (keep a single version of each file)\n\
        \t-a, --hash-comparison\t\t\tcompare hash after copy (default, slower)\n\
        \t    --no-hash-comparison\t\tdo not compare hash after copy (faster)\n\
        \t-s, --follow-symlinks\t\t\tfollow symlinks in source directory\n\
        \t    --no-follow-symlinks\t\tdo not follow symlinks (default)\n\
        \t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)\n\
        \t-v, --verbose\t\t\t\tdisplay more detailed information\n\n\
        Examples of patterns (regex):\n\
        \t*.tmp:\t\t\t\t\t--exclude \".*\\.tmp\"\n\
        \tmacOS trash:\t\t\t\t--exclude: \".*/\\.DS_Store$\"",
            self.args.exe
        );
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + '_>> {
        Box::pin(async move {
            if self.args.verbose {
                println!("init command started");
                println!("Running {}", self.args);
            }
            FileSystem::check_home_dir().await?;
            self.create_directories().await?;
            self.args.config.write().await?;
            if self.args.verbose {
                println!("Configuration file created");
            }
            Database::open(&self.args.config.target).await?;
            if self.args.verbose {
                println!("Metadata file created");
            }
            if !self.args.quiet {
                println!("Backup directory {} successfully initialized", String::from(self.args.config.target.to_str().unwrap()))
            }
            command::stop(&self.args.config.target).await
        })
    }
}

impl InitCommand {
    async fn create_directories(&mut self) -> Result<(), Error> {
        // target directory must not already contain a burst directory
        let mut target = FileSystem::target_backup_dir(&self.args.config.target);
        let mut exist = tokio::fs::try_exists(&target).await.map_err(|err| Error::IoError(IoError::from(&self.args.config.target, err)))?;
        if exist {
            return Err(Error::AlreadyInitialized());
        }
        tokio::fs::create_dir_all(&target).await.map_err(|err| Error::IoError(IoError::from(&target, err)))?;
        // create local backup directory
        target = FileSystem::home_backup_dir(&self.args.config.target);
        exist = tokio::fs::try_exists(&target).await.map_err(|err| Error::IoError(IoError::from(&self.args.config.target, err)))?;
        if exist {
            return Err(Error::AlreadyInitialized());
        }
        tokio::fs::create_dir_all(&target).await.map_err(|err| Error::IoError(IoError::from(&target, err)))?;
        Ok(())
    }
}
