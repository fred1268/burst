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
use crate::tools::cmderror::CmdError::{self, NoRemote};
use crate::tools::cmderror::IoError;
use crate::tools::fs::FileSystem;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub trait Command {
    fn validate(&mut self) -> Result<(), CmdError>;
    fn help(&self);
    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + '_>>;
}

pub async fn get_command(args: &[String]) -> Result<Box<dyn Command>, CmdError> {
    let cmd_args = Args::from_args(args).await?;
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

pub async fn start(target: &Path) -> Result<(), CmdError> {
    let home_dir = FileSystem::home_backup_dir(target);
    let target_dir = target.join(BURST_DIRECTORY);
    let mut exist = tokio::fs::try_exists(&home_dir).await.map_err(|err| CmdError::IoError(IoError::from(&home_dir, err)))?;
    if !exist {
        return Err(CmdError::InvalidBackupDirectory());
    }

    let local_cfg = home_dir.join(BURST_CONFIG_FILE);
    let local_db = home_dir.join(BURST_METADATA_FILE);
    let remote_cfg = target_dir.join(BURST_CONFIG_FILE);
    let remote_db = target_dir.join(BURST_METADATA_FILE);
    exist = tokio::fs::try_exists(&remote_cfg).await.map_err(|err| CmdError::IoError(IoError::from(&remote_cfg, err)))?;
    if !exist {
        return Err(NoRemote());
    }
    exist = tokio::fs::try_exists(&remote_db).await.map_err(|err| CmdError::IoError(IoError::from(&remote_db, err)))?;
    if !exist {
        return Err(NoRemote());
    }

    exist = tokio::fs::try_exists(&local_cfg).await.map_err(|err| CmdError::IoError(IoError::from(&local_cfg, err)))?;
    if !exist {
        tokio::fs::copy(&remote_cfg, &local_cfg)
            .await
            .map_err(|err| CmdError::IoError(IoError::from_str("Cannot copy configuration", err)))?;
    }
    exist = tokio::fs::try_exists(&local_db).await.map_err(|err| CmdError::IoError(IoError::from(&local_db, err)))?;
    if !exist {
        tokio::fs::copy(&remote_db, &local_db).await.map_err(|err| CmdError::IoError(IoError::from_str("Cannot copy metadata", err)))?;
    }
    Ok(())
}

pub async fn stop(target: &Path) -> Result<(), CmdError> {
    let home_dir = FileSystem::home_backup_dir(target);
    let target_dir = target.join(BURST_DIRECTORY);

    let local_cfg = home_dir.join(BURST_CONFIG_FILE);
    let local_db = home_dir.join(BURST_METADATA_FILE);
    let remote_cfg = target_dir.join(BURST_CONFIG_FILE);
    let remote_db = target_dir.join(BURST_METADATA_FILE);
    let exist = tokio::fs::try_exists(&target_dir).await.map_err(|err| CmdError::IoError(IoError::from(&target_dir, err)))?;
    if !exist {
        return Ok(());
    }

    tokio::fs::copy(&local_cfg, &remote_cfg).await.map_err(|err| CmdError::IoError(IoError::from_str("Cannot copy configuration", err)))?;
    tokio::fs::copy(&local_db, &remote_db).await.map_err(|err| CmdError::IoError(IoError::from_str("Cannot copy metadata", err)))?;
    Ok(())
}
