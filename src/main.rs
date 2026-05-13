use burst::cmds::{command, help::HelpCommand};
use burst::tools::error::Error::MissingCommand;
use std::{env, process::exit};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    match command::get_command(&args).await {
        Ok(mut c) => match c.validate() {
            Ok(_) => match c.run().await {
                Ok(_) => (),
                Err(err) => {
                    eprintln!("{}\n", err);
                    exit(1);
                }
            },
            Err(err) => {
                eprintln!("{}\n", err);
                c.help();
                exit(2);
            }
        },
        Err(err) => match err {
            MissingCommand => {
                eprintln!("{}\n", err);
                HelpCommand::default_help(&args[0]);
                exit(0);
            }
            _ => {
                eprintln!("{}\n", err);
                exit(1);
            }
        },
    }
    exit(0);
}
