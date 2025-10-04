use crate::cmds::constants::BURST_CONFIG_FILE;
use crate::tools::cmderror::CmdError::{self, InvalidOption};
use crate::tools::cmderror::IoError;
use crate::tools::fs::FileSystem;
use hashlink::linked_hash_map::LinkedHashMap;
use regex::Regex;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::string::String;
use yaml_rust2::{Yaml, YamlLoader};

pub struct BackupConfig {
    pub source: PathBuf,
    pub target: PathBuf,
    pub exclude: Vec<String>,
    pub re_excl: Vec<Regex>,
    pub no_history: Vec<String>,
    pub re_hist: Vec<Regex>,
    pub incremental: bool,
    pub hash_comparison: bool,
    pub follow_symlinks: bool,
}

impl Default for BackupConfig {
    fn default() -> Self {
        BackupConfig {
            source: PathBuf::new(),
            target: PathBuf::new(),
            exclude: Vec::new(),
            re_excl: Vec::new(),
            no_history: Vec::new(),
            re_hist: Vec::new(),
            incremental: true,
            hash_comparison: true,
            follow_symlinks: false,
        }
    }
}

impl fmt::Display for BackupConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.exclude.is_empty() {
            write!(f, "--exclude \"")?;
            let mut x = String::new();
            for entry in &self.exclude {
                x.push_str(entry);
                x.push(',')
            }
            write!(f, "{}\" ", x.trim_end_matches(','))?;
        }
        if !self.no_history.is_empty() {
            write!(f, "--no-history \"")?;
            let mut h = String::new();
            for entry in &self.no_history {
                h.push_str(entry);
                h.push(',')
            }
            write!(f, "{}\" ", h.trim_end_matches(','))?;
        }
        if !self.incremental {
            write!(f, "--no-incremental ")?;
        }
        if self.hash_comparison {
            write!(f, "--hash-comparison ")?;
        }
        if self.follow_symlinks {
            write!(f, "--follow-symlinks ")?;
        }
        write!(f, "{} {}", String::from(self.source.to_str().unwrap()), String::from(self.target.to_str().unwrap()))
    }
}

impl BackupConfig {
    pub fn read(&mut self, filename: &Path) -> Result<(), CmdError> {
        let cfg = fs::read_to_string(filename).map_err(|err| CmdError::IoError(IoError::from(filename, err)))?;
        let yaml = YamlLoader::load_from_str(&cfg).map_err(|err| CmdError::IoError(IoError::from(filename, std::io::Error::other(err))))?;
        if let Some(map) = yaml[0].as_hash() {
            self.parse(map)?;
            return Ok(());
        }
        Err(CmdError::GenericError(String::from("Cannot read configuration")))
    }

    fn parse(&mut self, map: &LinkedHashMap<Yaml, Yaml>) -> Result<(), CmdError> {
        for (key, value) in map.iter() {
            match key.as_str() {
                Some("source") => {
                    if let Some(dir) = value.as_str() {
                        self.source.push(dir);
                    }
                }
                Some("exclude") => {
                    if let Some(value) = value.as_vec() {
                        for value in value.iter() {
                            if let Some(value) = value.as_str()
                                && !value.is_empty()
                            {
                                let re = Regex::new(value).map_err(|_| InvalidOption(format!("Invalid regex {}", value)))?;
                                self.exclude.push(String::from(value));
                                self.re_excl.push(re);
                            }
                        }
                    }
                }
                Some("no_history") => {
                    if let Some(value) = value.as_vec() {
                        for value in value.iter() {
                            if let Some(value) = value.as_str()
                                && !value.is_empty()
                            {
                                let re = Regex::new(value).map_err(|_| InvalidOption(format!("Invalid regex {}", value)))?;
                                self.no_history.push(String::from(value));
                                self.re_hist.push(re);
                            }
                        }
                    }
                }
                Some("incremental") => {
                    if let Some(value) = value.as_bool() {
                        self.incremental = value
                    }
                }
                Some("hash_comparison") => {
                    if let Some(value) = value.as_bool() {
                        self.hash_comparison = value
                    }
                }
                Some("follow_symlinks") => {
                    if let Some(value) = value.as_bool() {
                        self.follow_symlinks = value
                    }
                }
                _ => {
                    return Err(InvalidOption(format!("Invalid option {}", key.as_str().unwrap())));
                }
            }
        }
        Ok(())
    }

    pub fn write(&self) -> Result<(), CmdError> {
        let home_dir = FileSystem::home_backup_dir(&self.target);
        fs::write(home_dir.join(BURST_CONFIG_FILE), self.as_str())
            .map_err(|err| CmdError::IoError(IoError::from_str(BURST_CONFIG_FILE, err)))
    }

    pub fn as_str(&self) -> String {
        let mut yaml = String::from("# do not manually modify this file\n# use the config command instead.\n\nsource: \"");
        yaml.push_str(&self.source.to_str().unwrap().replace("\\", "\\\\"));
        yaml.push_str("\"\n\nincremental: ");
        yaml.push_str(&self.incremental.to_string());
        yaml.push_str("\n\nhash_comparison: ");
        yaml.push_str(&self.hash_comparison.to_string());
        yaml.push_str("\n\nfollow_symlinks: ");
        yaml.push_str(&self.follow_symlinks.to_string());
        if !self.exclude.is_empty() {
            yaml.push_str("\n\nexclude:");
            for exclude in &self.exclude {
                yaml.push_str("\n  - \"");
                yaml.push_str(&exclude.replace("\\", "\\\\"));
                yaml.push('\"');
            }
        }
        if !self.no_history.is_empty() {
            yaml.push_str("\n\nno_history:");
            for no_history in &self.no_history {
                yaml.push_str("\n  - \"");
                yaml.push_str(&no_history.replace("\\", "\\\\"));
                yaml.push('\"');
            }
        }
        yaml.push('\n');
        yaml
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        for re in &self.re_excl {
            if BackupConfig::matches(re, path) {
                return true;
            }
        }
        false
    }

    pub fn is_incremental(&self, path: &Path) -> bool {
        if !self.incremental {
            return false;
        }
        for re in &self.re_hist {
            if BackupConfig::matches(re, path) {
                return false;
            }
        }
        true
    }

    pub fn matches(re: &Regex, path: &Path) -> bool {
        re.is_match(path.to_str().unwrap())
    }
}
