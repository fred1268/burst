#[derive(Debug)]
pub enum CmdError {
    IoError(String, String),
    DbError(String, String),
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

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::IoError(file, err) => write!(f, "{}: {}", file, err),
            Self::DbError(file, err) => write!(f, "{}: {}", file, err),
            Self::InvalidBackupDirectory() => write!(f, "Directory does not exist or is not a valid backup directory"),
            Self::InvalidSourceDirectory() => write!(f, "Directory does not exist or is not a valid source directory"),
            Self::InvalidRestoreDirectory() => write!(f, "Directory does not exist or is not a valid restore directory"),
            Self::AlreadyInitialized() => write!(f, "Backup directory has already been initialized"),
            Self::MissingCommand => write!(f, "No command provided"),
            Self::InvalidParameters => write!(f, "Invalid parameters"),
            Self::InvalidParameter(param) => write!(f, "Invalid or unknown parameter {}", param),
            Self::InvalidOption(msg) => write!(f, "{}", msg),
            Self::NoRemote() => write!(f, ""),
        }
    }
}

impl std::error::Error for CmdError {}
