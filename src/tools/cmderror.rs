use std::fmt::{Display, Formatter, Result};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum CmdError {
    GenericError(String),
    IoError(IoError),
    DbError(DbError),
    InvalidBackupDirectory(),
    InvalidSourceDirectory(),
    InvalidRestoreDirectory(),
    AlreadyInitialized(),
    MissingCommand,
    InvalidParameters,
    InvalidParameter(String),
    InvalidOption(String),
    NoRemote(),
}

impl Display for CmdError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Self::GenericError(msg) => write!(f, "{}", msg),
            Self::IoError(err) => write!(f, "{}", err),
            Self::DbError(err) => write!(f, "{}", err),
            Self::InvalidBackupDirectory() => write!(f, "Directory does not exist or is not a valid backup directory"),
            Self::InvalidSourceDirectory() => write!(f, "Directory does not exist or is not a valid source directory"),
            Self::InvalidRestoreDirectory() => write!(f, "Directory does not exist or is not a valid restore directory"),
            Self::AlreadyInitialized() => write!(f, "Backup directory has already been initialized"),
            Self::MissingCommand => write!(f, "No command provided"),
            Self::InvalidParameters => write!(f, "Invalid parameters"),
            Self::InvalidParameter(param) => write!(f, "Invalid or unknown parameter {}", param),
            Self::InvalidOption(msg) => write!(f, "Invalid option {}", msg),
            Self::NoRemote() => write!(f, ""),
        }
    }
}

impl std::error::Error for CmdError {}

#[derive(Debug)]
pub struct DbError {
    pub source: rusqlite::Error,
    pub culprit: String,
}

impl Display for DbError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}: {}", self.culprit, self.source)
    }
}

impl DbError {
    pub fn from(culprit: &str, err: rusqlite::Error) -> Self {
        DbError { culprit: String::from(culprit), source: err }
    }
}

#[derive(Debug)]
pub struct IoError {
    pub source: std::io::Error,
    pub culprit: PathBuf,
}

impl Display for IoError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{:?}: {}", self.culprit, self.source)
    }
}

impl IoError {
    pub fn from(culprit: &Path, err: std::io::Error) -> Self {
        IoError { culprit: PathBuf::from(culprit), source: err }
    }

    pub fn from_str(culprit: &str, err: std::io::Error) -> Self {
        IoError { culprit: PathBuf::from(culprit), source: err }
    }
}
