use crate::args::backupconfig::BackupConfig;
use crate::tools::cmderror::CmdError;
use crate::tools::fs::FileSystem;
use regex::Regex;
use std::fmt;
use std::path::PathBuf;
use std::string::String;

const MIN_PARAMS: usize = 4;

#[derive(Default)]
pub struct InitArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub quiet: bool,
    pub verbose: bool,
}

impl fmt::Display for InitArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} init ", self.exe)?;
        if self.quiet {
            write!(f, "--quiet ")?;
        }
        if self.verbose {
            write!(f, "--verbose ")?;
        }
        write!(f, "{}", self.config)
    }
}

impl InitArgs {
    pub async fn from_args(args: &[String]) -> Result<Self, CmdError> {
        if args.len() < MIN_PARAMS {
            return Err(CmdError::InvalidParameters);
        }
        let mut params = InitArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        // load config from file
        while n < args.len() {
            match args[n].as_str() {
                "--config" | "-c" => {
                    params.config.read(&PathBuf::from(&args[n + 1])).await?;
                    n += 1;
                }
                _ => n += 1,
            }
        }
        n = 2;
        // overwrite config with command line parameters
        while n < args.len() - 2 {
            match args[n].as_str() {
                "--config" | "-c" => {
                    n += 1;
                }
                "--exclude" | "-e" => {
                    for value in args[n + 1].split(',') {
                        match Regex::new(value) {
                            Ok(re) => {
                                params.config.exclude.push(String::from(value));
                                params.config.re_excl.push(re)
                            }
                            Err(_) => return Err(CmdError::InvalidParameter(args[n + 1].clone())),
                        }
                    }
                    n += 1;
                }
                "--no-history" | "-t" => {
                    for value in args[n + 1].split(',') {
                        match Regex::new(value) {
                            Ok(re) => {
                                params.config.no_history.push(String::from(value));
                                params.config.re_hist.push(re)
                            }
                            Err(_) => return Err(CmdError::InvalidParameter(args[n + 1].clone())),
                        }
                    }
                    n += 1;
                }
                "--incremental" | "-i" => params.config.incremental = true,
                "--no-incremental" => params.config.incremental = false,
                "--hash-comparison" | "-a" => params.config.hash_comparison = true,
                "--no-hash-comparison" => params.config.hash_comparison = false,
                "--follow-symlinks" | "-s" => params.config.follow_symlinks = true,
                "--no-follow-symlinks" => params.config.follow_symlinks = false,
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
        if !&args[args.len() - 2].starts_with("--") {
            match FileSystem::canonicalize(&args[args.len() - 2]).await {
                Ok(source) => params.config.source = source,
                Err(_) => return Err(CmdError::InvalidSourceDirectory()),
            }
        }
        if !&args[args.len() - 1].starts_with("--") {
            match FileSystem::canonicalize(&args[args.len() - 1]).await {
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
