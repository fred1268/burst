use crate::args::backupconfig::BackupConfig;
use crate::tools::cmderror::CmdError;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::string::String;

const MIN_PARAMS: usize = 3;

#[derive(Default, Clone)]
pub struct BackupArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub quiet: bool,
    pub verbose: bool,
    pub dry_run: bool,
    pub cont: bool,
}

impl fmt::Display for BackupArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} backup ", self.exe)?;
        if self.cont {
            write!(f, "--continue ")?;
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

impl BackupArgs {
    pub fn from_args(args: &[String]) -> Result<Self, CmdError> {
        if args.len() < MIN_PARAMS {
            return Err(CmdError::InvalidParameters);
        }
        let mut params = BackupArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--continue" | "-c" => params.cont = true,
                "--quiet" | "-q" => params.quiet = true,
                "--verbose" | "-v" => params.verbose = true,
                "--dry-run" | "-n" => params.dry_run = true,
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
        if params.dry_run {
            params.verbose = true;
        }
        if params.verbose {
            params.quiet = false;
        }
        Ok(params)
    }
}
