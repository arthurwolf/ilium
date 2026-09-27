//! Pure parsers for Git's stable machine-oriented output formats.

use crate::{GitError, GitStatus, GitVersion, Worktree};
use std::path::PathBuf;

pub fn git_version(output: &[u8]) -> Result<GitVersion, GitError> {
    let text = utf8(output)?.trim();
    let version = text
        .strip_prefix("git version ")
        .ok_or_else(|| GitError::Parse("missing git version prefix".into()))?;
    let mut parts = version.split('.');
    let major = parse_number(parts.next(), "git major version")?;
    let minor = parse_number(parts.next(), "git minor version")?;
    let patch = parts
        .next()
        .and_then(|part| {
            part.split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .filter(|part| !part.is_empty())
        .unwrap_or("0")
        .parse()
        .map_err(|_| GitError::Parse("invalid git patch version".into()))?;
    Ok(GitVersion {
        major,
        minor,
        patch,
    })
}

pub fn worktrees(output: &[u8]) -> Result<Vec<Worktree>, GitError> {
    if !output.is_empty() && !output.ends_with(&[0]) {
        return Err(GitError::Parse("unterminated worktree record".into()));
    }
    let mut result = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in output.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(worktree) = current.take() {
                result.push(worktree);
            }
            continue;
        }
        let field = utf8(field)?;
        if let Some(path) = field.strip_prefix("worktree ") {
            if let Some(worktree) = current.take() {
                result.push(worktree);
            }
            if path.is_empty() {
                return Err(GitError::Parse("empty worktree path".into()));
            }
            current = Some(Worktree {
                path: PathBuf::from(path),
                head: None,
                branch: None,
                is_bare: false,
                is_detached: false,
                is_locked: false,
                is_prunable: false,
            });
            continue;
        }
        let worktree = current
            .as_mut()
            .ok_or_else(|| GitError::Parse("worktree metadata before path".into()))?;
        if let Some(head) = field.strip_prefix("HEAD ") {
            worktree.head = Some(head.to_owned());
        } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
            worktree.branch = Some(branch.to_owned());
        } else if field == "bare" {
            worktree.is_bare = true;
        } else if field == "detached" {
            worktree.is_detached = true;
        } else if field == "locked" || field.starts_with("locked ") {
            worktree.is_locked = true;
        } else if field == "prunable" || field.starts_with("prunable ") {
            worktree.is_prunable = true;
        } else {
            return Err(GitError::Parse(format!("unknown worktree field: {field}")));
        }
    }
    if let Some(worktree) = current {
        result.push(worktree);
    }
    Ok(result)
}

/// Git 2.17–2.35 offers only newline-delimited porcelain output. Entries
/// with an embedded newline cannot be represented unambiguously by it; the
/// caller validates every parsed path against the repository before use.
pub fn worktrees_lines(output: &[u8]) -> Result<Vec<Worktree>, GitError> {
    if !output.is_empty() && !output.ends_with(b"\n") {
        return Err(GitError::Parse("unterminated worktree line".into()));
    }
    let mut normalized = Vec::with_capacity(output.len());
    for line in output.split(|byte| *byte == b'\n') {
        normalized.extend_from_slice(line);
        normalized.push(0);
    }
    worktrees(&normalized)
}

pub fn branches(output: &[u8]) -> Result<Vec<String>, GitError> {
    let text = utf8(output)?;
    Ok(text
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

pub fn status(output: &[u8]) -> Result<GitStatus, GitError> {
    if !output.is_empty() && !output.ends_with(&[0]) {
        return Err(GitError::Parse("unterminated status record".into()));
    }
    let mut status = GitStatus::default();
    let mut records = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(record) = records.next() {
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            let value = utf8(value)?;
            if value == "(detached)" {
                status.detached = true;
            } else if value != "(unknown)" {
                status.branch = Some(value.to_owned());
            }
        } else if let Some(value) = record.strip_prefix(b"# branch.upstream ") {
            status.upstream = Some(utf8(value)?.to_owned());
        } else if let Some(value) = record.strip_prefix(b"# branch.ab ") {
            let value = utf8(value)?;
            let mut fields = value.split_whitespace();
            status.ahead = parse_signed_count(fields.next(), '+', "ahead")?;
            status.behind = parse_signed_count(fields.next(), '-', "behind")?;
        } else if record.starts_with(b"# ") {
            // branch.oid and future additive branch headers do not change the
            // counts; callers may still use status on an unborn repository.
        } else if record.starts_with(b"1 ") || record.starts_with(b"2 ") {
            count_tracked(&mut status, record)?;
            if record.starts_with(b"2 ") {
                // -z emits the original path as a separate NUL field for
                // rename/copy records. Never count that path as a record.
                records
                    .next()
                    .ok_or_else(|| GitError::Parse("rename missing original path".into()))?;
            }
        } else if record.starts_with(b"u ") {
            status.conflicted += 1;
        } else if record.starts_with(b"? ") {
            status.untracked += 1;
        } else if record.starts_with(b"! ") {
            // ignored files are not dirty
        } else {
            return Err(GitError::Parse(format!(
                "unknown status record: {}",
                String::from_utf8_lossy(record)
            )));
        }
    }
    Ok(status)
}

fn count_tracked(status: &mut GitStatus, record: &[u8]) -> Result<(), GitError> {
    let state = record
        .split(|byte| *byte == b' ')
        .nth(1)
        .ok_or_else(|| GitError::Parse("tracked status missing XY".into()))?;
    if state.len() != 2 {
        return Err(GitError::Parse("long tracked XY".into()));
    }
    if state[0] != b'.' {
        status.staged += 1;
    }
    if state[1] != b'.' {
        status.modified += 1;
    }
    Ok(())
}

fn parse_signed_count(value: Option<&str>, sign: char, name: &str) -> Result<u32, GitError> {
    value
        .and_then(|text| text.strip_prefix(sign))
        .ok_or_else(|| GitError::Parse(format!("missing {name} count")))?
        .parse()
        .map_err(|_| GitError::Parse(format!("invalid {name} count")))
}

fn parse_number(value: Option<&str>, name: &str) -> Result<u32, GitError> {
    value
        .ok_or_else(|| GitError::Parse(format!("missing {name}")))?
        .parse()
        .map_err(|_| GitError::Parse(format!("invalid {name}")))
}

pub(crate) fn utf8(bytes: &[u8]) -> Result<&str, GitError> {
    std::str::from_utf8(bytes).map_err(|_| GitError::Parse("git output is not UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_worktree_porcelain_with_detached_and_locked_entries() {
        let output = b"worktree /tmp/main\0HEAD abc\0branch refs/heads/main\0\0worktree /tmp/linked\0HEAD def\0detached\0locked reason\0\0";
        let parsed = worktrees(output).expect("parse worktrees");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        assert!(parsed[1].is_detached);
        assert!(parsed[1].is_locked);
    }

    #[test]
    fn parses_status_with_rename_extra_field_and_counts() {
        let output = b"# branch.oid abc\0# branch.head feature/x\0# branch.upstream origin/x\0# branch.ab +3 -2\0\
                       1 .M N... 100644 100644 100644 a b file\0";
        let mut extended = output.to_vec();
        extended.extend_from_slice(b"2 R. N... 100644 100644 100644 a b R100 new\0old\0u UU N... 100644 100644 100644 100644 a b c conflict\0? new file\0");
        let parsed = status(&extended).expect("parse status");
        assert_eq!(parsed.branch.as_deref(), Some("feature/x"));
        assert_eq!((parsed.ahead, parsed.behind), (3, 2));
        assert_eq!(
            (
                parsed.staged,
                parsed.modified,
                parsed.untracked,
                parsed.conflicted
            ),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn malformed_records_are_rejected() {
        assert!(worktrees(b"branch refs/heads/main\0").is_err());
        assert!(status(b"2 R. metadata new\0").is_err());
        assert!(status(b"? untracked").is_err());
        assert!(
            worktrees_lines(b"worktree /tmp/a\nunexpected newline segment\nHEAD abc\n\n").is_err()
        );
        assert_eq!(
            worktrees_lines(b"worktree /tmp/a\nHEAD abc\nbranch refs/heads/main\n\n")
                .expect("old porcelain")[0]
                .branch
                .as_deref(),
            Some("main")
        );
    }

    #[test]
    fn status_counts_non_utf8_filenames_without_decoding_paths() {
        let parsed = status(b"# branch.head main\0? file-\xff\0").expect("count raw filename");
        assert_eq!(parsed.untracked, 1);
    }

    #[test]
    fn git_version_accepts_distribution_suffix() {
        assert_eq!(
            git_version(b"git version 2.39.5.windows.1\n").expect("version"),
            GitVersion {
                major: 2,
                minor: 39,
                patch: 5
            }
        );
    }
}
