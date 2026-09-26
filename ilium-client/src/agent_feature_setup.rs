//! Managed agent-instruction blocks for Ilium's agent-facing features.
//!
//! A target may already contain wording that looks like an Ilium feature
//! instruction, so status detection intentionally recognises stable phrases
//! rather than relying solely on our markers.  Only marker-delimited blocks
//! are ever removed: an unrelated, user-authored instruction must not be
//! deleted merely because it mentions `ilium progress` or `CHATROOM.md`.
//! Installing always first removes our old block, then writes the current
//! text. Progress instructions carry a schema version so startup
//! reconciliation can distinguish a current managed contract from one that
//! Ilium must refresh. User-authored lookalike text never grants ownership.
//! Every CLI reads the same text: progress monitoring says nothing about
//! `/goal`, so no per-provider variant exists.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use ilium_platform::file_lock::ExclusiveFileLock;
use regex::Regex;

const CHATROOM_INSTRUCTION: &str = "If `CHATROOM.md` exists in the project root, read recent coordination with `ilium chat context --limit 40` when beginning work and before changing shared areas. Use `ilium chat send --message \"...\"` only for a task claim or release, blocker, dependency, material discovery or decision, or a handoff; do not post routine progress narration. Never rewrite `CHATROOM.md` directly.";

const PROGRESS_INSTRUCTION: &str = r#"For every task expected to take at least three minutes inside an Ilium pane, you MUST use the Ilium progress-monitor lifecycle. Start the task first and verify that its process or job is alive. Then construct a cheap absolute-path probe which prints exactly one JSON object containing a stable non-empty `job_id`, a `status` of `not-started-yet`, `running`, `error`, or `done`, a finite `percent` from 0 through 100, and bounded `message`/`error` details. The probe runs from the Ilium server's project root and receives no pane-shell aliases or transient environment, so use absolute paths or an explicit absolute `cd`.

You MUST run `ilium progress check --command '<probe>'` and confirm its JSONL validation result before registration. Then run `ilium progress set --command '<probe>' --interval-seconds <n>` and wait for Ilium's positive JSONL registration acknowledgement containing the monitor ID and accepted first report. After registration, the agent MUST NOT poll in any form: do not make repeated tool calls, run checking loops, sleep then recheck, repeatedly inspect logs or files, issue recurring status commands, or spend conversational turns checking progress. Ilium's detached server is the sole recurring poller, and it will send you a message when the task reaches `done` or `error`. You MAY perform other useful work that does not poll the task.

Do not manually clear a terminal result before handling its notification. Retain the task identity, final process exit evidence, progress evidence, and failure details for verification; use `ilium progress clear --monitor-id <id>` only after the lifecycle is complete or when explicitly cancelling it."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentFeature {
    Chatroom,
    Progress,
}

impl AgentFeature {
    pub const ALL: [Self; 2] = [Self::Chatroom, Self::Progress];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Chatroom => "Chatroom",
            Self::Progress => "Progress",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Chatroom => "ilium-agent-feature: chatroom",
            Self::Progress => "ilium-agent-feature: progress",
        }
    }

    const fn current_version(self) -> Option<u32> {
        match self {
            Self::Chatroom => None,
            Self::Progress => Some(5),
        }
    }

    fn opening_marker(self) -> String {
        match self.current_version() {
            Some(version) => format!("<!-- {} version={version} -->", self.marker()),
            None => format!("<!-- {} -->", self.marker()),
        }
    }

    fn closing_marker(self) -> String {
        format!("<!-- /{} -->", self.marker())
    }

    fn instruction(self) -> &'static str {
        match self {
            Self::Chatroom => CHATROOM_INSTRUCTION,
            Self::Progress => PROGRESS_INSTRUCTION,
        }
    }

    fn detection_regexes(self) -> Vec<Regex> {
        match self {
            Self::Chatroom => vec![
                Regex::new(r"(?i)(ilium chat context|CHATROOM\.md)")
                    .expect("fixed chatroom subject regex"),
                Regex::new(r"(?i)(ilium chat send|shared areas|routine progress)")
                    .expect("fixed chatroom behavior regex"),
            ],
            Self::Progress => vec![
                Regex::new(r"(?i)ilium progress").expect("fixed progress command regex"),
                Regex::new(r"(?i)at least three minutes").expect("fixed progress threshold regex"),
                Regex::new(r"(?i)MUST use").expect("fixed mandatory-use regex"),
                Regex::new(r"(?i)start the task.*verify.*alive")
                    .expect("fixed task-start verification regex"),
                Regex::new(r"(?i)MUST NOT poll").expect("fixed no-poll regex"),
                Regex::new(r"(?i)sole recurring poller").expect("fixed poll-owner regex"),
                Regex::new(r"(?i)ilium progress check").expect("fixed preflight regex"),
                Regex::new(r"(?i)positive JSONL registration acknowledgement")
                    .expect("fixed acknowledgement regex"),
                Regex::new(r"(?i)job_id.*status.*percent").expect("fixed report contract regex"),
                Regex::new(r"(?i)send you a message when the task reaches")
                    .expect("fixed terminal-delivery regex"),
            ],
        }
    }

    fn is_detected(self, contents: &str) -> bool {
        self.detection_regexes()
            .iter()
            .all(|regex| regex.is_match(contents))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSetupStatus {
    NotInstalled,
    Managed,
    ManagedStale,
    ManagedFuture,
    DetectedUnmanaged,
}

impl FeatureSetupStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "Not set up",
            Self::Managed => "Set up by Ilium",
            Self::ManagedStale => "Ilium update required",
            Self::ManagedFuture => "Newer Ilium instructions detected",
            Self::DetectedUnmanaged => "Instruction detected",
        }
    }
}

pub fn global_claude_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".claude/CLAUDE.md"))
}

pub fn status(path: &Path, feature: AgentFeature) -> std::io::Result<FeatureSetupStatus> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FeatureSetupStatus::NotInstalled)
        }
        Err(error) => return Err(error),
    };
    let managed_blocks = validated_managed_blocks(&contents, feature)?;
    if !managed_blocks.is_empty() {
        if let Some(current_version) = feature.current_version() {
            if managed_blocks.iter().any(|block| {
                block
                    .version
                    .is_some_and(|version| version > current_version)
            }) {
                return Ok(FeatureSetupStatus::ManagedFuture);
            }
        }
        if managed_blocks.iter().all(|block| {
            block.version == feature.current_version()
                && feature.is_detected(&contents[block.start..block.end])
        }) {
            return Ok(FeatureSetupStatus::Managed);
        }
        return Ok(FeatureSetupStatus::ManagedStale);
    }
    if feature == AgentFeature::Chatroom && !legacy_chatroom_blocks(&contents)?.is_empty() {
        return Ok(FeatureSetupStatus::Managed);
    }
    Ok(if feature.is_detected(&contents) {
        FeatureSetupStatus::DetectedUnmanaged
    } else {
        FeatureSetupStatus::NotInstalled
    })
}

pub fn install(path: &Path, feature: AgentFeature) -> std::io::Result<()> {
    mutate_target(path, |existing| {
        let without_previous = remove_managed_blocks(existing, feature)?;
        let separator = if without_previous.trim().is_empty() {
            ""
        } else if without_previous.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        Ok(format!(
            "{without_previous}{separator}{}\n{}\n{}\n",
            feature.opening_marker(),
            feature.instruction(),
            feature.closing_marker()
        ))
    })
}

pub fn uninstall(path: &Path, feature: AgentFeature) -> std::io::Result<()> {
    mutate_target(path, |existing| remove_managed_blocks(existing, feature))
}

fn read_existing(path: &Path) -> std::io::Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// Serialises Ilium writers for one target and publishes a complete new file
/// atomically. The lock covers the read as well as the write, so two attached
/// clients enabling different features cannot each overwrite the other's
/// read-modify-write result. A symlinked instruction file stays a symlink: we
/// resolve its target before creating the sibling temporary file.
fn mutate_target(
    path: &Path,
    mutation: impl FnOnce(&str) -> std::io::Result<String>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let target = resolve_target_path(path)?;
    let _lock = ExclusiveFileLock::acquire(&lock_path_for(&target)?)?;
    let existing = read_existing(&target)?;
    let updated = mutation(&existing)?;
    if updated == existing {
        return Ok(());
    }
    write_target_atomically(&target, updated.as_bytes(), existing.as_bytes())
}

fn resolve_target_path(path: &Path) -> std::io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::canonicalize(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(file_name) = path.file_name() else {
                return Ok(path.to_path_buf());
            };
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            Ok(fs::canonicalize(parent)?.join(file_name))
        }
        Err(error) => Err(error),
    }
}

fn lock_path_for(path: &Path) -> std::io::Result<PathBuf> {
    // A fixed FNV-1a digest keeps old and new Ilium processes on the same
    // lock path. `DefaultHasher` deliberately does not promise a stable
    // algorithm across Rust releases. A collision only serialises two
    // unrelated targets, so it cannot corrupt either one.
    let mut path_digest = 0xcbf29ce484222325_u64;
    for byte in path.to_string_lossy().as_bytes() {
        path_digest ^= u64::from(*byte);
        path_digest = path_digest.wrapping_mul(0x100000001b3);
    }
    Ok(ilium_platform::runtime_dir::session_socket_directory()?
        .join(format!("agent-setup-{path_digest:016x}.lock")))
}

fn write_target_atomically(
    path: &Path,
    contents: &[u8],
    expected_existing: &[u8],
) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "instructions".to_string());
    let temporary_path = parent.join(format!(".{file_name}.ilium-tmp-{}", uuid::Uuid::new_v4()));
    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());

    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        if let Some(permissions) = existing_permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        if read_existing(path)?.as_bytes() != expected_existing {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!(
                    "{} changed while Ilium was preparing the update; no replacement was made",
                    path.display()
                ),
            ));
        }
        fs::rename(&temporary_path, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

/// Returns every balanced, non-nested marker block for one feature. A
/// malformed opener is never allowed to pair with a closer appended by a
/// later install: mutation refuses the file until the user repairs the
/// ambiguous structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ManagedBlock {
    start: usize,
    end: usize,
    version: Option<u32>,
}

fn validated_managed_blocks(
    contents: &str,
    feature: AgentFeature,
) -> std::io::Result<Vec<ManagedBlock>> {
    let start_prefix = format!("<!-- {}", feature.marker());
    let end_marker = feature.closing_marker();
    let mut cursor = 0;
    let mut open_block = None;
    let mut blocks = Vec::new();
    while cursor < contents.len() {
        let next_start = contents[cursor..]
            .find(&start_prefix)
            .map(|offset| cursor + offset);
        let next_end = contents[cursor..]
            .find(&end_marker)
            .map(|offset| cursor + offset);
        match (next_start, next_end) {
            (None, None) => break,
            (Some(start), Some(end)) if start < end => {
                let (marker_end, version) = parse_opening_marker(contents, feature, start)?;
                if open_block.replace((start, version)).is_some() {
                    return Err(invalid_data(format!(
                        "nested opening marker for {} at byte {start}",
                        feature.label()
                    )));
                }
                cursor = marker_end;
            }
            (Some(start), None) => {
                let (marker_end, version) = parse_opening_marker(contents, feature, start)?;
                if open_block.replace((start, version)).is_some() {
                    return Err(invalid_data(format!(
                        "nested opening marker for {} at byte {start}",
                        feature.label()
                    )));
                }
                cursor = marker_end;
            }
            (_, Some(end)) => {
                let Some((start, version)) = open_block.take() else {
                    return Err(invalid_data(format!(
                        "closing marker without an opener for {} at byte {end}",
                        feature.label()
                    )));
                };
                let mut after_end = end + end_marker.len();
                if contents[after_end..].starts_with("\r\n") {
                    after_end += 2;
                } else if contents[after_end..].starts_with('\n') {
                    after_end += 1;
                }
                let owned_start = if start > 0
                    && contents.as_bytes()[start - 1] == b'\n'
                    && contents[..start - 1].ends_with('\n')
                {
                    start - 1
                } else {
                    start
                };
                blocks.push(ManagedBlock {
                    start: owned_start,
                    end: after_end,
                    version,
                });
                cursor = after_end;
            }
        }
    }
    if let Some((start, _)) = open_block {
        return Err(invalid_data(format!(
            "opening marker without a closer for {} at byte {start}",
            feature.label()
        )));
    }

    let other = match feature {
        AgentFeature::Chatroom => AgentFeature::Progress,
        AgentFeature::Progress => AgentFeature::Chatroom,
    };
    for block in &blocks {
        let block = &contents[block.start..block.end];
        if block.contains(other.marker()) {
            return Err(invalid_data(format!(
                "overlapping {} and {} managed markers",
                feature.label(),
                other.label()
            )));
        }
    }
    Ok(blocks)
}

fn parse_opening_marker(
    contents: &str,
    feature: AgentFeature,
    start: usize,
) -> std::io::Result<(usize, Option<u32>)> {
    let Some(relative_end) = contents[start..].find("-->") else {
        return Err(invalid_data(format!(
            "unterminated opening marker for {} at byte {start}",
            feature.label()
        )));
    };
    let marker_end = start + relative_end + 3;
    let marker = &contents[start..marker_end];
    let legacy = format!("<!-- {} -->", feature.marker());
    if marker == legacy {
        return Ok((marker_end, None));
    }
    let version_prefix = format!("<!-- {} version=", feature.marker());
    let Some(version_text) = marker
        .strip_prefix(&version_prefix)
        .and_then(|remainder| remainder.strip_suffix(" -->"))
    else {
        return Err(invalid_data(format!(
            "unsupported opening marker for {} at byte {start}",
            feature.label()
        )));
    };
    let version = version_text.parse::<u32>().map_err(|_| {
        invalid_data(format!(
            "invalid managed-block version for {} at byte {start}",
            feature.label()
        ))
    })?;
    Ok((marker_end, Some(version)))
}

/// Locates only the exact one-paragraph shapes emitted by supported legacy
/// releases. A marker in any other structure is ambiguous and blocks mutation
/// instead of authorising deletion up to a guessed boundary.
fn legacy_chatroom_blocks(contents: &str) -> std::io::Result<Vec<(usize, usize)>> {
    const LEGACY_MARKERS: [&str; 2] = [
        "<!-- ilium-chatroom-guidance: v2 -->",
        "<!-- ilium-chatroom-guidance: v1 -->",
    ];
    const LEGACY_V1_BODY: &str = "Post concise coordination, task claims, blockers, and handoffs.";
    let mut blocks = Vec::new();
    for marker in LEGACY_MARKERS {
        let mut search_from = 0;
        while let Some(relative_offset) = contents[search_from..].find(marker) {
            let marker_offset = search_from + relative_offset;
            let heading = "## Ilium Chatroom\n\n";
            let Some(section_start) = contents[..marker_offset].rfind(heading) else {
                return Err(invalid_data(format!(
                    "legacy Chatroom marker at byte {marker_offset} has no adjacent generated heading"
                )));
            };
            if section_start + heading.len() != marker_offset {
                return Err(invalid_data(format!(
                    "legacy Chatroom marker at byte {marker_offset} is outside the supported generated shape"
                )));
            }
            let after_marker = marker_offset + marker.len();
            let body_start = after_marker
                + if contents[after_marker..].starts_with("\r\n\r\n") {
                    4
                } else if contents[after_marker..].starts_with("\n\n") {
                    2
                } else {
                    return Err(invalid_data(format!(
                        "legacy Chatroom marker at byte {marker_offset} has an unsupported separator"
                    )));
                };
            let body_end = contents[body_start..]
                .find('\n')
                .map(|offset| body_start + offset)
                .unwrap_or(contents.len());
            let body = contents[body_start..body_end].trim_end_matches('\r');
            let body_is_recognized = if marker.ends_with("v1 -->") {
                body == LEGACY_V1_BODY
            } else {
                body.starts_with(
                    "This project has a local, gitignored `CHATROOM.md` shared by the user and agents running in ilium.",
                ) && body.ends_with(
                    "Codex users must review and trust the generated project hooks through `/hooks` before those hooks can run.",
                )
            };
            if !body_is_recognized {
                return Err(invalid_data(format!(
                    "legacy Chatroom marker at byte {marker_offset} has unrecognized body text"
                )));
            }
            let mut section_end = body_end;
            if contents[section_end..].starts_with("\r\n") {
                section_end += 2;
            } else if contents[section_end..].starts_with('\n') {
                section_end += 1;
            }
            blocks.push((section_start, section_end));
            search_from = section_end;
        }
    }
    blocks.sort_unstable();
    Ok(blocks)
}

fn remove_managed_blocks(contents: &str, feature: AgentFeature) -> std::io::Result<String> {
    let mut blocks: Vec<(usize, usize)> = validated_managed_blocks(contents, feature)?
        .into_iter()
        .map(|block| (block.start, block.end))
        .collect();
    if feature == AgentFeature::Chatroom {
        blocks.extend(legacy_chatroom_blocks(contents)?);
    }
    blocks.sort_unstable();
    for pair in blocks.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(invalid_data(format!(
                "overlapping managed {} blocks",
                feature.label()
            )));
        }
    }
    let mut updated = String::with_capacity(contents.len());
    let mut cursor = 0;
    for (start, end) in blocks {
        updated.push_str(&contents[cursor..start]);
        cursor = end;
    }
    updated.push_str(&contents[cursor..]);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_reports_managed_and_replaces_old_copy() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(&target, "# Existing\n").unwrap();
        install(&target, AgentFeature::Progress).unwrap();
        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::Managed
        );
        install(&target, AgentFeature::Progress).unwrap();
        let contents = fs::read_to_string(&target).unwrap();
        assert_eq!(contents.matches("ilium-agent-feature: progress").count(), 2);
        assert!(contents.contains("# Existing"));
    }

    #[test]
    fn install_repairs_duplicate_managed_blocks_to_one_current_copy() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        let block = format!(
            "<!-- {} -->\nold\n<!-- /{} -->\n",
            AgentFeature::Progress.marker(),
            AgentFeature::Progress.marker()
        );
        fs::write(&target, format!("{block}\n{block}")).unwrap();

        install(&target, AgentFeature::Progress).unwrap();

        let contents = fs::read_to_string(&target).unwrap();
        assert_eq!(contents.matches("ilium-agent-feature: progress").count(), 2);
        assert!(!contents.contains("\nold\n"));
    }

    #[test]
    fn stale_managed_progress_copy_is_detected_and_replaced_in_place() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("AGENTS.md");
        fs::write(
            &target,
            "<!-- ilium-agent-feature: progress -->\nUse ilium progress as a progress monitor and clear it when finished.\n<!-- /ilium-agent-feature: progress -->\n",
        )
        .unwrap();
        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::ManagedStale
        );

        install(&target, AgentFeature::Progress).unwrap();

        let contents = fs::read_to_string(&target).unwrap();
        assert!(!contents.contains("as a progress monitor"));
        assert!(contents.contains("ilium-agent-feature: progress version=5"));
        assert!(contents.contains("at least three minutes"));
        assert!(contents.contains("MUST NOT poll"));
        assert!(contents.contains("sole recurring poller"));
        assert!(contents.contains("ilium progress check --command"));
        assert!(contents.contains("positive JSONL registration acknowledgement"));
        assert!(contents.contains("`job_id`"));
        assert!(contents.contains("ilium progress set --command"));
        assert!(contents.contains("ilium progress clear"));
        assert!(contents.contains("send you a message when the task reaches `done` or `error`"));
        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::Managed
        );
    }

    #[test]
    fn progress_instruction_never_mentions_goals_pausing_or_resuming() {
        let instruction = AgentFeature::Progress.instruction().to_lowercase();
        for forbidden in [
            "/goal",
            "goal",
            "pause",
            "resume",
            "stop hook",
            "arm-goal-resume",
            "end the turn",
        ] {
            assert!(
                !instruction.contains(forbidden),
                "progress instruction must not mention {forbidden:?}"
            );
        }
        assert!(instruction.contains("must not poll"));
    }

    #[test]
    fn every_provider_target_gets_the_same_progress_text() {
        let directory = tempfile::tempdir().unwrap();
        let claude_target = directory.path().join("CLAUDE.md");
        let codex_target = directory.path().join("AGENTS.md");
        install(&claude_target, AgentFeature::Progress).unwrap();
        install(&codex_target, AgentFeature::Progress).unwrap();
        assert_eq!(
            fs::read_to_string(&claude_target).unwrap(),
            fs::read_to_string(&codex_target).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_agents_file_symlinked_to_claude_stays_one_stable_shared_file() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let claude_file = directory.path().join("CLAUDE.md");
        let codex_link = directory.path().join("AGENTS.md");
        fs::write(&claude_file, "Keep this.\n").unwrap();
        symlink(&claude_file, &codex_link).unwrap();

        install(&claude_file, AgentFeature::Progress).unwrap();
        let contents = fs::read_to_string(&claude_file).unwrap();
        assert!(contents.starts_with("Keep this.\n"));
        // Both views of the one file accept it, so neither rewrites it.
        for path in [&claude_file, &codex_link] {
            assert_eq!(
                status(path, AgentFeature::Progress).unwrap(),
                FeatureSetupStatus::Managed
            );
        }
        install(&codex_link, AgentFeature::Progress).unwrap();
        assert_eq!(fs::read_to_string(&claude_file).unwrap(), contents);
        assert!(codex_link.is_symlink());
    }

    #[test]
    fn uninstall_preserves_unmanaged_text() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(&target, "Keep this.\n").unwrap();
        install(&target, AgentFeature::Chatroom).unwrap();
        uninstall(&target, AgentFeature::Chatroom).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "Keep this.\n");
        assert_eq!(
            status(&target, AgentFeature::Chatroom).unwrap(),
            FeatureSetupStatus::NotInstalled
        );
    }

    #[test]
    fn obsolete_unmanaged_instruction_does_not_satisfy_the_current_contract() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(
            &target,
            "Use ilium progress with an absolute-path command and clear it when finished.\n",
        )
        .unwrap();
        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::NotInstalled
        );
        uninstall(&target, AgentFeature::Progress).unwrap();
        assert!(fs::read_to_string(&target)
            .unwrap()
            .contains("Use ilium progress"));
    }

    #[test]
    fn detects_complete_current_unmanaged_instruction_without_claiming_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("AGENTS.md");
        fs::write(
            &target,
            concat!(
                "For every task taking at least three minutes, you MUST use ilium progress. ",
                "Start the task and verify its process is alive. ",
                "Run ilium progress check, then wait for a positive JSONL registration acknowledgement. ",
                "The report has job_id, status, and percent. The agent MUST NOT poll; ",
                "Ilium is the sole recurring poller and will send you a message when the task ",
                "reaches done or error.\n",
            ),
        )
        .unwrap();

        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::DetectedUnmanaged
        );
        uninstall(&target, AgentFeature::Progress).unwrap();
        assert!(fs::read_to_string(&target)
            .unwrap()
            .contains("The agent MUST NOT poll"));
    }

    #[test]
    fn detection_is_order_independent_but_requires_two_feature_specific_signals() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(
            &target,
            "Avoid routine progress narration. Send with `ilium chat send`; consult CHATROOM.md first.\n",
        )
        .unwrap();
        assert_eq!(
            status(&target, AgentFeature::Chatroom).unwrap(),
            FeatureSetupStatus::DetectedUnmanaged
        );

        fs::write(&target, "The project happens to contain CHATROOM.md.\n").unwrap();
        assert_eq!(
            status(&target, AgentFeature::Chatroom).unwrap(),
            FeatureSetupStatus::NotInstalled
        );
    }

    #[test]
    fn legacy_ilium_chatroom_block_is_managed_and_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(
            &target,
            "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n\n## Keep\n\nUser text.\n",
        )
        .unwrap();

        assert_eq!(
            status(&target, AgentFeature::Chatroom).unwrap(),
            FeatureSetupStatus::Managed
        );
        install(&target, AgentFeature::Chatroom).unwrap();
        let contents = fs::read_to_string(&target).unwrap();
        assert!(!contents.contains("ilium-chatroom-guidance"));
        assert!(contents.contains("ilium-agent-feature: chatroom"));
        assert!(contents.contains("## Keep\n\nUser text."));
    }

    #[test]
    fn replacing_legacy_chatroom_block_preserves_unheaded_user_text_after_it() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(
            &target,
            "# Existing\n\n## Ilium Chatroom\n\n<!-- ilium-chatroom-guidance: v1 -->\n\nPost concise coordination, task claims, blockers, and handoffs.\n\nKeep this unheaded user note.\n",
        )
        .unwrap();

        install(&target, AgentFeature::Chatroom).unwrap();
        let contents = fs::read_to_string(&target).unwrap();
        assert!(!contents.contains("Post concise coordination"));
        assert!(contents.contains("Keep this unheaded user note."));
    }

    #[test]
    fn malformed_managed_marker_is_refused_without_changing_user_text() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        let original = "<!-- ilium-agent-feature: progress -->\nOld guidance.\n\n# Personal notes\nKeep this text.\n";
        fs::write(&target, original).unwrap();

        assert!(status(&target, AgentFeature::Progress).is_err());
        assert!(install(&target, AgentFeature::Progress).is_err());
        assert!(uninstall(&target, AgentFeature::Progress).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
    }

    #[test]
    fn empty_managed_block_is_stale_and_repairable() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(
            &target,
            "<!-- ilium-agent-feature: progress -->\n<!-- /ilium-agent-feature: progress -->\n",
        )
        .unwrap();

        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::ManagedStale
        );
        install(&target, AgentFeature::Progress).unwrap();
        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::Managed
        );
    }

    #[test]
    fn future_managed_version_is_preserved_until_explicit_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("AGENTS.md");
        let original = concat!(
            "# User policy\n\n",
            "<!-- ilium-agent-feature: progress version=99 -->\n",
            "Future Ilium-owned wording.\n",
            "<!-- /ilium-agent-feature: progress -->\n",
        );
        fs::write(&target, original).unwrap();

        assert_eq!(
            status(&target, AgentFeature::Progress).unwrap(),
            FeatureSetupStatus::ManagedFuture
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), original);

        install(&target, AgentFeature::Progress).unwrap();
        let updated = fs::read_to_string(&target).unwrap();
        assert!(updated.contains("# User policy"));
        assert!(updated.contains("version=5"));
        assert!(!updated.contains("version=99"));
    }

    #[test]
    fn unsupported_version_syntax_is_refused_without_changing_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("AGENTS.md");
        let original = concat!(
            "Keep this.\n",
            "<!-- ilium-agent-feature: progress version=next -->\n",
            "Owned-looking but ambiguous.\n",
            "<!-- /ilium-agent-feature: progress -->\n",
        );
        fs::write(&target, original).unwrap();

        assert!(status(&target, AgentFeature::Progress).is_err());
        assert!(install(&target, AgentFeature::Progress).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
    }

    #[test]
    fn atomic_publish_refuses_a_concurrent_content_change() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("AGENTS.md");
        fs::write(&target, "new user contents\n").unwrap();

        let error = write_target_atomically(&target, b"ilium replacement\n", b"old contents\n")
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(fs::read_to_string(&target).unwrap(), "new user contents\n");
        assert_eq!(
            fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains("ilium-tmp"))
                .count(),
            0
        );
    }

    #[test]
    fn overlapping_feature_markers_are_refused_without_changing_user_text() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        let original = concat!(
            "<!-- ilium-agent-feature: chatroom -->\n",
            "Read CHATROOM.md and use `ilium chat send`.\n",
            "<!-- ilium-agent-feature: progress -->\n",
            "Use `ilium progress set` and `ilium progress clear`.\n",
            "<!-- /ilium-agent-feature: chatroom -->\n",
            "<!-- /ilium-agent-feature: progress -->\n",
            "Keep this user text.\n",
        );
        fs::write(&target, original).unwrap();

        assert!(status(&target, AgentFeature::Chatroom).is_err());
        assert!(install(&target, AgentFeature::Chatroom).is_err());
        assert!(uninstall(&target, AgentFeature::Progress).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn installing_through_a_symlink_preserves_the_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("actual.md");
        let link = directory.path().join("CLAUDE.md");
        fs::write(&target, "Keep this.\n").unwrap();
        symlink(&target, &link).unwrap();

        install(&link, AgentFeature::Progress).unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(fs::read_to_string(&target)
            .unwrap()
            .contains("ilium-agent-feature: progress"));
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replacement_preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("CLAUDE.md");
        fs::write(&target, "Keep this.\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();

        install(&target, AgentFeature::Progress).unwrap();

        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
