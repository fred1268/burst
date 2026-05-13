use crate::args::backupconfig::BackupConfig;
use crate::tools::error::Error;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::string::String;

const MIN_PARAMS: usize = 3;

#[derive(Default)]
pub struct ConfigArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub subcommand: String,
    pub key: String,
    pub value: String,
    pub fix_history: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub dry_run: bool,
}

impl fmt::Display for ConfigArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} config ", self.exe)?;
        if !self.subcommand.is_empty() {
            write!(f, "{} ", self.subcommand)?;
        }
        if !self.key.is_empty() {
            write!(f, "{} ", self.key)?;
        }
        if !self.value.is_empty() {
            write!(f, "{} ", self.value)?;
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

impl ConfigArgs {
    pub async fn from_args(args: &[String]) -> Result<Self, Error> {
        if args.len() < MIN_PARAMS {
            return Err(Error::InvalidParameters);
        }
        let mut params = ConfigArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--fix-history" => params.fix_history = true,
                "--quiet" | "-q" => params.quiet = true,
                "--verbose" | "-v" => params.verbose = true,
                "--dry-run" | "-n" => params.dry_run = true,
                _ => {
                    if args[n].starts_with("--") {
                        return Err(Error::InvalidParameter(args[n].clone()));
                    } else {
                        params.subcommand.push_str(&args[n]);
                        match args[n].as_str() {
                            "get" | "convert" => {
                                if n < args.len() - 2 && !args[n + 1].starts_with("--") {
                                    params.key.push_str(&args[n + 1]);
                                    n += 1;
                                }
                            }
                            "set" | "add" | "remove" => {
                                if n < args.len() - 2 && !args[n + 1].starts_with("--") {
                                    params.key.push_str(&args[n + 1]);
                                    n += 1;
                                }
                                if n < args.len() - 2 && !args[n + 1].starts_with("--") {
                                    params.value.push_str(&args[n + 1]);
                                    n += 1;
                                }
                            }
                            "show" => (),
                            _ => {
                                return Err(Error::InvalidParameter(args[n].clone()));
                            }
                        }
                    }
                }
            }
            n += 1
        }
        if !args[args.len() - 1].starts_with("--") {
            match FileSystem::canonicalize(&args[args.len() - 1]).await {
                Ok(target) => params.config.target = target,
                Err(_) => return Err(Error::InvalidBackupDirectory()),
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
