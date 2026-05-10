use crate::cmds::constants::BURST_DIRECTORY;
use crate::cmds::file::File;
use crate::tools::cmderror::{CmdError, IoError};
use sha2::{Digest, Sha256};
use std::env;
use std::io::ErrorKind;
use std::path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path, PathBuf};
use tokio::io::AsyncReadExt;

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

    pub async fn canonicalize(path: &str) -> Result<PathBuf, CmdError> {
        let dir = match path.ends_with(MAIN_SEPARATOR) {
            true => PathBuf::from(path.strip_suffix(MAIN_SEPARATOR).unwrap()),
            false => PathBuf::from(path),
        };
        let exist = tokio::fs::try_exists(&dir).await.map_err(|err| CmdError::IoError(IoError::from_str(path, err)))?;
        if exist {
            return tokio::fs::canonicalize(&dir).await.map_err(|err| CmdError::IoError(IoError::from(&dir, err)));
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

    pub async fn check_home_dir() -> Result<(), CmdError> {
        let home = Self::home_dir();
        let exist = tokio::fs::try_exists(&home).await.map_err(|err| CmdError::IoError(IoError::from(&home, err)))?;
        if exist {
            return Ok(());
        }
        tokio::fs::create_dir_all(&home).await.map_err(|err| CmdError::IoError(IoError::from(&home, err)))
    }

    fn other_os_separator() -> char {
        match MAIN_SEPARATOR {
            '\\' => '/',
            '/' => '\\',
            _ => '\0',
        }
    }

    pub async fn copy_new_file(&self, file: &mut File, compare_hash: bool) -> Result<(), CmdError> {
        let to = file.archive_dir(&self.target);
        if !tokio::fs::try_exists(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))? {
            tokio::fs::create_dir_all(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))?;
        }
        tokio::fs::copy(file.source_name(&self.source), to.join(&file.name)).await.map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        if compare_hash {
            let digest = self.compute_digest(file, &self.target).await?;
            if digest != file.digest {
                println!("Warning: hash comparison failed for {:?}. Retrying", file.fullname());
                tokio::fs::copy(file.source_name(&self.source), to.join(&file.name))
                    .await
                    .map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
                let hash = self.compute_digest(file, &self.target).await?;
                if hash != file.digest {
                    println!("Error: hash comparison failed for {:?}", file.fullname());
                }
            }
        }
        Ok(())
    }

    pub async fn compute_digest(&self, file: &File, path: &Path) -> Result<String, CmdError> {
        let mut hasher = Sha256::new();
        let mut f = tokio::fs::File::open(file.source_name(path)).await.map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        let mut buffer: [u8; BUF_SIZE] = [0u8; BUF_SIZE];
        loop {
            let read = f.read(&mut buffer[..]).await.map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
            hasher.update(&buffer[0..read]);
            if read < BUF_SIZE {
                break;
            }
        }
        let result = hasher.finalize();
        Ok(format!("{:x}", result))
    }

    pub async fn archive_file(&self, _sid: u64, file: &File) -> Result<(), CmdError> {
        let to: PathBuf = file.archive_dir(&self.target);
        if !tokio::fs::try_exists(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))? {
            tokio::fs::create_dir_all(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))?;
        }
        tokio::fs::rename(file.source_name(&self.target), to.join(&file.name)).await.map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))
    }

    pub async fn unarchive_file(&self, _sid: u64, file: &File) -> Result<(), CmdError> {
        tokio::fs::rename(file.archive_name(&self.target), file.source_name(&self.target))
            .await
            .map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        if let Some(parent) = file.archive_name(&self.target).parent() {
            self.recurse_remove_empty_dir(parent).await?;
        }
        Ok(())
    }

    pub async fn remove_file(&self, file: &File) -> Result<(), CmdError> {
        match file.is_dir {
            true => self.remove_dir(file).await,
            false => {
                tokio::fs::remove_file(file.archive_name(&self.target)).await.map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
                if let Some(parent) = file.archive_name(&self.target).parent() {
                    self.recurse_remove_empty_dir(parent).await?;
                }
                Ok(())
            }
        }
    }

    pub async fn remove_dir(&self, dir: &File) -> Result<(), CmdError> {
        self.recurse_remove_empty_dir(&dir.archive_name(&self.target)).await
    }

    pub async fn remove_archive_dir(&self, dir: &File) -> Result<(), CmdError> {
        self.recurse_remove_empty_dir(&dir.archive_name(&self.target)).await
    }

    pub async fn restore_file(&self, file: &File, to_dir: &Path, flatten: bool, overwrite: bool) -> Result<bool, CmdError> {
        let to = match flatten {
            true => to_dir.join(&file.name),
            false => {
                let to = to_dir.join(&file.path);
                if !tokio::fs::try_exists(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))? {
                    tokio::fs::create_dir_all(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))?;
                }
                to.join(&file.name)
            }
        };
        let conflict = tokio::fs::try_exists(&to).await.map_err(|err| CmdError::IoError(IoError::from(&to, err)))? && !overwrite;
        if !conflict {
            tokio::fs::copy(file.archive_name(&self.target), &to).await.map_err(|err| CmdError::IoError(IoError::from(&file.name, err)))?;
        }
        Ok(conflict)
    }

    async fn recurse_remove_empty_dir(&self, path: &Path) -> Result<(), CmdError> {
        let mut dir = path;
        loop {
            match tokio::fs::remove_dir(dir).await {
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

    pub async fn exists(&self, file: &File, path: &Path) -> Result<bool, CmdError> {
        let str = String::from(file.archive_name(path).to_str().unwrap());
        let str2 = str.replace(Self::other_os_separator(), MAIN_SEPARATOR_STR);
        tokio::fs::try_exists(str2).await.map_err(|err| CmdError::IoError(IoError::from_str(&str, err)))
    }
}
