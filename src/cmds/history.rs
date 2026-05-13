use crate::args::history::HistoryArgs;
use crate::cmds::command;
use crate::cmds::command::Command;
use crate::cmds::snapshot::Snapshot;
use crate::tools::db::Database;
use crate::tools::error::Error;
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
    fn validate(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn help(&self) {
        println!(
            "Usage: {} history [OPTIONS] <BACKUP_PATH>\n\n\
        Shows backup snapshot timeline with statistics.\n\n\
        Options:\n\
        \t-n, --limit\t\t\t\tLimit number of backups shown\n\
        \t-q, --quiet\t\t\t\tdisplay less information than usual (only errors)\n\
        \t-v, --verbose\t\t\t\tdisplay more detailed information",
            self.args.exe
        );
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + '_>> {
        Box::pin(async move {
            if self.args.verbose {
                println!("history command started");
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
