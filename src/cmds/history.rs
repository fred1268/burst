use crate::args::history::HistoryArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::snapshot::Snapshot;
use crate::tools::cmderror::CmdError;
use crate::tools::db::Database;
use std::future::Future;
use std::pin::Pin;

pub struct HistoryCommand {
    args: HistoryArgs,
}

impl Default for HistoryCommand {
    fn default() -> Self {
        HistoryCommand::from(HistoryArgs::default())
    }
}

impl From<HistoryArgs> for HistoryCommand {
    fn from(args: HistoryArgs) -> Self {
        HistoryCommand { args }
    }
}

impl Command for HistoryCommand {
    fn validate(&mut self) -> Result<(), CmdError> {
        Ok(())
    }

    fn help(&self) {
        println!("Usage: {} history [OPTIONS] <BACKUP_PATH>", self.args.exe);
        println!();
        println!("Shows backup snapshot timeline with statistics.");
        println!();
        println!("Options:");
        println!("\t-n, --limit\t\t\t\tLimit number of backups shown");
        println!("\t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)");
        println!("\t-v, --verbose\t\t\t\tdisplay more detailed information");
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), CmdError>> + '_>> {
        Box::pin(async move {
            if self.args.verbose {
                println!("history command started");
                println!("Running {}", self.args);
            }
            match command::start(&self.args.config.target).await {
                Ok(_) => (),
                Err(err) => match err {
                    CmdError::NoRemote() => (),
                    _ => return Err(err),
                },
            };
            self.args.config.read(&command::config_file(&self.args.config.target)).await?;
            let db = Database::open(&self.args.config.target).await?;
            let snapshots = Snapshot::get_last(&db, self.args.limit).await?;
            if !self.args.quiet {
                Snapshot::header();
            }
            for snapshot in snapshots {
                println!("{}", snapshot)
            }
            command::stop(&self.args.config.target).await
        })
    }
}
