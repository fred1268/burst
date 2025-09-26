use crate::args::backupconfig::BackupConfig;
use crate::tools::cmderror::CmdError;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::string::String;

const MIN_PARAMS: usize = 3;

#[derive(Default)]
pub struct ListArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub sid: u64,
    pub pattern: String,
    pub diff_sid: u64,
    pub deleted: bool,
    pub quiet: bool,
    pub verbose: bool,
}

impl fmt::Display for ListArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} list ", self.exe)?;
        if self.sid != 0 {
            write!(f, "--snapshot {} ", self.sid)?;
        }
        if self.diff_sid != 0 {
            write!(f, "--diff-with {}", self.diff_sid)?;
        }
        if !self.pattern.is_empty() {
            write!(f, "--pattern {} ", self.pattern)?;
        }
        if self.deleted {
            write!(f, "--deleted ")?;
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

impl ListArgs {
    pub fn from_args(args: &[String]) -> Result<Self, CmdError> {
        if args.len() < MIN_PARAMS {
            return Err(CmdError::InvalidParameters);
        }
        let mut params = ListArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--snapshot" | "-s" => {
                    match args[n + 1].parse::<u64>() {
                        Ok(sid) => params.sid = sid,
                        Err(_) => return Err(CmdError::InvalidParameter(args[n + 1].clone())),
                    }
                    n += 1;
                }
                "--pattern" | "-p" => {
                    params.pattern.push_str(&args[n + 1]);
                    n += 1;
                }
                "--diff-with" | "-i" => {
                    match args[n + 1].parse::<u64>() {
                        Ok(sid) => params.diff_sid = sid,
                        Err(_) => return Err(CmdError::InvalidParameter(args[n + 1].clone())),
                    }
                    n += 1;
                }
                "--deleted" | "-d" => params.deleted = true,
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
