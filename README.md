# 🌟 Burst

An opinionated, cross‑platform backup CLI written in Rust. Burst is especially convenient when you’re backing up data to a removable drive, keeping everything organized and ready to unplug. It makes your backups easy to browse and reason about by mirroring your source tree in the backup location and keeping previous versions right next to the current data. It favors clear, predictable operations and fast, human‑readable listings backed by a small SQLite metadata database.

## 📝 Key Principles

- Mirror‑first layout: the backup directory mirrors your source; you can browse it like a normal folder.
- Local versions: previous versions live under `.versions/<snapshot_id>` alongside current files and folders.
- Snapshots by default: incremental snapshots preserve history; sync mode keeps only one version.
- Strong integrity: every file has a SHA‑256 digest; verify commands detect drift or corruption.
- Practical controls: dry‑run, quiet/verbose, regex‑based excludes, and per‑path “no history”.
- Small, portable core: SQLite metadata; works on Linux, macOS, and Windows.
- Safety first: destructive actions are explicit and opt‑in (e.g., `--fix-history`).

Repository internals live in a dedicated control directory at the backup root: `.burst/config.yaml` and `.burst/metadata.db`. Current content sits in the mirrored tree; archived versions sit under `.versions/<snapshot_id>` inside each directory.

## 🖫 Install

- Build from source: `cargo build --release` (binary at `target/release/burst`).
- Or install locally: `cargo install --path .`.

Requires a recent Rust toolchain.

## 🎬 Quick Start

### 1) Initialize a repository

```
# Create/prepare a backup at /backups/home, sourcing from /home/alice
burst init --exclude ".*\.tmp,\.*/\.DS_Store$,\.*/logs$" --no-history "manuals/" /home/alice /backups/home
```

### 2) Run a backup

```
# Uses the repository config saved at /backups/home/.burst/config.yaml
burst backup /backups/home
```

### 3) Explore and inspect

```
# List files from the latest snapshot
burst list /backups/home

# Show the last 5 snapshots
burst history --limit 5 /backups/home
```

### 4) Restore

```
# Restore a single file to /tmp/restore
burst restore --pattern "^docs/report\.md$" --overwrite /backups/home /tmp/restore

# Restore an entire directory from a specific snapshot
burst restore --snapshot 42 --pattern "^photos/2022(/.*)?$" /backups/home /tmp/restore
```

## 🩻 Concepts & Layout

- Control directory: `.burst/` at the backup root contains `config.yaml` and `metadata.db`.
- Current files: mirrored 1:1 with your source paths.
- Archived versions: when a file or directory changes (or is deleted) in incremental mode, the previous content is moved under `.versions/<snapshot_id>` within the same parent directory.

Example (simplified):

```
/backups/home
├── .burst/
│   ├── config.yaml
│   └── metadata.db
└── docs/
    ├── report.md               # current version
    └── .versions/
        └── 41/
            └── report.md       # version saved in snapshot 41
```

- Patterns are regular expressions: exclude and no‑history lists accept POSIX‑style regex (not glob).

## 🗹 Global Options

Most commands support common flags:

- `-q, --quiet`: reduce output to errors
- `-v, --verbose`: increase output detail

Many commands also support `-n, --dry-run` to preview work without touching the filesystem.

## 🛠️ Commands

### help

Show general help or help for a specific command.

```
burst help
burst help <COMMAND>
```

### init

Initialize a new backup repository at the target path, saving the configuration under `.burst/config.yaml` and creating `.burst/metadata.db`.

```
burst init [OPTIONS] <SOURCE> <BACKUP_PATH>
```

- `-c, --config <FILE>`: load initial YAML config from file
- `-e, --exclude <PATTERN[,PATTERN…]>`: add regex patterns to exclude
- `-t, --no-history <PATTERN[,PATTERN…]>`: back up but don’t keep versions for matching paths
- `-i, --incremental`: incremental mode (keep history; default)
- `--no-incremental`: sync mode (single version only)
- `-a, --hash-comparison`: verify copies by comparing SHA‑256 (default)
- `--no-hash-comparison`: skip hash compare after copy
- `-s, --follow-symlinks`: follow symlinks in source
- `--no-follow-symlinks`: do not follow symlinks (default)
- `-q, --quiet` | `-v, --verbose`

Examples

```
# Basic init with excludes and no-history
burst init --exclude ".*\.tmp" --no-history "logs/" /home/alice /backups/home

# Sync mode (no history preserved)
burst init --no-incremental /home/alice /backups/home

# Initialize from an existing config template file
echo "incremental: true" > template.yaml
burst init --config template.yaml /home/alice /backups/home
```

### backup

Create a snapshot (incremental by default) using the repository configuration saved at the target.

```
burst backup [OPTIONS] <BACKUP_PATH>
```

- `-c, --continue`: resume an interrupted backup process
- `-n, --dry-run`: preview changes
- `-q, --quiet` | `-v, --verbose`

Notes

- The repository must be initialized first (`init`).
- In sync mode, only the latest content remains; prior snapshot records are removed.
- A backup can fail if it is interrupted (e.g., Ctrl-C or process kill), on filesystem errors (permission denied, I/O errors, path issues), when disk space is exhausted, or if the metadata database is locked/corrupted. Use `--continue` to resume where possible.

### restore

Restore files and/or directories from a snapshot to a given destination.

```
burst restore [OPTIONS] <BACKUP_PATH> <RESTORE_PATH>
```

- `-s, --snapshot <ID>`: restore from a specific snapshot; if omitted, uses the latest
- `-p, --pattern <PATTERN>`: POSIX‑style regex to select files/directories (relative to backup root)
- `-w, --overwrite`: overwrite existing files at the destination
- `-t, --flatten`: ignore original directory paths; restore directly under `<RESTORE_PATH>`
- `-n, --dry-run`: preview without copying
- `-q, --quiet` | `-v, --verbose`

Examples

```
# Restore all PNG files from the latest snapshot
burst restore --pattern ".*\.png$" /backups/home /tmp/restore

# Restore PDFs inside any "manuals/" directory
burst restore --pattern "/manuals(/.*)?/.*\.pdf$" /backups/home /tmp/restore

# Restore from a specific snapshot (ID 42)
burst restore --snapshot 42 --pattern "^photos/2022(/.*)?$" /backups/home /tmp/restore

# Restore matching files into a single folder (flatten)
burst restore --flatten --pattern ".*\.log$" /backups/home /tmp/restore
```

### config

Inspect and update repository configuration, and perform controlled/destructive conversions.

```
burst config <SUBCOMMAND> [OPTIONS] <BACKUP_PATH>
```

Subcommands

- `show`: print the current config
- `get <KEY>`: print a specific key (`source`, `incremental`, `hash_comparison`, `follow_symlinks`, `exclude`, `no_history`)
- `set <KEY> <VALUE>`: update single‑value keys (`source`, `hash_comparison`, `follow_symlinks`)
- `add <KEY> <PATTERN[,PATTERN…]>`: add to multi‑value keys (`exclude`, `no_history`)
- `remove <KEY> [<PATTERN>]`: remove a value or clear the key (`exclude`, `no_history`)
- `convert <MODE>`: switch between `sync` and `incremental`

Options

- `--fix-history`: when used with `add/remove/convert`, delete now‑excluded history or clean up when converting to sync
- `-n, --dry-run`: preview changes
- `-q, --quiet` | `-v, --verbose`

Examples

```
# See current settings
burst config show /backups/home

# Enable following symlinks
burst config set follow_symlinks true /backups/home

# Add excludes and clean up matching history
burst config add exclude ".*\.log,build/" --fix-history /backups/home

# Convert to sync mode and remove historical versions (destructive)
burst config convert sync --fix-history /backups/home
```

### delete

Delete snapshot data to enforce retention policies or reclaim space.

```
burst delete <SNAPSHOT_SELECTION> [OPTIONS] <BACKUP_PATH>
```

Choose what to delete using one or more selectors:

- `-s, --snapshot <SPEC> (*)`: snapshot IDs to delete; accepts single `5`, list `1,3,7`, or range `10-20`
- `-o, --older-than <YYYY-MM-DD> (*)`: delete snapshots older than date
- `-l, --keep-last <COUNT> (*)`: keep only the last N snapshots (deletes the rest)

Scope the deletion with a regex (optional):

- `-p, --pattern <PATTERN>`: POSIX‑style regex to select files/directories (relative to backup root)

Other options

- `-n, --dry-run`: preview without deleting
- `-q, --quiet` | `-v, --verbose`

Note: one of the history selection options (marked with `*` above) is mandatory.

Examples

```
# Delete only PNG files from selected snapshots
burst delete --snapshot 10-20 --pattern ".*\.png$" /backups/home

# Delete everything under any "office/" directory in snapshots 5 and 7
burst delete --snapshot 5,7 --pattern "/?office(/.*)?$" /backups/home
```

### list

Show files from the latest snapshot, explore history for specified files, or show deletions.

```
burst list [OPTIONS] <BACKUP_PATH>
```

- `-s, --snapshot <ID>`: list files from a specific snapshot (defaults to latest)
- `-i, --diff-with`: show differences with the specified snapshot
- `-p, --pattern <PATTERN>`: show the full version history for the specified files
- `-d, --deleted`: show files deleted between the previous and the given/latest snapshot
- `-q, --quiet` | `-v, --verbose`

### history

Show the snapshot timeline with counts and sizes.

```
burst history [OPTIONS] <BACKUP_PATH>
```

- `-n, --limit <NUM>`: limit the number of snapshots shown (default: 10)
- `-q, --quiet` | `-v, --verbose`

### verify

Verify backup integrity.

```
burst verify <WHAT> [OPTIONS] <BACKUP_PATH>
```

What

- `integrity`: cross‑check filesystem vs database, report and optionally fix orphans
- `hash`: recompute SHA‑256 for all files and report mismatches

Options

- `--fix`: try to fix issues where possible (integrity topic)
- `-q, --quiet` | `-v, --verbose`

## ℹ️ Things You Might Want To Do

- Create a new incremental repository
  - `burst init /data/projects /backups/projects`
- Preview what a backup would do
  - `burst backup --dry-run /backups/projects`
- See what changed and what was deleted
  - `burst list --deleted /backups/projects`
  - `burst list --snapshot 42 /backups/projects`
- Restore a single file or an entire folder
  - `burst restore --pattern "^src/main\.rs$" --overwrite /backups/projects /tmp/restore`
  - `burst restore --snapshot 41 --pattern "^src(/.*)?$" /backups/projects /tmp/restore`
- Convert to sync mode (one live version only)
  - `burst config convert sync --fix-history /backups/projects`
- Exclude noisy paths and purge old versions
  - `burst config add exclude "build/,.*\.tmp" --fix-history /backups/projects`
- Keep only the last 7 snapshots or prune by date
  - `burst delete --keep-last 7 /backups/projects`
  - `burst delete --older-than 2024-07-01 /backups/projects`
- Verify the repository
  - `burst verify integrity /backups/projects`
  - `burst verify hash /backups/projects`
- Automate with cron/systemd
  - Run `burst backup /backups/projects` daily; add `-q` for quiet logs.

### Tips

- Use `--dry-run` when changing config, excludes, or retention to validate effects.
- Exclude/no‑history values are regex, comma‑separated. Quote them in shells.
- `--hash-comparison` makes backups safer but slower; disable for speed on trusted media.

## 🔥 Roadmap

### Planned / In Progress

- Compression: optional per‑file/backup compression with stored compressed sizes and ratios
- Encryption: optional at‑rest encryption with key management
- S3‑compatible storage: off‑disk/remote backends using similar metadata semantics

## ✍️ Contributing

Issues and PRs are welcome. Please file reproducible bug reports with command lines and platform details. For larger changes, start a discussion first to agree on scope and approach.

## 🗎 License

MIT. See LICENSE.
