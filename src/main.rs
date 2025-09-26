use burst::cmds::{command, help::HelpCommand};
use burst::tools::cmderror::CmdError::MissingCommand;
use std::{env, process::exit};

fn main() {
    let args: Vec<String> = env::args().collect();
    match command::get_command(&args) {
        Ok(mut c) => match c.validate() {
            Ok(_) => match c.run() {
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
