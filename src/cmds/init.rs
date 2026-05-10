use crate::args::init::InitArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::tools::cmderror::CmdError::{self, InvalidBackupDirectory, InvalidSourceDirectory};
use crate::tools::cmderror::IoError;
use crate::tools::db::Database;
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
    fn validate(&mut self) -> Result<(), CmdError> {
        let mut exist =
            self.args.config.source.try_exists().map_err(|err| CmdError::IoError(IoError::from(&self.args.config.source, err)))?;
        if !exist {
            return Err(InvalidSourceDirectory());
        }
        exist = self.args.config.target.try_exists().map_err(|err| CmdError::IoError(IoError::from(&self.args.config.target, err)))?;
        if !exist {
            return Err(InvalidBackupDirectory());
        }
        Ok(())
    }

    fn help(&self) {
        println!("Usage: {} init [OPTIONS] <SOURCE> <BACKUP_PATH>", self.args.exe);
        println!();
        println!("Initialize a new directory as backup target.");
        println!();
        println!("Options:");
        println!("\t-c, --config\t\t\t\tyaml configuration file to use");
        println!("\t-e, --exclude <PATTERN>\t\t\tlist of patterns to exclude from the backup");
        println!("\t-t, --no-history <PATTERN>\t\tlist of patterns for which history won't be kept");
        println!("\t-i, --incremental\t\t\tincremental mode, keep versions (default)");
        println!("\t    --no-incremental\t\t\tnon incremental mode (keep a single version of each file)");
        println!("\t-a, --hash-comparison\t\t\tcompare hash after copy (default, slower)");
        println!("\t    --no-hash-comparison\t\tdo not compare hash after copy (faster)");
        println!("\t-s, --follow-symlinks\t\t\tfollow symlinks in source directory");
        println!("\t    --no-follow-symlinks\t\tdo not follow symlinks (default)");
        println!("\t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)");
        println!("\t-v, --verbose\t\t\t\tdisplay more detailed information");
        println!();
        println!("Examples of patterns (regex):");
        println!("\t*.tmp:\t\t\t\t\t--exclude \".*\\.tmp\"");
        println!("\tmacOS trash:\t\t\t\t--exclude: \".*/\\.DS_Store$\"");
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + '_>> {
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
    async fn create_directories(&mut self) -> Result<(), CmdError> {
        // target directory must not already contain a burst directory
        let mut target = FileSystem::target_backup_dir(&self.args.config.target);
        let mut exist =
            tokio::fs::try_exists(&target).await.map_err(|err| CmdError::IoError(IoError::from(&self.args.config.target, err)))?;
        if exist {
            return Err(CmdError::AlreadyInitialized());
        }
        tokio::fs::create_dir_all(&target).await.map_err(|err| CmdError::IoError(IoError::from(&target, err)))?;
        // create local backup directory
        target = FileSystem::home_backup_dir(&self.args.config.target);
        exist = tokio::fs::try_exists(&target).await.map_err(|err| CmdError::IoError(IoError::from(&self.args.config.target, err)))?;
        if exist {
            return Err(CmdError::AlreadyInitialized());
        }
        tokio::fs::create_dir_all(&target).await.map_err(|err| CmdError::IoError(IoError::from(&target, err)))?;
        Ok(())
    }
}
