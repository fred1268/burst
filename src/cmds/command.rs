use crate::args::arg::Args;
use crate::cmds::backup::BackupCommand;
use crate::cmds::config::ConfigCommand;
use crate::cmds::constants::{BURST_CONFIG_FILE, BURST_DIRECTORY, BURST_METADATA_FILE};
use crate::cmds::delete::DeleteCommand;
use crate::cmds::help::HelpCommand;
use crate::cmds::history::HistoryCommand;
use crate::cmds::init::InitCommand;
use crate::cmds::list::ListCommand;
use crate::cmds::restore::RestoreCommand;
use crate::cmds::verify::VerifyCommand;
use crate::tools::cmderror::CmdError::{self, IoError, NoRemote};
use crate::tools::fs::FileSystem;
use std::fs;
use std::path::{Path, PathBuf};

pub trait Command {
    fn validate(&mut self) -> Result<(), CmdError>;
    fn help(&self);
    fn run(&mut self) -> Result<(), CmdError>;
}

pub fn get_command(args: &[String]) -> Result<Box<dyn Command>, CmdError> {
    let cmd_args = Args::from_args(args)?;
    match cmd_args {
        Args::Help(args) => Ok(Box::new(HelpCommand::from(args))),
        Args::Init(args) => Ok(Box::new(InitCommand::from(args))),
        Args::Backup(args) => Ok(Box::new(BackupCommand::from(args))),
        Args::List(args) => Ok(Box::new(ListCommand::from(args))),
        Args::History(args) => Ok(Box::new(HistoryCommand::from(args))),
        Args::Delete(args) => Ok(Box::new(DeleteCommand::from(args))),
        Args::Restore(args) => Ok(Box::new(RestoreCommand::from(args))),
        Args::Config(args) => Ok(Box::new(ConfigCommand::from(args))),
        Args::Verify(args) => Ok(Box::new(VerifyCommand::from(args))),
    }
}

pub fn config_file(target: &Path) -> PathBuf {
    FileSystem::home_backup_dir(target).join(BURST_CONFIG_FILE)
}

pub fn start(target: &Path) -> Result<(), CmdError> {
    let home_dir = FileSystem::home_backup_dir(target);
    let target_dir = target.join(BURST_DIRECTORY);
    let mut exist = fs::exists(&home_dir).map_err(|err| IoError(String::from(home_dir.to_str().unwrap()), err.to_string()))?;
    if !exist {
        return Err(CmdError::InvalidBackupDirectory());
    }

    let local_cfg = home_dir.join(BURST_CONFIG_FILE);
    let local_db = home_dir.join(BURST_METADATA_FILE);
    let remote_cfg = target_dir.join(BURST_CONFIG_FILE);
    let remote_db = target_dir.join(BURST_METADATA_FILE);
    exist = fs::exists(&remote_cfg).map_err(|err| IoError(String::from(remote_cfg.to_str().unwrap()), err.to_string()))?;
    if !exist {
        return Err(NoRemote());
    }
    exist = fs::exists(&remote_db).map_err(|err| IoError(String::from(remote_db.to_str().unwrap()), err.to_string()))?;
    if !exist {
        return Err(NoRemote());
    }

    exist = fs::exists(&local_cfg).map_err(|err| IoError(String::from(local_cfg.to_str().unwrap()), err.to_string()))?;
    if !exist {
        fs::copy(&remote_cfg, &local_cfg).map_err(|err| IoError(String::from("Configuration"), err.to_string()))?;
    }
    exist = fs::exists(&local_db).map_err(|err| IoError(String::from(local_db.to_str().unwrap()), err.to_string()))?;
    if !exist {
        fs::copy(&remote_db, &local_db).map_err(|err| IoError(String::from("Metadata"), err.to_string()))?;
    }
    Ok(())
}

pub fn stop(target: &Path) -> Result<(), CmdError> {
    let home_dir = FileSystem::home_backup_dir(target);
    let target_dir = target.join(BURST_DIRECTORY);

    let local_cfg = home_dir.join(BURST_CONFIG_FILE);
    let local_db = home_dir.join(BURST_METADATA_FILE);
    let remote_cfg = target_dir.join(BURST_CONFIG_FILE);
    let remote_db = target_dir.join(BURST_METADATA_FILE);
    let exist = fs::exists(&target_dir).map_err(|err| IoError(String::from(target_dir.to_str().unwrap()), err.to_string()))?;
    if !exist {
        return Ok(());
    }

    fs::copy(&local_cfg, &remote_cfg).map_err(|err| IoError(String::from("Configuration"), err.to_string()))?;
    fs::copy(&local_db, &remote_db).map_err(|err| IoError(String::from("Metadata"), err.to_string()))?;
    Ok(())
}
