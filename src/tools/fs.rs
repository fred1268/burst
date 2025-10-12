use crate::cmds::constants::BURST_DIRECTORY;
use crate::cmds::file::File;
use crate::tools::cmderror::{CmdError, IoError};
use sha2::{Digest, Sha256};
use std::io::{BufReader, ErrorKind, Read};
use std::path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path, PathBuf};
use std::{env, fs};

const BUF_SIZE: usize = 65536;

pub struct FileSystem {
    source: PathBuf,
    target: PathBuf,
}

impl FileSystem {
    pub fn new(source: &Path, target: &Path) -> Self {
        FileSystem { source: PathBuf::from(source), target: PathBuf::from(target) }
    }

    fn home_dir() -> PathBuf {
        env::home_dir().unwrap().join(BURST_DIRECTORY)
    }

    pub fn home_backup_dir(target: &Path) -> PathBuf {
        Self::home_dir().join(Self::sanitize(target))
    }

    pub fn target_backup_dir(target: &Path) -> PathBuf {
        target.join(BURST_DIRECTORY)
    }

    pub fn canonicalize(path: &str) -> Result<PathBuf, CmdError> {
        let dir = match path.ends_with(MAIN_SEPARATOR) {
            true => PathBuf::from(path.strip_suffix(MAIN_SEPARATOR).unwrap()),
            false => PathBuf::from(path),
        };
        let exist = fs::exists(&dir).map_err(|err| CmdError::IoError(IoError::from_str(path, err)))?;
        if exist {
            return dir.canonicalize().map_err(|err| CmdError::IoError(IoError::from(&dir, err)));
        }
        if path.ends_with(MAIN_SEPARATOR) {
            return Ok(PathBuf::from(path.strip_suffix(MAIN_SEPARATOR).unwrap()));
        }
        Ok(dir)
    }

    fn sanitize(path: &Path) -> PathBuf {
        let mut str = String::from(path.to_str().unwrap());
        str = str.replace("\\\\?\\", "");
        str = str.replace("UNC\\", "");
        str = str.replace("\\\\", "");
        str = str.replace(":\\", "_");
        str = str.replace(MAIN_SEPARATOR, "_");
        str = str.replace('?', "_");
        PathBuf::from(str)
    }

    pub fn check_home_dir() -> Result<(), CmdError> {
        let home = Self::home_dir();
        let exist = fs::exists(&home).map_err(|err| CmdError::IoError(IoError::from(&home, err)))?;
        if exist {
            return Ok(());
        }
        fs::create_dir_all(&home).map_err(|err| CmdError::IoError(IoError::from(&home, err)))
    }

    fn other_os_separator() -> char {
        match MAIN_SEPARATOR {
            '\\' => '/',
            '/' => '\\',
            _ => '\0',
        }
    }

    pub fn copy_new_file(&self, file: &mut File, compare_hash: bool) -> Result<(), CmdError> {
        let to = file.archive_dir(&self.target);
        if !to.exists() {
            fs::create_dir_all(&to).map_err(|err| CmdError::IoError(IoError::from(&to, err)))?;
        }
        fs::copy(file.source_name(&self.source), to.join(&file.name)).map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        if compare_hash {
            let digest = self.compute_digest(file, &self.target)?;
            if digest != file.digest {
                println!("Warning: hash comparison failed for {:?}. Retrying", file.fullname());
                fs::copy(file.source_name(&self.source), to.join(&file.name))
                    .map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
                let hash = self.compute_digest(file, &self.target)?;
                if hash != file.digest {
                    println!("Error: hash comparison failed for {:?}", file.fullname());
                }
            }
        }
        Ok(())
    }

    pub fn compute_digest(&self, file: &File, path: &Path) -> Result<String, CmdError> {
        let mut hasher = Sha256::new();
        let f = fs::File::open(file.source_name(path)).map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        let mut reader = BufReader::new(f);
        let mut buffer: [u8; BUF_SIZE] = [0u8; BUF_SIZE];
        loop {
            let read = reader.read(&mut buffer[..]).map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
            hasher.update(&buffer[0..read]);
            if read < BUF_SIZE {
                break;
            }
        }
        let result = hasher.finalize();
        Ok(format!("{:x}", result))
    }

    pub fn archive_file(&self, _sid: u64, file: &File) -> Result<(), CmdError> {
        let to: PathBuf = file.archive_dir(&self.target);
        if !to.exists() {
            fs::create_dir_all(&to).map_err(|err| CmdError::IoError(IoError::from(&to, err)))?;
        }
        fs::rename(file.source_name(&self.target), to.join(&file.name)).map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))
    }

    pub fn unarchive_file(&self, _sid: u64, file: &File) -> Result<(), CmdError> {
        fs::rename(file.archive_name(&self.target), file.source_name(&self.target))
            .map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        if let Some(parent) = file.archive_name(&self.target).parent() {
            self.recurse_remove_empty_dir(parent)?;
        }
        Ok(())
    }

    pub fn remove_file(&self, file: &File) -> Result<(), CmdError> {
        match file.is_dir {
            true => self.remove_dir(file),
            false => {
                fs::remove_file(file.archive_name(&self.target)).map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
                if let Some(parent) = file.archive_name(&self.target).parent() {
                    self.recurse_remove_empty_dir(parent)?;
                }
                Ok(())
            }
        }
    }

    pub fn remove_dir(&self, dir: &File) -> Result<(), CmdError> {
        self.recurse_remove_empty_dir(&dir.archive_name(&self.target))
    }

    pub fn remove_archive_dir(&self, dir: &File) -> Result<(), CmdError> {
        self.recurse_remove_empty_dir(&dir.archive_name(&self.target))
    }

    pub fn restore_file(&self, file: &File, to_dir: &Path, flatten: bool, overwrite: bool) -> Result<bool, CmdError> {
        let to = match flatten {
            true => to_dir.join(&file.name),
            false => {
                let to = to_dir.join(&file.path);
                if !to.exists() {
                    fs::create_dir_all(&to).map_err(|err| CmdError::IoError(IoError::from(&to, err)))?;
                }
                to.join(&file.name)
            }
        };
        let conflict = to.exists() && !overwrite;
        if !conflict {
            fs::copy(file.archive_name(&self.target), &to).map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        }
        Ok(conflict)
    }

    fn recurse_remove_empty_dir(&self, path: &Path) -> Result<(), CmdError> {
        let mut dir = path;
        loop {
            match fs::remove_dir(dir) {
                Ok(_) => match dir.parent() {
                    Some(parent) => dir = parent,
                    None => break,
                },
                Err(err) => match err.kind() {
                    ErrorKind::DirectoryNotEmpty | ErrorKind::NotFound => break,
                    _ => return Err(CmdError::IoError(IoError::from(dir, err))),
                },
            }
        }
        Ok(())
    }

    pub fn exists(&self, file: &File, path: &Path) -> bool {
        let str = String::from(file.archive_name(path).to_str().unwrap());
        let str2 = str.replace(Self::other_os_separator(), MAIN_SEPARATOR_STR);
        if let Ok(exists) = fs::exists(str2) {
            return exists;
        }
        false
    }
}
