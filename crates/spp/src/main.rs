use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

const RUNTIME_CONFIG: &str = ".codex-spp/config.toml";
const STATE_FILE: &str = ".codex-spp/state.json";
const SESSION_DIR: &str = ".codex-spp/sessions";
const WEEKLY_DIR: &str = ".codex-spp/weekly";
const TEMPLATE_CONFIG: &str = "template_spp.config.toml";
const PROJECT_RUNTIME_CONFIG_FILE: &str = ".codex-spp/config.toml";
const PROJECT_CODEX_CONFIG_FILE: &str = ".codex/config.toml";
const GITIGNORE_RULE_CODEX_SPP: &str = "/.codex-spp/";

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
        "skills/spp-drive/SKILL.md",
        include_str!("../../../skills/spp-drive/SKILL.md"),
    ),
    (
        "skills/spp-coach/SKILL.md",
        include_str!("../../../skills/spp-coach/SKILL.md"),
    ),
    (
        "skills/spp-stats/SKILL.md",
        include_str!("../../../skills/spp-stats/SKILL.md"),
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
    Drive,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AttributionConfig {
    codex_author_emails: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            log_schema_version: "1.0".to_string(),
            weekly_ratio_target: 0.70,
            max_log_bytes: 524_288_000,
            diff_snapshot_enabled: false,
            codex: CodexConfig::default(),
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
struct State {
    mode: Mode,
    drive_reason: Option<String>,
    pause_until: Option<DateTime<Utc>>,
    attribution_overrides: HashMap<String, Actor>,
    updated_at: DateTime<Utc>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            drive_reason: None,
            pause_until: None,
            attribution_overrides: HashMap::new(),
            updated_at: Utc::now(),
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
                Commands::Drive => cmd_drive(&repo_root),
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

fn cmd_drive(repo_root: &Path) -> Result<()> {
    ensure_runtime_dirs(repo_root)?;
    let mut state = load_state(repo_root)?;
    state.mode = Mode::Drive;
    state.drive_reason = Some("manual".to_string());
    state.updated_at = Utc::now();
    save_state(repo_root, &state)?;
    println!("mode switched to drive");
    Ok(())
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

    let weekly_dir = repo_root.join(WEEKLY_DIR);
    if weekly_dir.exists() {
        for entry in fs::read_dir(&weekly_dir)? {
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
    for rel in [SESSION_DIR, WEEKLY_DIR] {
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
