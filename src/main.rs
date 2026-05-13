use burst::cmds::{command, help::HelpCommand};
use burst::tools::error::Error::MissingCommand;
use std::{env, process::exit};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let mut cmd = command::get_command(&args).await.unwrap_or_else(|err| match err {
        MissingCommand => {
            eprintln!("{}\n", err);
            HelpCommand::default_help(&args[0]);
            exit(0);
        }
        _ => {
            eprintln!("{}\n", err);
            exit(1);
        }
    });
    if let Err(err) = cmd.validate() {
        eprintln!("{}\n", err);
        cmd.help();
        exit(2);
    }
    if let Err(err) = cmd.run().await {
        eprintln!("{}\n", err);
        exit(1);
    }
    exit(0);
}
