use std::fmt;

use crate::tools::cmderror::CmdError;

const MIN_PARAMS: usize = 2;

#[derive(Default)]
pub struct HelpArgs {
    pub exe: String,
    pub command: String,
}

impl fmt::Display for HelpArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} help {}", self.exe, self.command)
    }
}

impl HelpArgs {
    pub async fn from_args(args: &[String]) -> Result<Self, CmdError> {
        if args.len() < MIN_PARAMS {
            return Err(CmdError::InvalidParameters);
        }
        let mut params = HelpArgs::default();
        params.exe.push_str(&args[0]);
        if args.len() > 2 {
            match args[2].as_str() {
                "init" | "backup" | "restore" | "config" | "delete" | "list" | "history" | "verify" | "info" => {
                    params.command.push_str(&args[2])
                }
                _ => return Err(CmdError::InvalidParameter(args[2].clone())),
            }
        }
        Ok(params)
    }
}
