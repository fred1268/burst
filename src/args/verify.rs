use crate::args::backupconfig::BackupConfig;
use crate::tools::cmderror::CmdError;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::string::String;

const MIN_PARAMS: usize = 4;

#[derive(Default)]
pub struct VerifyArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub topic: String,
    pub fix: bool,
    pub quiet: bool,
    pub verbose: bool,
}

impl fmt::Display for VerifyArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} verify ", self.exe)?;
        if !self.topic.is_empty() {
            write!(f, "{} ", self.topic)?;
        }
        if self.fix {
            write!(f, "--fix ")?;
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

impl VerifyArgs {
    pub fn from_args(args: &[String]) -> Result<Self, CmdError> {
        if args.len() < MIN_PARAMS {
            return Err(CmdError::InvalidParameters);
        }
        let mut params = VerifyArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--fix" => params.fix = true,
                "--quiet" | "-q" => params.quiet = true,
                "--verbose" | "-v" => params.verbose = true,
                _ => {
                    if args[n].starts_with("--") {
                        return Err(CmdError::InvalidParameter(args[n].clone()));
                    } else {
                        params.topic.push_str(&args[n]);
                        match args[n].as_str() {
                            "hash" | "integrity" => (),
                            _ => return Err(CmdError::InvalidParameter(args[n].clone())),
                        }
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
