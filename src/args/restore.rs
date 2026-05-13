use crate::args::backupconfig::BackupConfig;
use crate::tools::error::Error;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::path::PathBuf;
use std::string::String;

const MIN_PARAMS: usize = 4;

#[derive(Default)]
pub struct RestoreArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub to: PathBuf,
    pub sid: u64,
    pub pattern: String,
    pub overwrite: bool,
    pub flatten: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub dry_run: bool,
}

impl fmt::Display for RestoreArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} list ", self.exe)?;
        if self.sid != 0 {
            write!(f, "--snapshot {} ", self.sid)?;
        }
        if !self.pattern.is_empty() {
            write!(f, "--pattern {} ", self.pattern)?;
        }
        if self.overwrite {
            write!(f, "--overwrite ")?;
        }
        if self.flatten {
            write!(f, "--flatten ")?;
        }
        if self.quiet {
            write!(f, "--quiet ")?;
        }
        if self.verbose {
            write!(f, "--verbose ")?;
        }
        if self.dry_run {
            write!(f, "--dry-run ")?;
        }
        write!(f, "{}", String::from(self.config.target.to_str().unwrap()))
    }
}

impl RestoreArgs {
    pub async fn from_args(args: &[String]) -> Result<Self, Error> {
        if args.len() < MIN_PARAMS {
            return Err(Error::InvalidParameters);
        }
        let mut params = RestoreArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--snapshot" | "-s" => {
                    match args[n + 1].parse::<u64>() {
                        Ok(sid) => params.sid = sid,
                        Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                    }
                    n += 1;
                }
                "--pattern" | "-p" => {
                    params.pattern.push_str(&args[n + 1]);
                    n += 1;
                }
                "--overwrite" | "-w" => params.overwrite = true,
                "--flatten" | "-t" => params.flatten = true,
                "--quiet" | "-q" => params.quiet = true,
                "--verbose" | "-v" => params.verbose = true,
                "--dry-run" | "-n" => params.dry_run = true,
                _ => {
                    if args[n].starts_with("--") {
                        return Err(Error::InvalidParameter(args[n].clone()));
                    }
                }
            }
            n += 1
        }
        if !args[args.len() - 2].starts_with("--") {
            match FileSystem::canonicalize(&args[args.len() - 2]).await {
                Ok(target) => params.config.target = target,
                Err(_) => return Err(Error::InvalidBackupDirectory()),
            }
        }
        if !args[args.len() - 1].starts_with("--") {
            match FileSystem::canonicalize(&args[args.len() - 1]).await {
                Ok(to) => params.to = to,
                Err(_) => return Err(Error::InvalidRestoreDirectory()),
            }
        }
        if params.dry_run {
            params.verbose = true;
        }
        if params.verbose {
            params.quiet = false;
        }
        Ok(params)
    }
}
