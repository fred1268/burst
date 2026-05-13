use crate::args::help::HelpArgs;
use crate::cmds::backup::BackupCommand;
use crate::cmds::command::Command;
use crate::cmds::config::ConfigCommand;
use crate::cmds::delete::DeleteCommand;
use crate::cmds::history::HistoryCommand;
use crate::cmds::init::InitCommand;
use crate::cmds::list::ListCommand;
use crate::cmds::restore::RestoreCommand;
use crate::cmds::verify::VerifyCommand;
use crate::tools::error::Error;
use std::future::Future;
use std::pin::Pin;

pub struct HelpCommand {
    args: HelpArgs,
}

impl Default for HelpCommand {
    fn default() -> Self {
        HelpCommand::from(HelpArgs::default())
    }
}

impl From<HelpArgs> for HelpCommand {
    fn from(args: HelpArgs) -> Self {
        HelpCommand { args }
    }
}

impl Command for HelpCommand {
    fn validate(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn help(&self) {
        HelpCommand::help(&self.args.exe);
        println!(
            "Commands:\n\
        \thelp\t\tGet this list of commands or help about a specific command\n\
        \tinit\t\tInitialize a new directory as backup target\n\
        \tbackup\t\tBackup files to specified directory using this directory's configuration.\n\
        \tlist\t\tShows files in a snapshot or the specified files history\n\
        \thistory\t\tShows backup snapshot timeline with statistics\n\
        \tdelete\t\tRemoves backup history selectively to manage storage space and retention policies\n\
        \trestore\t\tRestores files from backup snapshot to specified location\n\
        \tconfig\t\tManages backup repository configuration and performs configuration-related operations\n\
        \tverify\t\tVerifies backup integrity and files consistency"
        );
    }

    fn run(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + '_>> {
        Box::pin(async move {
            match self.args.command.as_str() {
                "" => self.help(),
                "init" => {
                    let cmd = InitCommand::default();
                    cmd.help();
                }
                "backup" => {
                    let cmd = BackupCommand::default();
                    cmd.help();
                }
                "list" => {
                    let cmd = ListCommand::default();
                    cmd.help();
                }
                "history" => {
                    let cmd = HistoryCommand::default();
                    cmd.help();
                }
                "delete" => {
                    let cmd = DeleteCommand::default();
                    cmd.help();
                }
                "restore" => {
                    let cmd = RestoreCommand::default();
                    cmd.help();
                }
                "config" => {
                    let cmd = ConfigCommand::default();
                    cmd.help();
                }
                "verify" => {
                    let cmd = VerifyCommand::default();
                    cmd.help();
                }
                _ => (),
            }
            Ok(())
        })
    }
}

impl HelpCommand {
    pub fn help(exe: &str) {
        println!(
            "Usage: {} <COMMAND> [OPTIONS]\n\n\
        Burst - An opinionated, cross‑platform backup CLI written in Rust.\n",
            exe
        );
    }

    pub fn default_help(exe: &String) {
        HelpCommand::help(exe);
        println!("Try running '{} help' to get the list of commands.", exe)
    }
}
