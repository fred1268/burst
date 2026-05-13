use chrono::{DateTime, Local, NaiveDate};

use crate::args::backupconfig::BackupConfig;
use crate::tools::error::Error;
use crate::tools::fs::FileSystem;
use std::fmt;
use std::string::String;

const MIN_PARAMS: usize = 4;

#[derive(Default)]
pub struct DeleteArgs {
    pub config: BackupConfig,
    pub exe: String,
    pub sids: Vec<u64>,
    pub pattern: String,
    pub older_than: DateTime<Local>,
    pub keep_last: u64,
    pub quiet: bool,
    pub verbose: bool,
    pub dry_run: bool,
}

impl fmt::Display for DeleteArgs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} delete ", self.exe)?;
        if !self.sids.is_empty() {
            write!(f, "--snapshot ")?;
            let mut ids = String::new();
            for id in &self.sids {
                ids.push_str(&format!("{}", id));
                ids.push(',')
            }
            write!(f, "{} ", ids.trim_end_matches(','))?;
        }
        if !self.pattern.is_empty() {
            write!(f, "--pattern {} ", self.pattern)?;
        }
        if self.keep_last != 0 {
            write!(f, "--keep-last {} ", self.keep_last)?;
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

impl DeleteArgs {
    pub async fn from_args(args: &[String]) -> Result<Self, Error> {
        if args.len() < MIN_PARAMS {
            return Err(Error::InvalidParameters);
        }
        let mut params = DeleteArgs::default();
        params.exe.push_str(&args[0]);
        let mut n: usize = 2;
        while n < args.len() - 1 {
            match args[n].as_str() {
                "--snapshot" | "-s" => {
                    if args[n + 1].contains(',') {
                        let ids: Vec<_> = args[n + 1].split(',').collect();
                        for id in ids {
                            match id.parse::<u64>() {
                                Ok(sid) => params.sids.push(sid),
                                Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                            }
                        }
                    } else if args[n + 1].contains('-') {
                        if let Some(sep) = args[n + 1].rfind('-') {
                            let start = match args[n + 1][..sep].parse::<u64>() {
                                Ok(sid) => sid,
                                Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                            };
                            let end = match args[n + 1][sep + 1..].parse::<u64>() {
                                Ok(sid) => sid,
                                Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                            };
                            if start >= end {
                                return Err(Error::InvalidOption(format!("start ({}) must be strictly lower than end ({})", start, end)));
                            }
                            for sid in start..end + 1 {
                                params.sids.push(sid);
                            }
                        }
                    } else {
                        match args[n + 1].parse::<u64>() {
                            Ok(sid) => params.sids.push(sid),
                            Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                        }
                    }
                    n += 1;
                }
                "--older-than" | "-o" => {
                    match NaiveDate::parse_from_str(&args[n + 1], "%Y-%m-%d") {
                        Ok(date) => {
                            params.older_than = date.and_hms_opt(0, 0, 0).unwrap().and_local_timezone(Local).unwrap();
                        }
                        Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                    }
                    n += 1;
                }
                "--keep-last" | "-l" => {
                    match args[n + 1].parse::<u64>() {
                        Ok(last) => params.keep_last = last,
                        Err(_) => return Err(Error::InvalidParameter(args[n + 1].clone())),
                    }
                    n += 1;
                }
                "--pattern" | "-p" => {
                    params.pattern.push_str(&args[n + 1]);
                    n += 1;
                }
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
