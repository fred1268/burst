use crate::args::backupconfig::BackupConfig;
use crate::tools::cmderror::CmdError;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::string::String;

const MIN_PARAMS: usize = 3;

pub struct HistoryArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub limit: u64,
    pub quiet: bool,
    pub verbose: bool,
}

impl Default for HistoryArgs {
    fn default() -> Self {
        HistoryArgs { config: BackupConfig::default(), exe: String::new(), limit: 10, quiet: false, verbose: false }
    }
}

impl fmt::Display for HistoryArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} list ", self.exe)?;
        if self.limit != 0 {
            write!(f, "--limit {} ", self.limit)?;
        }
        if self.quiet {
            write!(f, "--quiet ")?;
        }
        if self.verbose {
            write!(f, "--verbose ")?;
        }
        write!(f, "{}", String::from(self.config.target.to_str().unwrap()))
    }
}

impl HistoryArgs {
    pub fn from_args(args: &[String]) -> Result<Self, CmdError> {
        if args.len() < MIN_PARAMS {
            return Err(CmdError::InvalidParameters);
        }
        let mut params = HistoryArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--limit" | "-n" => {
                    match args[n + 1].parse::<u64>() {
                        Ok(limit) => params.limit = limit,
                        Err(_) => return Err(CmdError::InvalidParameter(args[n + 1].clone())),
                    }
                    n += 1;
                }
                "--quiet" | "-q" => params.quiet = true,
                "--verbose" | "-v" => params.verbose = true,
                _ => {
                    if args[n].starts_with("--") {
                        return Err(CmdError::InvalidParameter(args[n].clone()));
                    }
                }
            }
            n += 1
        }
        if !args[args.len() - 1].starts_with("--") {
            match FileSystem::canonicalize(&args[args.len() - 1]) {
                Ok(target) => params.config.target = target,
                Err(_) => return Err(CmdError::InvalidBackupDirectory()),
            }
        }
        if params.verbose {
            params.quiet = false;
        }
        Ok(params)
    }
}
