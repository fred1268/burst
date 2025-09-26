use crate::args::{backup, help, init, list};
use crate::args::{config, delete, history, restore, verify};
use crate::tools::cmderror::CmdError::{self, InvalidParameters};
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
    pub fn from_args(args: &[String]) -> Result<Args, CmdError> {
        if args.len() < 2 {
            return Err(CmdError::MissingCommand);
        }
        match args[1].as_str() {
            "help" => help::HelpArgs::from_args(args).map(|cfg| Ok(Args::Help(cfg)))?,
            "init" => init::InitArgs::from_args(args).map(|cfg| Ok(Args::Init(cfg)))?,
            "backup" => backup::BackupArgs::from_args(args).map(|cfg| Ok(Args::Backup(cfg)))?,
            "list" => list::ListArgs::from_args(args).map(|cfg| Ok(Args::List(cfg)))?,
            "history" => history::HistoryArgs::from_args(args).map(|cfg| Ok(Args::History(cfg)))?,
            "delete" => delete::DeleteArgs::from_args(args).map(|cfg| Ok(Args::Delete(cfg)))?,
            "restore" => restore::RestoreArgs::from_args(args).map(|cfg| Ok(Args::Restore(cfg)))?,
            "config" => config::ConfigArgs::from_args(args).map(|cfg| Ok(Args::Config(cfg)))?,
            "verify" => verify::VerifyArgs::from_args(args).map(|cfg| Ok(Args::Verify(cfg)))?,
            _ => Err(InvalidParameters),
        }
    }
}
