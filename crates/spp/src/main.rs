use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration as StdDuration, Instant, SystemTime};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use similar::TextDiff;
use walkdir::WalkDir;

const RUNTIME_CONFIG: &str = ".codex-spp/config.toml";
const STATE_FILE: &str = ".codex-spp/state.json";
const SESSION_DIR: &str = ".codex-spp/sessions";
const WEEKLY_DIR: &str = ".codex-spp/weekly";
const TRANSCRIPT_DIR: &str = ".codex-spp/transcripts";
const RUNTIME_DIR: &str = ".codex-spp/runtime";
const TEMPLATE_CONFIG: &str = "template_spp.config.toml";
const PROJECT_RUNTIME_CONFIG_FILE: &str = ".codex-spp/config.toml";
const PROJECT_CODEX_CONFIG_FILE: &str = ".codex/config.toml";
const GITIGNORE_RULE_CODEX_SPP: &str = "/.codex-spp/";
const DEFAULT_HISTORY_PATH: &str = "auto";
const DEFAULT_CHAT_SOURCE: &str = "history_jsonl";
const DEFAULT_TRANSCRIPT_EVENT_MAX_BYTES: u64 = 64_000;
const DEFAULT_POLL_INTERVAL_MS: u64 = 2000;
const MAX_RECORDER_ERRORS: usize = 50;

static EVENT_COUNTER: AtomicU64 = AtomicU64::new(1);

const PROJECT_ASSETS: &[(&str, &str)] = &[
    ("AGENTS.md", include_str!("../../../AGENTS.md")),
    (
        ".agents/policy.md",
        include_str!("../../../.agents/policy.md"),
    ),
    (
        ".agents/attribution.md",
        include_str!("../../../.agents/attribution.md"),
    ),
    (
        ".agents/modes/drive.md",
        include_str!("../../../.agents/modes/drive.md"),
    ),
    (
        ".agents/modes/normal.md",
        include_str!("../../../.agents/modes/normal.md"),
    ),
    (
        ".agents/schemas/template_spp.session.schema.json",
        include_str!("../../../.agents/schemas/template_spp.session.schema.json"),
    ),
    (
        ".agents/schemas/template_spp.weekly_report.schema.json",
        include_str!("../../../.agents/schemas/template_spp.weekly_report.schema.json"),
    ),
    (
        ".agents/schemas/template_spp.transcript_event.schema.json",
        include_str!("../../../.agents/schemas/template_spp.transcript_event.schema.json"),
    ),
    (
        ".agents/skills/spp-drive/SKILL.md",
        include_str!("../../../.agents/skills/spp-drive/SKILL.md"),
    ),
    (
        ".agents/skills/spp-coach/SKILL.md",
        include_str!("../../../.agents/skills/spp-coach/SKILL.md"),
    ),
    (
        ".agents/skills/spp-stats/SKILL.md",
        include_str!("../../../.agents/skills/spp-stats/SKILL.md"),
    ),
    (
        "skills/spp-drive/SKILL.md",
        include_str!("../../../.agents/skills/spp-drive/SKILL.md"),
    ),
    (
        "skills/spp-coach/SKILL.md",
        include_str!("../../../.agents/skills/spp-coach/SKILL.md"),
    ),
    (
        "skills/spp-stats/SKILL.md",
        include_str!("../../../.agents/skills/spp-stats/SKILL.md"),
    ),
];

const PROJECT_RUNTIME_CONFIG_ASSET: &str = include_str!("../../../template_spp.config.toml");
const PROJECT_CODEX_CONFIG_ASSET: &str = include_str!("../../../template_spp.codex.config.toml");

#[derive(Parser, Debug)]
#[command(name = "spp", version, about = "codex-spp wrapper CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init,
    Status,
    Drive(DriveArgs),
    Pause(PauseArgs),
    Resume,
    Reset,
    Codex(CodexArgs),
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    Attrib {
        #[command(subcommand)]
        command: AttribCommands,
    },
}

#[derive(Args, Debug)]
struct PauseArgs {
    #[arg(long, default_value_t = 24)]
    hours: u8,
}

#[derive(Args, Debug)]
struct CodexArgs {
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    extra: Vec<String>,
}

#[derive(Args, Debug, Default)]
struct DriveArgs {
    #[command(subcommand)]
    command: Option<DriveSubcommand>,
}

#[derive(Subcommand, Debug)]
enum DriveSubcommand {
    Start,
    Stop,
    Status,
    #[command(hide = true)]
    Record(DriveRecordArgs),
}

#[derive(Args, Debug, Clone)]
struct DriveRecordArgs {
    #[arg(long)]
    session_id: String,
    #[arg(long)]
    log_schema_version: String,
    #[arg(long)]
    transcript_path: PathBuf,
    #[arg(long)]
    history_path: PathBuf,
    #[arg(long)]
    history_offset: u64,
    #[arg(long)]
    control_path: PathBuf,
    #[arg(long)]
    done_path: PathBuf,
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    include_file_diff: bool,
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    capture_full_text: bool,
    #[arg(long, default_value_t = DEFAULT_TRANSCRIPT_EVENT_MAX_BYTES)]
    max_event_bytes: u64,
    #[arg(long, default_value_t = DEFAULT_POLL_INTERVAL_MS)]
    poll_interval_ms: u64,
    #[arg(long)]
    exclude: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum ProjectCommands {
    Init(ProjectInitArgs),
}

#[derive(Args, Debug)]
struct ProjectInitArgs {
    #[arg(value_name = "PROJECT", default_value = ".")]
    project: PathBuf,
    #[arg(long, default_value_t = false)]
    force: bool,
    #[arg(long, default_value_t = false)]
    with_codex_config: bool,
}

#[derive(Subcommand, Debug)]
enum AttribCommands {
    Fix(AttribFixArgs),
}

#[derive(Args, Debug)]
struct AttribFixArgs {
    commit: String,
    #[arg(long)]
    actor: Actor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    Normal,
    Drive,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    ValueEnum,
    Default,
    Hash,
    PartialOrd,
    Ord,
)]
#[serde(rename_all = "lowercase")]
enum Actor {
    #[default]
    Human,
    Ai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    log_schema_version: String,
    weekly_ratio_target: f64,
    max_log_bytes: u64,
    diff_snapshot_enabled: bool,
    codex: CodexConfig,
    transcript: TranscriptConfig,
    attribution: AttributionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct CodexConfig {
    #[serde(default = "default_codex_mode_normal")]
    normal: CodexModeConfig,
    #[serde(default = "default_codex_mode_drive")]
    drive: CodexModeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct CodexModeConfig {
    sandbox: String,
    approval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct TranscriptConfig {
    chat_source: String,
    history_path: String,
    capture_full_text: bool,
    max_event_bytes: u64,
    include_file_diff: bool,
    watch_exclude: Vec<String>,
    poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AttributionConfig {
    codex_author_emails: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            log_schema_version: "1.1".to_string(),
            weekly_ratio_target: 0.70,
            max_log_bytes: 524_288_000,
            diff_snapshot_enabled: false,
            codex: CodexConfig::default(),
            transcript: TranscriptConfig::default(),
            attribution: AttributionConfig::default(),
        }
    }
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            normal: default_codex_mode_normal(),
            drive: default_codex_mode_drive(),
        }
    }
}

impl Default for CodexModeConfig {
    fn default() -> Self {
        Self {
            sandbox: "read-only".to_string(),
            approval: "on-request".to_string(),
        }
    }
}

impl Default for TranscriptConfig {
    fn default() -> Self {
        Self {
            chat_source: DEFAULT_CHAT_SOURCE.to_string(),
            history_path: DEFAULT_HISTORY_PATH.to_string(),
            capture_full_text: true,
            max_event_bytes: DEFAULT_TRANSCRIPT_EVENT_MAX_BYTES,
            include_file_diff: true,
            watch_exclude: vec![
                ".git/".to_string(),
                ".codex-spp/".to_string(),
                "target/".to_string(),
            ],
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
        }
    }
}

fn default_codex_mode_normal() -> CodexModeConfig {
    CodexModeConfig {
        sandbox: "workspace-write".to_string(),
        approval: "on-request".to_string(),
    }
}

fn default_codex_mode_drive() -> CodexModeConfig {
    CodexModeConfig {
        sandbox: "read-only".to_string(),
        approval: "on-request".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct State {
    mode: Mode,
    drive_reason: Option<String>,
    pause_until: Option<DateTime<Utc>>,
    attribution_overrides: HashMap<String, Actor>,
    active_drive_session: Option<ActiveDriveSession>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveDriveSession {
    session_id: String,
    started_at: DateTime<Utc>,
    history_path: String,
    history_offset: u64,
    transcript_path: String,
    control_path: String,
    done_path: String,
    recorder_pid: Option<u32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            drive_reason: None,
            pause_until: None,
            attribution_overrides: HashMap::new(),
            active_drive_session: None,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TranscriptEvent {
    log_schema_version: String,
    event_id: String,
    session_id: String,
    event_type: String,
    timestamp: DateTime<Utc>,
    mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecorderDone {
    session_id: String,
    finished_at: DateTime<Utc>,
    history_offset: u64,
    chat_events: u64,
    diff_events: u64,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    role: String,
    content: String,
    message_id: Option<String>,
    raw: Option<Value>,
}

#[derive(Debug, Clone)]
struct FileState {
    len: u64,
    modified: Option<SystemTime>,
    content: Arc<str>,
}

impl Default for RecorderDone {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            finished_at: Utc::now(),
            history_offset: 0,
            chat_events: 0,
            diff_events: 0,
            errors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WeeklyReport {
    log_schema_version: String,
    generated_at: DateTime<Utc>,
    year: i32,
    iso_week: u32,
    human_lines_added: u64,
    ai_lines_added: u64,
    human_commit_count: u64,
    ai_commit_count: u64,
    ratio: f64,
    target_ratio: f64,
    gate_passed: bool,
    mode_after_evaluation: Mode,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionLogEntry {
    log_schema_version: String,
    timestamp: DateTime<Utc>,
    command: String,
    mode: Mode,
    sandbox: String,
    approval: String,
    git_branch: String,
    git_commit: Option<String>,
    gate_ratio: Option<f64>,
    gate_target: Option<f64>,
    notes: Option<String>,
}

#[derive(Debug, Default)]
struct WeeklyMetrics {
    human_lines_added: u64,
    ai_lines_added: u64,
    human_commit_count: u64,
    ai_commit_count: u64,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteOutcome {
    Created,
    Overwritten,
    Skipped,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Project { command } => match command {
            ProjectCommands::Init(args) => cmd_project_init(args),
        },
        other => {
            let repo_root = detect_repo_root()?;
            match other {
                Commands::Init => cmd_init(&repo_root),
                Commands::Status => cmd_status(&repo_root),
                Commands::Drive(args) => cmd_drive(&repo_root, args),
                Commands::Pause(args) => cmd_pause(&repo_root, args),
                Commands::Resume => cmd_resume(&repo_root),
                Commands::Reset => cmd_reset(&repo_root),
                Commands::Codex(args) => cmd_codex(&repo_root, args),
                Commands::Attrib { command } => match command {
                    AttribCommands::Fix(args) => cmd_attrib_fix(&repo_root, args),
                },
                Commands::Project { .. } => unreachable!("project command handled above"),
            }
        }
    }
}

fn cmd_project_init(args: ProjectInitArgs) -> Result<()> {
    let project_root = if args.project.is_absolute() {
        args.project
    } else {
        std::env::current_dir()
            .with_context(|| "failed to resolve current directory")?
            .join(args.project)
    };
    fs::create_dir_all(&project_root)
        .with_context(|| format!("failed to create project dir {}", project_root.display()))?;

    let mut created = 0_u64;
    let mut overwritten = 0_u64;
    let mut skipped = 0_u64;

    for (rel, content) in PROJECT_ASSETS {
        let outcome = write_text_asset(&project_root.join(rel), content, args.force)?;
        match outcome {
            WriteOutcome::Created => created += 1,
            WriteOutcome::Overwritten => overwritten += 1,
            WriteOutcome::Skipped => skipped += 1,
        }
    }

    let runtime_cfg_outcome = write_text_asset(
        &project_root.join(PROJECT_RUNTIME_CONFIG_FILE),
        PROJECT_RUNTIME_CONFIG_ASSET,
        args.force,
    )?;
    match runtime_cfg_outcome {
        WriteOutcome::Created => created += 1,
        WriteOutcome::Overwritten => overwritten += 1,
        WriteOutcome::Skipped => skipped += 1,
    }

    if args.with_codex_config {
        let codex_cfg_outcome = write_text_asset(
            &project_root.join(PROJECT_CODEX_CONFIG_FILE),
            PROJECT_CODEX_CONFIG_ASSET,
            args.force,
        )?;
        match codex_cfg_outcome {
            WriteOutcome::Created => created += 1,
            WriteOutcome::Overwritten => overwritten += 1,
            WriteOutcome::Skipped => skipped += 1,
        }
    }

    let gitignore_path = project_root.join(".gitignore");
    let gitignore_updated = ensure_gitignore_rule(&gitignore_path, GITIGNORE_RULE_CODEX_SPP)?;

    println!("project scaffold complete: {}", project_root.display());
    println!(
        "assets -> created: {}, overwritten: {}, skipped: {}",
        created, overwritten, skipped
    );
    if gitignore_updated {
        println!("updated: {}", gitignore_path.display());
    } else {
        println!("gitignore already contains {}", GITIGNORE_RULE_CODEX_SPP);
    }
    if skipped > 0 && !args.force {
        println!("note: existing files were skipped (use --force to overwrite)");
    }

    Ok(())
}

fn cmd_init(repo_root: &Path) -> Result<()> {
    fs::create_dir_all(repo_root.join(SESSION_DIR))
        .with_context(|| "failed to create session log directory")?;
    fs::create_dir_all(repo_root.join(WEEKLY_DIR))
        .with_context(|| "failed to create weekly report directory")?;
    fs::create_dir_all(repo_root.join(TRANSCRIPT_DIR))
        .with_context(|| "failed to create transcript directory")?;
    fs::create_dir_all(repo_root.join(RUNTIME_DIR))
        .with_context(|| "failed to create runtime directory")?;

    let runtime_cfg_path = repo_root.join(RUNTIME_CONFIG);
    if !runtime_cfg_path.exists() {
        if let Some(parent) = runtime_cfg_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let template_cfg = repo_root.join(TEMPLATE_CONFIG);
        if template_cfg.exists() {
            fs::copy(&template_cfg, &runtime_cfg_path).with_context(|| {
                format!(
                    "failed to copy template config from {}",
                    template_cfg.display()
                )
            })?;
        } else {
            let cfg_text = toml::to_string_pretty(&AppConfig::default())?;
            fs::write(&runtime_cfg_path, cfg_text)
                .with_context(|| "failed to write default runtime config")?;
        }
    }

    let state_path = repo_root.join(STATE_FILE);
    if !state_path.exists() {
        save_state(repo_root, &State::default())?;
    }

    println!("initialized codex-spp runtime");
    Ok(())
}

fn cmd_status(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let config = load_config(repo_root)?;
    let mut state = load_state(repo_root)?;
    refresh_pause(&mut state);

    let mut report = compute_weekly_report(repo_root, &config, &state)?;
    let pause = pause_active(&state);
    apply_gate(&mut state, &mut report, pause);

    save_state(repo_root, &state)?;
    write_weekly_report(repo_root, &report)?;
    enforce_log_size(repo_root, config.max_log_bytes)?;

    println!("mode: {:?}", state.mode);
    println!(
        "ratio: {:.3} (target: {:.3}) gate_passed: {}",
        report.ratio, report.target_ratio, report.gate_passed
    );
    if let Some(pause_until) = state.pause_until {
        println!("pause_until: {}", pause_until.to_rfc3339());
    }

    Ok(())
}

fn cmd_drive(repo_root: &Path, args: DriveArgs) -> Result<()> {
    match args.command.unwrap_or(DriveSubcommand::Start) {
        DriveSubcommand::Start => cmd_drive_start(repo_root),
        DriveSubcommand::Stop => cmd_drive_stop(repo_root),
        DriveSubcommand::Status => cmd_drive_status(repo_root),
        DriveSubcommand::Record(args) => cmd_drive_record(repo_root, args),
    }
}

fn cmd_drive_start(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let config = load_config(repo_root)?;
    validate_transcript_source(&config.transcript)?;

    let mut state = load_state(repo_root)?;
    if let Some(active) = &state.active_drive_session {
        bail!(
            "drive session already active: {} (stop it first with `spp drive stop`)",
            active.session_id
        );
    }

    refresh_pause(&mut state);
    let pause = pause_active(&state);
    let mut report = compute_weekly_report(repo_root, &config, &state)?;
    apply_gate(&mut state, &mut report, pause);

    let history_path = resolve_history_path(repo_root, &config.transcript)?;
    if !history_path.exists() {
        bail!(
            "history file not found: {}. Ensure Codex history persistence is enabled.",
            history_path.display()
        );
    }
    let metadata = fs::metadata(&history_path)
        .with_context(|| format!("failed to stat history file {}", history_path.display()))?;
    let history_offset = metadata.len();

    let session_id = generate_session_id();
    let transcript_path = repo_root
        .join(TRANSCRIPT_DIR)
        .join(format!("{session_id}.jsonl"));
    let control_path = repo_root
        .join(RUNTIME_DIR)
        .join(format!("{session_id}.control"));
    let done_path = repo_root
        .join(RUNTIME_DIR)
        .join(format!("{session_id}.done"));

    if let Some(parent) = transcript_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = control_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&control_path, "run\n")
        .with_context(|| format!("failed to write {}", control_path.display()))?;
    if done_path.exists() {
        fs::remove_file(&done_path)
            .with_context(|| format!("failed to clean stale {}", done_path.display()))?;
    }

    state.mode = Mode::Drive;
    if state.drive_reason.as_deref() != Some("gate") {
        state.drive_reason = Some("manual".to_string());
    }

    let start_event = TranscriptEvent {
        log_schema_version: config.log_schema_version.clone(),
        event_id: generate_event_id(),
        session_id: session_id.clone(),
        event_type: "session_start".to_string(),
        timestamp: Utc::now(),
        mode: state.mode.clone(),
        payload: Some(json!({
            "config_snapshot": {
                "chat_source": config.transcript.chat_source,
                "capture_full_text": config.transcript.capture_full_text,
                "include_file_diff": config.transcript.include_file_diff,
                "max_event_bytes": config.transcript.max_event_bytes,
                "poll_interval_ms": config.transcript.poll_interval_ms
            },
            "history": {
                "path": history_path.to_string_lossy(),
                "offset": history_offset,
                "inode": file_inode(&metadata)
            },
            "git": {
                "branch": git_output(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim()
                    .to_string(),
                "commit": git_output(repo_root, &["rev-parse", "HEAD"]).ok().map(|s| s.trim().to_string())
            }
        })),
        notes: None,
    };
    write_transcript_event(&transcript_path, &start_event)?;

    let recorder_args = build_recorder_args(
        &session_id,
        &config.log_schema_version,
        &transcript_path,
        &history_path,
        history_offset,
        &control_path,
        &done_path,
        &config.transcript,
    );
    let recorder_pid = spawn_recorder(repo_root, &recorder_args)?;

    state.active_drive_session = Some(ActiveDriveSession {
        session_id: session_id.clone(),
        started_at: Utc::now(),
        history_path: history_path.to_string_lossy().to_string(),
        history_offset,
        transcript_path: transcript_path.to_string_lossy().to_string(),
        control_path: control_path.to_string_lossy().to_string(),
        done_path: done_path.to_string_lossy().to_string(),
        recorder_pid: Some(recorder_pid),
    });

    save_state(repo_root, &state)?;
    write_weekly_report(repo_root, &report)?;
    enforce_log_size(repo_root, config.max_log_bytes)?;

    println!("drive session started: {}", session_id);
    println!("transcript: {}", transcript_path.display());
    println!("history source: {}", history_path.display());
    Ok(())
}

fn cmd_drive_stop(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let config = load_config(repo_root)?;
    let mut state = load_state(repo_root)?;

    let active = match state.active_drive_session.clone() {
        Some(active) => active,
        None => bail!("no active drive session"),
    };

    let control_path = PathBuf::from(&active.control_path);
    let done_path = PathBuf::from(&active.done_path);
    let transcript_path = PathBuf::from(&active.transcript_path);

    fs::write(&control_path, "stop\n")
        .with_context(|| format!("failed to write {}", control_path.display()))?;

    let wait_timeout = stop_wait_timeout(&config);
    let done = wait_for_recorder_done(&done_path, wait_timeout)?;
    let timeout = done.is_none();
    let mut timeout_errors = Vec::new();
    if timeout {
        if let Some(pid) = active.recorder_pid {
            if let Err(err) = terminate_recorder_process(pid) {
                timeout_errors.push(format!(
                    "recorder did not finish in time; failed to terminate pid {}: {}",
                    pid, err
                ));
            } else {
                timeout_errors.push(format!(
                    "recorder did not finish in time; sent termination signal to pid {}",
                    pid
                ));
            }
        } else {
            timeout_errors.push("recorder did not finish in time (pid unavailable)".to_string());
        }
    }
    let done = match done {
        Some(done) => done,
        None => RecorderDone {
            session_id: active.session_id.clone(),
            finished_at: Utc::now(),
            history_offset: active.history_offset,
            chat_events: 0,
            diff_events: 0,
            errors: if timeout_errors.is_empty() {
                vec!["recorder did not finish in time".to_string()]
            } else {
                timeout_errors
            },
        },
    };

    let end_event = TranscriptEvent {
        log_schema_version: config.log_schema_version.clone(),
        event_id: generate_event_id(),
        session_id: active.session_id.clone(),
        event_type: "session_end".to_string(),
        timestamp: Utc::now(),
        mode: state.mode.clone(),
        payload: Some(json!({
            "stats": {
                "chat_events": done.chat_events,
                "diff_events": done.diff_events,
                "duration_sec": (Utc::now() - active.started_at).num_seconds().max(0),
                "history_offset": done.history_offset
            },
            "reason": if timeout { "manual_stop_timeout" } else { "manual_stop" },
            "errors": done.errors.clone()
        })),
        notes: None,
    };
    write_transcript_event(&transcript_path, &end_event)?;

    if state.mode == Mode::Drive && state.drive_reason.as_deref() == Some("manual") {
        state.mode = Mode::Normal;
        state.drive_reason = None;
    }
    state.active_drive_session = None;
    save_state(repo_root, &state)?;

    if !timeout {
        let _ = fs::remove_file(control_path);
        let _ = fs::remove_file(done_path);
    }

    println!("drive session stopped: {}", active.session_id);
    println!(
        "summary: chat_events={}, diff_events={}",
        done.chat_events, done.diff_events
    );
    Ok(())
}

fn cmd_drive_status(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let state = load_state(repo_root)?;
    println!("mode: {:?}", state.mode);
    println!(
        "drive_reason: {}",
        state.drive_reason.unwrap_or_else(|| "none".to_string())
    );
    if let Some(active) = state.active_drive_session {
        println!("active_session: {}", active.session_id);
        println!("started_at: {}", active.started_at.to_rfc3339());
        println!("history_path: {}", active.history_path);
        println!("transcript_path: {}", active.transcript_path);
        println!(
            "recorder_pid: {}",
            active
                .recorder_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
    } else {
        println!("active_session: none");
    }
    Ok(())
}

fn cmd_drive_record(repo_root: &Path, args: DriveRecordArgs) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let summary = run_drive_recorder_loop(repo_root, &args).unwrap_or_else(|err| RecorderDone {
        session_id: args.session_id.clone(),
        finished_at: Utc::now(),
        history_offset: args.history_offset,
        chat_events: 0,
        diff_events: 0,
        errors: vec![format!("{err:#}")],
    });
    write_recorder_done(&args.done_path, &summary)?;
    Ok(())
}

fn validate_transcript_source(config: &TranscriptConfig) -> Result<()> {
    if config.chat_source == DEFAULT_CHAT_SOURCE {
        return Ok(());
    }
    bail!(
        "unsupported transcript chat_source `{}` (supported: `{}`)",
        config.chat_source,
        DEFAULT_CHAT_SOURCE
    );
}

fn resolve_history_path(repo_root: &Path, config: &TranscriptConfig) -> Result<PathBuf> {
    if config.history_path == DEFAULT_HISTORY_PATH {
        let codex_home = env::var("CODEX_HOME").ok().map(PathBuf::from).or_else(|| {
            env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        });
        if let Some(codex_home) = codex_home {
            return Ok(codex_home.join("history.jsonl"));
        }
        bail!("failed to resolve history path: set CODEX_HOME or HOME");
    }

    let path = expand_tilde_path(&config.history_path);
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(repo_root.join(path))
}

fn expand_tilde_path(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

fn build_recorder_args(
    session_id: &str,
    log_schema_version: &str,
    transcript_path: &Path,
    history_path: &Path,
    history_offset: u64,
    control_path: &Path,
    done_path: &Path,
    transcript: &TranscriptConfig,
) -> DriveRecordArgs {
    DriveRecordArgs {
        session_id: session_id.to_string(),
        log_schema_version: log_schema_version.to_string(),
        transcript_path: transcript_path.to_path_buf(),
        history_path: history_path.to_path_buf(),
        history_offset,
        control_path: control_path.to_path_buf(),
        done_path: done_path.to_path_buf(),
        include_file_diff: transcript.include_file_diff,
        capture_full_text: transcript.capture_full_text,
        max_event_bytes: transcript.max_event_bytes,
        poll_interval_ms: transcript.poll_interval_ms,
        exclude: transcript.watch_exclude.clone(),
    }
}

fn spawn_recorder(repo_root: &Path, args: &DriveRecordArgs) -> Result<u32> {
    let current_exe = env::current_exe().with_context(|| "failed to resolve current executable")?;
    let mut command = Command::new(current_exe);
    command
        .current_dir(repo_root)
        .arg("drive")
        .arg("record")
        .arg("--session-id")
        .arg(&args.session_id)
        .arg("--log-schema-version")
        .arg(&args.log_schema_version)
        .arg("--transcript-path")
        .arg(&args.transcript_path)
        .arg("--history-path")
        .arg(&args.history_path)
        .arg("--history-offset")
        .arg(args.history_offset.to_string())
        .arg("--control-path")
        .arg(&args.control_path)
        .arg("--done-path")
        .arg(&args.done_path)
        .arg("--include-file-diff")
        .arg(args.include_file_diff.to_string())
        .arg("--capture-full-text")
        .arg(args.capture_full_text.to_string())
        .arg("--max-event-bytes")
        .arg(args.max_event_bytes.to_string())
        .arg("--poll-interval-ms")
        .arg(args.poll_interval_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for pattern in &args.exclude {
        command.arg("--exclude").arg(pattern);
    }
    let child = command
        .spawn()
        .with_context(|| "failed to spawn drive recorder process")?;
    Ok(child.id())
}

fn wait_for_recorder_done(path: &Path, timeout: StdDuration) -> Result<Option<RecorderDone>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            let raw = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let done: RecorderDone =
                serde_json::from_str(&raw).with_context(|| "failed to parse recorder done file")?;
            return Ok(Some(done));
        }
        sleep(StdDuration::from_millis(200));
    }
    Ok(None)
}

fn stop_wait_timeout(config: &AppConfig) -> StdDuration {
    let poll_ms = config.transcript.poll_interval_ms.max(100);
    let dynamic_ms = poll_ms.saturating_mul(3);
    let wait_ms = dynamic_ms.max(15_000);
    StdDuration::from_millis(wait_ms)
}

fn terminate_recorder_process(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .with_context(|| "failed to execute kill command")?;
        if !status.success() {
            bail!("kill -TERM {} exited with status {}", pid, status);
        }
        return Ok(());
    }

    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .with_context(|| "failed to execute taskkill command")?;
        if !status.success() {
            bail!("taskkill {} exited with status {}", pid, status);
        }
        return Ok(());
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        bail!("recorder termination is not supported on this platform");
    }
}

fn write_recorder_done(path: &Path, done: &RecorderDone) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(done)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn run_drive_recorder_loop(repo_root: &Path, args: &DriveRecordArgs) -> Result<RecorderDone> {
    let mut history_offset = args.history_offset;
    let mut snapshot = if args.include_file_diff {
        capture_workspace_text_files(repo_root, &args.exclude, None)?
    } else {
        HashMap::new()
    };
    let mut chat_events = 0_u64;
    let mut diff_events = 0_u64;
    let mut errors = Vec::new();
    let mut error_seen = HashSet::new();
    let poll_interval = StdDuration::from_millis(args.poll_interval_ms.max(100));

    loop {
        let stop_requested = should_stop_recorder(&args.control_path);

        match read_history_values(&args.history_path, history_offset) {
            Ok((next_offset, values)) => {
                history_offset = next_offset;
                for value in values {
                    let messages = extract_chat_messages(&value, args.max_event_bytes);
                    for message in messages {
                        let event_type = if message.role == "assistant" {
                            "chat_assistant"
                        } else {
                            "chat_user"
                        };
                        let content = if args.capture_full_text {
                            truncate_to_bytes(&message.content, args.max_event_bytes)
                        } else {
                            truncate_to_bytes(
                                &summarize_text(&message.content),
                                args.max_event_bytes,
                            )
                        };
                        let payload = json!({
                            "role": message.role,
                            "message_id": message.message_id,
                            "content": content,
                            "raw": message.raw
                        });
                        let event = TranscriptEvent {
                            log_schema_version: args.log_schema_version.clone(),
                            event_id: generate_event_id(),
                            session_id: args.session_id.clone(),
                            event_type: event_type.to_string(),
                            timestamp: Utc::now(),
                            mode: Mode::Drive,
                            payload: Some(payload),
                            notes: None,
                        };
                        if let Err(err) = write_transcript_event(&args.transcript_path, &event) {
                            push_recorder_error(
                                &mut errors,
                                &mut error_seen,
                                format!("failed to write chat event: {err:#}"),
                            );
                        } else {
                            chat_events += 1;
                        }
                    }
                }
            }
            Err(err) => push_recorder_error(
                &mut errors,
                &mut error_seen,
                format!("failed to read history stream: {err:#}"),
            ),
        }

        if args.include_file_diff {
            match capture_workspace_text_files(repo_root, &args.exclude, Some(&snapshot)) {
                Ok(next_snapshot) => {
                    let mut paths: HashSet<String> = HashSet::new();
                    paths.extend(snapshot.keys().cloned());
                    paths.extend(next_snapshot.keys().cloned());
                    let mut paths = paths.into_iter().collect::<Vec<_>>();
                    paths.sort();
                    for path in paths {
                        let before = snapshot.get(&path).map(|state| state.content.as_ref());
                        let after = next_snapshot.get(&path).map(|state| state.content.as_ref());
                        if before == after {
                            continue;
                        }
                        if let Some(diff) = build_unified_diff(before, after, &path) {
                            let payload = json!({
                                "path": path,
                                "diff_unified": truncate_to_bytes(&diff, args.max_event_bytes),
                                "bytes": diff.len(),
                                "language": guess_language(&path),
                            });
                            let event = TranscriptEvent {
                                log_schema_version: args.log_schema_version.clone(),
                                event_id: generate_event_id(),
                                session_id: args.session_id.clone(),
                                event_type: "file_diff".to_string(),
                                timestamp: Utc::now(),
                                mode: Mode::Drive,
                                payload: Some(payload),
                                notes: None,
                            };
                            if let Err(err) = write_transcript_event(&args.transcript_path, &event)
                            {
                                push_recorder_error(
                                    &mut errors,
                                    &mut error_seen,
                                    format!("failed to write diff event: {err:#}"),
                                );
                            } else {
                                diff_events += 1;
                            }
                        }
                    }
                    snapshot = next_snapshot;
                }
                Err(err) => push_recorder_error(
                    &mut errors,
                    &mut error_seen,
                    format!("failed to capture workspace snapshot: {err:#}"),
                ),
            }
        }

        if stop_requested {
            break;
        }

        sleep(poll_interval);
    }

    Ok(RecorderDone {
        session_id: args.session_id.clone(),
        finished_at: Utc::now(),
        history_offset,
        chat_events,
        diff_events,
        errors,
    })
}

fn push_recorder_error(errors: &mut Vec<String>, seen: &mut HashSet<String>, message: String) {
    let truncated_marker = "__truncated__";
    if errors.len() >= MAX_RECORDER_ERRORS + 1 {
        return;
    }

    if !seen.insert(message.clone()) {
        return;
    }

    errors.push(message);
    if errors.len() == MAX_RECORDER_ERRORS && !seen.contains(truncated_marker) {
        let _ = seen.insert(truncated_marker.to_string());
        errors.push(format!(
            "recorder errors truncated at {} unique entries",
            MAX_RECORDER_ERRORS
        ));
    }
}

fn should_stop_recorder(control_path: &Path) -> bool {
    match fs::read_to_string(control_path) {
        Ok(value) => value.trim() == "stop",
        Err(err) if err.kind() == ErrorKind::NotFound => true,
        Err(_) => true,
    }
}

fn read_history_values(path: &Path, offset: u64) -> Result<(u64, Vec<Value>)> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("failed to seek {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut current_offset = offset;
    let mut values = Vec::new();

    loop {
        let mut line = String::new();
        let offset_before_read = current_offset;
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        if !line.ends_with('\n') {
            reader
                .get_mut()
                .seek(SeekFrom::Start(offset_before_read))
                .with_context(|| format!("failed to rewind {}", path.display()))?;
            current_offset = offset_before_read;
            break;
        }

        let line = line.trim();
        current_offset += bytes_read as u64;
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(line) {
            Ok(value) => values.push(value),
            Err(_) => {
                reader
                    .get_mut()
                    .seek(SeekFrom::Start(offset_before_read))
                    .with_context(|| format!("failed to rewind {}", path.display()))?;
                current_offset = offset_before_read;
                break;
            }
        }
    }

    Ok((current_offset, values))
}

fn extract_chat_messages(value: &Value, max_event_bytes: u64) -> Vec<ChatMessage> {
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        let mut out = Vec::new();
        for item in messages {
            if let Some(msg) = extract_single_chat_message(item, max_event_bytes) {
                out.push(msg);
            }
        }
        return out;
    }

    extract_single_chat_message(value, max_event_bytes)
        .map(|msg| vec![msg])
        .unwrap_or_default()
}

fn extract_single_chat_message(value: &Value, max_event_bytes: u64) -> Option<ChatMessage> {
    let role = resolve_chat_role(value)?;
    let content_value = value
        .pointer("/message/content")
        .or_else(|| value.get("content"))
        .or_else(|| value.pointer("/message/text"))
        .or_else(|| value.get("text"))?;
    let content = flatten_content(content_value)?;
    if content.trim().is_empty() {
        return None;
    }
    let content = truncate_to_bytes(&content, max_event_bytes);
    let message_id = value
        .pointer("/message/id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let raw = {
        let rendered = serde_json::to_string(value).ok();
        if let Some(rendered) = rendered {
            if rendered.len() as u64 <= max_event_bytes {
                Some(value.clone())
            } else {
                None
            }
        } else {
            None
        }
    };

    Some(ChatMessage {
        role: role.to_string(),
        content,
        message_id,
        raw,
    })
}

fn resolve_chat_role(value: &Value) -> Option<&'static str> {
    let candidate = value
        .pointer("/message/role")
        .or_else(|| value.get("role"))
        .or_else(|| value.pointer("/item/role"))
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)?;
    let candidate = candidate.to_lowercase();
    if candidate.contains("assistant") {
        return Some("assistant");
    }
    if candidate.contains("user") {
        return Some("user");
    }
    None
}

fn flatten_content(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    if let Some(arr) = value.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(text) = item.as_str() {
                parts.push(text.to_string());
                continue;
            }
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
                continue;
            }
            if let Some(text) = item.pointer("/content/text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }

    if let Some(obj) = value.as_object() {
        if let Some(text) = obj.get("text").and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    None
}

fn summarize_text(input: &str) -> String {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_to_bytes(&normalized, 280)
}

fn truncate_to_bytes(input: &str, max_bytes: u64) -> String {
    if input.len() as u64 <= max_bytes {
        return input.to_string();
    }

    let mut end = 0usize;
    for (idx, _) in input.char_indices() {
        if idx as u64 > max_bytes.saturating_sub(14) {
            break;
        }
        end = idx;
    }
    let mut out = input[..end].to_string();
    out.push_str("...[truncated]");
    out
}

fn capture_workspace_text_files(
    repo_root: &Path,
    exclude: &[String],
    previous: Option<&HashMap<String, FileState>>,
) -> Result<HashMap<String, FileState>> {
    let mut snapshot = HashMap::new();
    for entry in WalkDir::new(repo_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = match entry.path().strip_prefix(repo_root) {
            Ok(rel) => rel,
            Err(_) => continue,
        };
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        if is_excluded_path(&rel_path, exclude) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let len = metadata.len();
        if len > 2_000_000 {
            continue;
        }
        let modified = metadata.modified().ok();

        if let Some(prev) = previous.and_then(|prev| prev.get(&rel_path)) {
            if prev.len == len && prev.modified == modified {
                snapshot.insert(rel_path, prev.clone());
                continue;
            }
        }

        let mut bytes = Vec::new();
        if File::open(entry.path())
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .is_err()
        {
            continue;
        }
        if bytes.contains(&0) {
            continue;
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => continue,
        };
        snapshot.insert(
            rel_path,
            FileState {
                len,
                modified,
                content: Arc::<str>::from(text),
            },
        );
    }
    Ok(snapshot)
}

fn is_excluded_path(path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|pattern| {
        let normalized = pattern.trim_start_matches("./").trim_end_matches('/');
        path == normalized || path.starts_with(&format!("{normalized}/"))
    })
}

fn build_unified_diff(before: Option<&str>, after: Option<&str>, path: &str) -> Option<String> {
    let old = before.unwrap_or_default();
    let new = after.unwrap_or_default();
    if old == new {
        return None;
    }
    let diff = TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string();
    if diff.trim().is_empty() {
        None
    } else {
        Some(diff)
    }
}

fn guess_language(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(OsStr::to_str)
        .map(|ext| ext.to_lowercase())
        .unwrap_or_else(|| "unknown".to_string())
}

fn generate_session_id() -> String {
    format!(
        "{}-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        std::process::id(),
        EVENT_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn generate_event_id() -> String {
    format!(
        "evt-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        EVENT_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn write_transcript_event(path: &Path, event: &TranscriptEvent) -> Result<()> {
    append_jsonl(path, event)
}

fn file_inode(metadata: &fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        Some(metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn cmd_pause(repo_root: &Path, args: PauseArgs) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let mut state = load_state(repo_root)?;
    let hours = args.hours.clamp(1, 24);
    state.pause_until = Some(Utc::now() + Duration::hours(i64::from(hours)));
    state.updated_at = Utc::now();
    save_state(repo_root, &state)?;
    println!("gate checks paused for {} hour(s)", hours);
    Ok(())
}

fn cmd_resume(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let mut state = load_state(repo_root)?;
    state.pause_until = None;
    state.updated_at = Utc::now();
    save_state(repo_root, &state)?;
    println!("pause cleared");
    Ok(())
}

fn cmd_reset(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let state = State::default();
    save_state(repo_root, &state)?;

    for dir in [
        repo_root.join(WEEKLY_DIR),
        repo_root.join(TRANSCRIPT_DIR),
        repo_root.join(RUNTIME_DIR),
    ] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::remove_file(entry.path())?;
            }
        }
    }

    println!("weekly state reset");
    Ok(())
}

fn cmd_codex(repo_root: &Path, args: CodexArgs) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let config = load_config(repo_root)?;
    let mut state = load_state(repo_root)?;
    refresh_pause(&mut state);

    let pause = pause_active(&state);
    let mut report = compute_weekly_report(repo_root, &config, &state)?;
    apply_gate(&mut state, &mut report, pause);

    let codex_mode = match state.mode {
        Mode::Normal => &config.codex.normal,
        Mode::Drive => &config.codex.drive,
    };

    validate_codex_extra_args(&args.extra)?;

    let mut codex_args = args.extra.clone();
    codex_args.extend([
        "--sandbox".to_string(),
        codex_mode.sandbox.clone(),
        "--ask-for-approval".to_string(),
        codex_mode.approval.clone(),
    ]);

    let branch = git_output(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();
    let commit = git_output(repo_root, &["rev-parse", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string());

    let session_entry = SessionLogEntry {
        log_schema_version: config.log_schema_version.clone(),
        timestamp: Utc::now(),
        command: format!("codex {}", codex_args.join(" ")),
        mode: state.mode.clone(),
        sandbox: codex_mode.sandbox.clone(),
        approval: codex_mode.approval.clone(),
        git_branch: branch,
        git_commit: commit,
        gate_ratio: Some(report.ratio),
        gate_target: Some(report.target_ratio),
        notes: Some(format!(
            "gate_passed={}, pause_active={}",
            report.gate_passed, pause
        )),
    };

    save_state(repo_root, &state)?;
    write_weekly_report(repo_root, &report)?;
    write_session_log(repo_root, &session_entry)?;
    enforce_log_size(repo_root, config.max_log_bytes)?;

    if args.dry_run {
        println!("dry-run: codex {}", codex_args.join(" "));
        return Ok(());
    }

    let status = Command::new("codex")
        .args(&codex_args)
        .status()
        .with_context(|| "failed to start codex command")?;
    if !status.success() {
        bail!("codex exited with status {}", status);
    }

    Ok(())
}

fn validate_codex_extra_args(extra: &[String]) -> Result<()> {
    for arg in extra {
        if arg == "--full-auto" || arg.starts_with("--full-auto=") {
            bail!("`--full-auto` is prohibited by SPP policy");
        }
        if arg == "--sandbox" || arg.starts_with("--sandbox=") {
            bail!("`--sandbox` is managed by spp and cannot be overridden");
        }
        if arg == "--ask-for-approval" || arg.starts_with("--ask-for-approval=") {
            bail!("`--ask-for-approval` is managed by spp and cannot be overridden");
        }
    }
    Ok(())
}

fn cmd_attrib_fix(repo_root: &Path, args: AttribFixArgs) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let full_commit = git_output(repo_root, &["rev-parse", "--verify", &args.commit])?
        .trim()
        .to_string();
    if full_commit.is_empty() {
        bail!("commit not found: {}", args.commit);
    }

    let mut state = load_state(repo_root)?;
    state
        .attribution_overrides
        .insert(full_commit.clone(), args.actor);
    state.updated_at = Utc::now();
    save_state(repo_root, &state)?;
    println!(
        "attribution override saved: {} => {:?}",
        full_commit, args.actor
    );
    Ok(())
}

fn ensure_runtime_dirs(repo_root: &Path) -> Result<()> {
    fs::create_dir_all(repo_root.join(SESSION_DIR))?;
    fs::create_dir_all(repo_root.join(WEEKLY_DIR))?;
    fs::create_dir_all(repo_root.join(TRANSCRIPT_DIR))?;
    fs::create_dir_all(repo_root.join(RUNTIME_DIR))?;
    Ok(())
}

fn detect_repo_root() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .with_context(|| "failed to execute git")?;
    if !out.status.success() {
        bail!("current directory is not a git repository");
    }
    let root = String::from_utf8(out.stdout)?.trim().to_string();
    if root.is_empty() {
        bail!("failed to resolve git repo root");
    }
    Ok(PathBuf::from(root))
}

fn load_config(repo_root: &Path) -> Result<AppConfig> {
    let runtime_path = repo_root.join(RUNTIME_CONFIG);
    if runtime_path.exists() {
        let text = fs::read_to_string(&runtime_path)
            .with_context(|| format!("failed to read {}", runtime_path.display()))?;
        let cfg: AppConfig =
            toml::from_str(&text).with_context(|| "failed to parse runtime config")?;
        return Ok(cfg);
    }

    let template_path = repo_root.join(TEMPLATE_CONFIG);
    if template_path.exists() {
        let text = fs::read_to_string(&template_path)
            .with_context(|| format!("failed to read {}", template_path.display()))?;
        let cfg: AppConfig =
            toml::from_str(&text).with_context(|| "failed to parse template config")?;
        return Ok(cfg);
    }

    Ok(AppConfig::default())
}

fn load_state(repo_root: &Path) -> Result<State> {
    let path = repo_root.join(STATE_FILE);
    if !path.exists() {
        return Ok(State::default());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let state: State = serde_json::from_str(&text).with_context(|| "failed to parse state")?;
    Ok(state)
}

fn save_state(repo_root: &Path, state: &State) -> Result<()> {
    let mut state = state.clone();
    state.updated_at = Utc::now();
    let path = repo_root.join(STATE_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&state)?;
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn refresh_pause(state: &mut State) {
    if let Some(until) = state.pause_until {
        if Utc::now() >= until {
            state.pause_until = None;
        }
    }
}

fn pause_active(state: &State) -> bool {
    matches!(state.pause_until, Some(until) if Utc::now() < until)
}

fn compute_weekly_report(
    repo_root: &Path,
    config: &AppConfig,
    state: &State,
) -> Result<WeeklyReport> {
    let now = Utc::now();
    let iso = now.iso_week();
    let year = iso.year();
    let iso_week = iso.week();
    let metrics = collect_weekly_metrics(repo_root, config, state, year, iso_week)?;
    let total = metrics.human_lines_added + metrics.ai_lines_added;
    let ratio = if total == 0 {
        1.0
    } else {
        metrics.human_lines_added as f64 / total as f64
    };
    let gate_passed = ratio >= config.weekly_ratio_target;
    Ok(WeeklyReport {
        log_schema_version: config.log_schema_version.clone(),
        generated_at: now,
        year,
        iso_week,
        human_lines_added: metrics.human_lines_added,
        ai_lines_added: metrics.ai_lines_added,
        human_commit_count: metrics.human_commit_count,
        ai_commit_count: metrics.ai_commit_count,
        ratio,
        target_ratio: config.weekly_ratio_target,
        gate_passed,
        mode_after_evaluation: state.mode.clone(),
        notes: metrics.notes,
    })
}

fn apply_gate(state: &mut State, report: &mut WeeklyReport, pause_active: bool) {
    if pause_active {
        report
            .notes
            .push("gate evaluation bypassed due to active pause".to_string());
        report.mode_after_evaluation = state.mode.clone();
        return;
    }

    if report.gate_passed {
        if state.mode == Mode::Drive && state.drive_reason.as_deref() == Some("gate") {
            state.mode = Mode::Normal;
            state.drive_reason = None;
        }
    } else {
        state.mode = Mode::Drive;
        state.drive_reason = Some("gate".to_string());
        report
            .notes
            .push("ratio below target, forced drive mode".to_string());
    }

    report.mode_after_evaluation = state.mode.clone();
}

fn collect_weekly_metrics(
    repo_root: &Path,
    config: &AppConfig,
    state: &State,
    year: i32,
    iso_week: u32,
) -> Result<WeeklyMetrics> {
    let monday = NaiveDate::from_isoywd_opt(year, iso_week, Weekday::Mon)
        .with_context(|| "failed to compute start of ISO week")?;
    let start = monday
        .and_hms_opt(0, 0, 0)
        .with_context(|| "invalid week start timestamp")?;
    let end = start + Duration::days(7);

    let since = start.and_utc().to_rfc3339();
    let until = end.and_utc().to_rfc3339();

    let commits_raw = git_output(
        repo_root,
        &[
            "log",
            "--since",
            &since,
            "--until",
            &until,
            "--no-merges",
            "--pretty=format:%H",
        ],
    )?;
    let mut metrics = WeeklyMetrics::default();

    for commit in commits_raw.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let commit = commit.to_string();
        let actor = classify_actor(repo_root, &commit, config, state)?;
        let added_lines = commit_added_lines(repo_root, &commit)?;

        match actor {
            Actor::Human => {
                metrics.human_commit_count += 1;
                metrics.human_lines_added += added_lines;
            }
            Actor::Ai => {
                metrics.ai_commit_count += 1;
                metrics.ai_lines_added += added_lines;
            }
        }
    }

    Ok(metrics)
}

fn classify_actor(
    repo_root: &Path,
    commit: &str,
    config: &AppConfig,
    state: &State,
) -> Result<Actor> {
    if let Some(actor) = state.attribution_overrides.get(commit).copied() {
        return Ok(actor);
    }

    let trailer_output = git_output(repo_root, &["show", "-s", "--format=%B", commit])?;
    let trailer_lc = trailer_output.to_lowercase();
    if trailer_lc.contains("co-authored-by: codex") {
        return Ok(Actor::Ai);
    }

    let email = git_output(repo_root, &["show", "-s", "--format=%ae", commit])?;
    let email_lc = email.trim().to_lowercase();
    if config
        .attribution
        .codex_author_emails
        .iter()
        .any(|candidate| candidate.to_lowercase() == email_lc)
    {
        return Ok(Actor::Ai);
    }

    if let Ok(notes) = git_output(repo_root, &["notes", "show", commit]) {
        let notes_lc = notes.to_lowercase();
        if notes_lc.contains("spp:ai") {
            return Ok(Actor::Ai);
        }
        if notes_lc.contains("spp:human") {
            return Ok(Actor::Human);
        }
    }

    Ok(Actor::Human)
}

fn commit_added_lines(repo_root: &Path, commit: &str) -> Result<u64> {
    let out = git_output(repo_root, &["show", "--numstat", "--format=", commit])?;
    let mut sum = 0_u64;
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut parts = line.split('\t');
        let added = parts.next().unwrap_or_default();
        if added == "-" {
            continue;
        }
        if let Ok(v) = added.parse::<u64>() {
            sum += v;
        }
    }
    Ok(sum)
}

fn git_output(repo_root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("failed to execute git {:?}", args))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("git {:?} failed: {}", args, stderr);
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn write_weekly_report(repo_root: &Path, report: &WeeklyReport) -> Result<()> {
    let path = repo_root
        .join(WEEKLY_DIR)
        .join(format!("{}-W{:02}.json", report.year, report.iso_week));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(report)?;
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn write_session_log(repo_root: &Path, entry: &SessionLogEntry) -> Result<()> {
    let iso = entry.timestamp.iso_week();
    let path = repo_root
        .join(SESSION_DIR)
        .join(format!("{}-W{:02}.jsonl", iso.year(), iso.week()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    append_jsonl(&path, entry)
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(value)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn write_text_asset(path: &Path, content: &str, force: bool) -> Result<WriteOutcome> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    if path.exists() && !force {
        return Ok(WriteOutcome::Skipped);
    }

    let outcome = if path.exists() {
        WriteOutcome::Overwritten
    } else {
        WriteOutcome::Created
    };
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(outcome)
}

fn ensure_gitignore_rule(path: &Path, rule: &str) -> Result<bool> {
    let mut content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };

    let has_rule = content.lines().any(|line| {
        let normalized = line.trim();
        normalized == rule || normalized == ".codex-spp/" || normalized == "/.codex-spp"
    });
    if has_rule {
        return Ok(false);
    }

    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(rule);
    content.push('\n');

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn enforce_log_size(repo_root: &Path, max_bytes: u64) -> Result<()> {
    let mut files = collect_log_files(repo_root)?;
    let mut total: u64 = files.iter().map(|f| f.size).sum();
    if total <= max_bytes {
        return Ok(());
    }

    files.sort_by_key(|f| f.modified);
    for file in files {
        if total <= max_bytes {
            break;
        }
        fs::remove_file(&file.path)
            .with_context(|| format!("failed to remove {}", file.path.display()))?;
        total = total.saturating_sub(file.size);
    }
    Ok(())
}

#[derive(Debug)]
struct SizedFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

fn collect_log_files(repo_root: &Path) -> Result<Vec<SizedFile>> {
    let mut files = Vec::new();
    for rel in [SESSION_DIR, WEEKLY_DIR, TRANSCRIPT_DIR] {
        let dir = repo_root.join(rel);
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(OsStr::to_str).is_none() {
                continue;
            }
            let metadata = entry.metadata()?;
            files.push(SizedFile {
                path,
                size: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
    }
    Ok(files)
}

#[allow(dead_code)]
fn touch(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = File::options().create(true).append(true).open(path)?;
    Ok(())
}
