use crate::args::{backup, help, init, list};
use crate::args::{config, delete, history, restore, verify};
use crate::tools::error::Error::{self, InvalidParameters};
use std::string::String;

pub enum Args {
    Help(help::HelpArgs),
    Init(init::InitArgs),
    Backup(backup::BackupArgs),
    List(list::ListArgs),
    History(history::HistoryArgs),
    Delete(delete::DeleteArgs),
    Restore(restore::RestoreArgs),
    Config(config::ConfigArgs),
    Verify(verify::VerifyArgs),
}

impl Args {
    pub async fn from_args(args: &[String]) -> Result<Args, Error> {
        if args.len() < 2 {
            return Err(Error::MissingCommand);
        }
        match args[1].as_str() {
            "help" => help::HelpArgs::from_args(args).await.map(|cfg| Ok(Args::Help(cfg)))?,
            "init" => init::InitArgs::from_args(args).await.map(|cfg| Ok(Args::Init(cfg)))?,
            "backup" => backup::BackupArgs::from_args(args).await.map(|cfg| Ok(Args::Backup(cfg)))?,
            "list" => list::ListArgs::from_args(args).await.map(|cfg| Ok(Args::List(cfg)))?,
            "history" => history::HistoryArgs::from_args(args).await.map(|cfg| Ok(Args::History(cfg)))?,
            "delete" => delete::DeleteArgs::from_args(args).await.map(|cfg| Ok(Args::Delete(cfg)))?,
            "restore" => restore::RestoreArgs::from_args(args).await.map(|cfg| Ok(Args::Restore(cfg)))?,
            "config" => config::ConfigArgs::from_args(args).await.map(|cfg| Ok(Args::Config(cfg)))?,
            "verify" => verify::VerifyArgs::from_args(args).await.map(|cfg| Ok(Args::Verify(cfg)))?,
            _ => Err(InvalidParameters),
        }
    }
}
