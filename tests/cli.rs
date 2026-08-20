//! End-to-end CLI tests.
//!
//! Each test spawns the real `engrym` binary against throwaway directories,
//! fully isolated via per-process `HOME` / `ENGRYM_HOME` (so parallel tests
//! never race on env or touch the developer's machine). Embedding is always
//! skipped (`--no-embed` / `--keyword`) so the suite stays offline and fast —
//! the embedding/daemon paths are covered by unit tests, not here.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_engrym");

/// An isolated workspace: a fake `$HOME` (also the `$ENGRYM_HOME` parent) and a
/// separate repo directory.
struct Workspace {
    home: TempDir,
    repo: TempDir,
}

impl Workspace {
    fn new() -> Self {
        Workspace { home: tempdir(), repo: tempdir() }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }
    fn repo(&self) -> &Path {
        self.repo.path()
    }

    /// Mark the repo as a git repo (`repo_anchor` only checks `.git` exists).
    fn git_init(&self) -> &Self {
        fs::create_dir_all(self.repo().join(".git")).unwrap();
        self
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(self.repo(), args)
    }

    fn run_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let out = Command::new(BIN)
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.home())
            .env("ENGRYM_HOME", self.home().join(".engrym"))
            .env("ENGRYM_NO_DAEMON", "1")
            .output()
            .expect("spawn engrym");
        Output {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `$ENGRYM_HOME/projects` — where local-mode KBs live.
    fn store(&self) -> PathBuf {
        self.home().join(".engrym").join("projects")
    }

    /// Scaffold an in-repo KB and author a small connected graph (no index yet).
    fn seed(&self) {
        self.run(&["init", "--agent", "none"]).ok();
        self.new_doc("overview", 0, "core", &[], "# Overview\nThe entry point is main.rs.");
        self.new_doc(
            "auth",
            1,
            "core/auth",
            &["refines:overview"],
            "# Auth\nSessions use OAuth token refresh.",
        );
    }

    fn new_doc(&self, id: &str, altitude: u8, topic: &str, relations: &[&str], body: &str) -> Output {
        let alt = altitude.to_string();
        let mut args = vec![
            "new", id, "--title", id, "--altitude", &alt, "--topic", topic, "--body", body,
        ];
        for r in relations {
            args.push("--relation");
            args.push(r);
        }
        self.run(&args)
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Output {
    fn ok(&self) -> &Self {
        assert_eq!(
            self.code, 0,
            "expected success\nstdout: {}\nstderr: {}",
            self.stdout, self.stderr
        );
        self
    }
    fn fail(&self) -> &Self {
        assert_ne!(self.code, 0, "expected failure\nstdout: {}", self.stdout);
        self
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout was not JSON ({e}):\n{}", self.stdout))
    }
    fn has(&self, needle: &str) -> &Self {
        assert!(
            self.stdout.contains(needle),
            "stdout missing {needle:?}\nstdout: {}",
            self.stdout
        );
        self
    }
    fn err_has(&self, needle: &str) -> &Self {
        assert!(
            self.stderr.contains(needle),
            "stderr missing {needle:?}\nstderr: {}",
            self.stderr
        );
        self
    }
}

fn tempdir() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// A fake clone: a `.git/` dir whose config carries an `origin` remote, enough
/// for `repo_anchor` (checks `.git` exists) and `repo_identity` (reads the URL).
fn fake_clone(dir: &Path, origin: &str) {
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(
        dir.join(".git").join("config"),
        format!("[remote \"origin\"]\n\turl = {origin}\n"),
    )
    .unwrap();
}

/// The single subdirectory of `dir` (asserts there is exactly one).
fn only_subdir(dir: &Path) -> PathBuf {
    let mut subs: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|_| panic!("reading {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(subs.len(), 1, "expected one subdir in {}", dir.display());
    subs.pop().unwrap()
}

// --------------------------------------------------------------------------
// init — in-repo
// --------------------------------------------------------------------------

#[test]
fn init_in_repo_scaffolds_and_gitignores() {
    let ws = Workspace::new();
    let v = ws.run(&["init", "--agent", "none", "--json"]).ok().json();
    assert_eq!(v["local"], false);

    assert!(ws.repo().join("engrym.toml").is_file());
    assert!(ws.repo().join("docs").is_dir());
    let gitignore = fs::read_to_string(ws.repo().join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|l| l.trim() == ".engrym/"), "{gitignore}");
}

#[test]
fn init_refuses_when_already_initialized() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none"]).ok();
    ws.run(&["init", "--agent", "none"]).fail().err_has("already");
}

#[test]
fn init_force_rescaffolds() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none"]).ok();
    ws.run(&["init", "--agent", "none", "--force"]).ok();
}

#[test]
fn init_in_repo_claude_skill_is_project_level() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "claude", "--json"]).ok();
    assert!(ws.repo().join(".claude/skills/engrym/SKILL.md").is_file());
    assert!(ws.repo().join(".claude/skills/engrym-bootstrap/SKILL.md").is_file());
    // The repo-level dir is used, not the user-global one.
    assert!(!ws.home().join(".claude/skills/engrym/SKILL.md").exists());
}

#[test]
fn init_docs_flag_sets_the_docs_root() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none", "--docs", "kb", "--json"]).ok();
    let cfg = fs::read_to_string(ws.repo().join("engrym.toml")).unwrap();
    assert!(cfg.contains("root = \"kb\""), "config root not set:\n{cfg}");
    assert!(ws.repo().join("kb").is_dir());
    assert!(!ws.repo().join("docs").exists());

    // And authoring + indexing honor it.
    ws.new_doc("x", 0, "core", &[], "# X").ok();
    assert!(ws.repo().join("kb/x.md").is_file());
    ws.run(&["index", "--no-embed", "--json"]).ok();
}

#[test]
fn init_docs_flag_rejects_unsafe_paths() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none", "--docs", ".."]).fail().err_has("relative path");
    let ws2 = Workspace::new();
    ws2.run(&["init", "--agent", "none", "--docs", "/etc"]).fail().err_has("relative path");
}

#[test]
fn init_handoff_prompt_tells_agent_not_to_reinitialize() {
    let ws = Workspace::new();
    // A fake agent that just records the prompt it was handed.
    let agent = ws.repo().join("fake-agent.sh");
    fs::write(&agent, "#!/bin/sh\nprintf '%s' \"$1\" > \"$PWD/handoff.txt\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&agent).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&agent, perm).unwrap();
    }
    // Non-JSON path triggers the launch; `--agent-cmd` runs our fake agent.
    let cmd = format!("{} {{prompt}}", agent.display());
    ws.run(&["init", "--agent-cmd", &cmd]).ok();

    let prompt = fs::read_to_string(ws.repo().join("handoff.txt")).unwrap();
    assert!(
        prompt.contains("do NOT run `engrym init`"),
        "handoff prompt should warn against re-init; was: {prompt}"
    );
}

// --------------------------------------------------------------------------
// init — local mode
// --------------------------------------------------------------------------

#[test]
fn init_local_leaves_repo_untouched() {
    let ws = Workspace::new();
    ws.git_init();
    let v = ws.run(&["init", "--local", "--agent", "none", "--json"]).ok().json();
    assert_eq!(v["local"], true);

    // Nothing in the repo.
    assert!(!ws.repo().join("engrym.toml").exists());
    assert!(!ws.repo().join("docs").exists());
    assert!(!ws.repo().join(".engrym").exists());

    // Everything in the external store, under one project key.
    let proj = only_subdir(&ws.store());
    assert!(proj.join("engrym.toml").is_file());
    assert!(proj.join("docs").is_dir());
    let cfg = fs::read_to_string(proj.join("engrym.toml")).unwrap();
    assert!(cfg.contains("Bound to repo:"), "local config header missing:\n{cfg}");
}

#[test]
fn init_local_also_records_the_repo_in_global_memory() {
    let ws = Workspace::new();
    ws.git_init();
    ws.run(&["init", "--local", "--agent", "claude", "--json"]).ok();

    let mem = ws.home().join(".claude/CLAUDE.md");
    let body = fs::read_to_string(&mem).unwrap_or_default();
    let repo_canon = fs::canonicalize(ws.repo()).unwrap();
    assert!(body.contains("engrym knowledge bases"), "memory note missing:\n{body}");
    assert!(body.contains(repo_canon.to_str().unwrap()), "repo not listed:\n{body}");
}

#[test]
fn init_in_repo_also_records_global_memory() {
    let ws = Workspace::new();
    // init records the repo in the agent's global memory in both modes.
    ws.run(&["init", "--agent", "claude", "--json"]).ok();
    let body = fs::read_to_string(ws.home().join(".claude/CLAUDE.md")).unwrap_or_default();
    let repo_canon = fs::canonicalize(ws.repo()).unwrap();
    assert!(body.contains("engrym knowledge bases"), "memory note missing:\n{body}");
    assert!(body.contains(repo_canon.to_str().unwrap()), "repo not listed:\n{body}");
}

#[test]
fn init_local_claude_skill_is_user_global() {
    let ws = Workspace::new();
    ws.git_init();
    ws.run(&["init", "--local", "--agent", "claude", "--json"]).ok();
    // User-global, never the repo.
    assert!(ws.home().join(".claude/skills/engrym/SKILL.md").is_file());
    assert!(!ws.repo().join(".claude").exists());
}

#[test]
fn local_kb_resolves_for_all_commands_from_a_subdir() {
    let ws = Workspace::new();
    ws.git_init();
    ws.run(&["init", "--local", "--agent", "none"]).ok();

    // Author + index from the repo root, then query from a nested subdir.
    ws.new_doc("overview", 0, "core", &[], "# Overview\nThe entry point is main.rs.").ok();
    ws.run(&["index", "--no-embed"]).ok().has("Local KB");

    let deep = ws.repo().join("src/inner");
    fs::create_dir_all(&deep).unwrap();
    let v = ws.run_in(&deep, &["search", "entry point", "--keyword", "--json"]).ok().json();
    assert!(
        v.as_array().unwrap().iter().any(|h| h["id"] == "overview"),
        "expected overview hit from subdir: {v}"
    );

    // The doc and index live in the store, not the repo.
    let proj = only_subdir(&ws.store());
    assert!(proj.join("docs/overview.md").is_file());
    assert!(!ws.repo().join("docs").exists());
}

// --------------------------------------------------------------------------
// install
// --------------------------------------------------------------------------

#[test]
fn install_skills_claude() {
    let ws = Workspace::new();
    let v = ws.run(&["install", "skills", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["agent"], "claude");
    assert!(ws.repo().join(".claude/skills/engrym/SKILL.md").is_file());
    assert!(ws.repo().join(".claude/skills/engrym-bootstrap/SKILL.md").is_file());
}

#[test]
fn install_skills_opencode() {
    let ws = Workspace::new();
    let v = ws.run(&["install", "skills", "--agent", "opencode", "--json"]).ok().json();
    assert_eq!(v["agent"], "opencode");
    assert!(ws.repo().join(".opencode/skills/engrym/SKILL.md").is_file());
    assert!(ws.repo().join(".opencode/skills/engrym-bootstrap/SKILL.md").is_file());
}

#[test]
fn install_skills_opencode_local_uses_user_global_dir() {
    let ws = Workspace::new();
    ws.run(&["install", "skills", "--agent", "opencode", "--local"]).ok();
    assert!(ws.home().join(".config/opencode/skills/engrym/SKILL.md").is_file());
    assert!(!ws.repo().join(".opencode").exists());
}

#[test]
fn install_skills_unknown_agent_fails() {
    let ws = Workspace::new();
    ws.run(&["install", "skills", "--agent", "nope"]).fail().err_has("unknown agent");
}

#[test]
fn install_skills_for_cli_only_agent_fails() {
    let ws = Workspace::new();
    ws.run(&["install", "skills", "--agent", "gemini"]).fail().err_has("no engrym skill");
}

// --------------------------------------------------------------------------
// uninstall
// --------------------------------------------------------------------------

#[test]
fn uninstall_skills_removes_and_is_idempotent() {
    let ws = Workspace::new();
    ws.run(&["install", "skills", "--agent", "claude"]).ok();
    let v = ws.run(&["uninstall", "skills", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["removed"].as_array().unwrap().len(), 2);
    assert!(!ws.repo().join(".claude/skills/engrym").exists());

    // Second time: nothing left to remove.
    let v = ws.run(&["uninstall", "skills", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["removed"].as_array().unwrap().len(), 0);
}

// --------------------------------------------------------------------------
// install / uninstall memory (global per-project cue)
// --------------------------------------------------------------------------

#[test]
fn install_memory_records_repo_in_global_file_not_the_repo() {
    let ws = Workspace::new();
    ws.git_init();
    let v = ws.run(&["install", "memory", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["agent"], "claude");
    assert_eq!(v["added"], true);

    // Written to ~/.claude/CLAUDE.md, never into the repo.
    let mem = ws.home().join(".claude/CLAUDE.md");
    let body = fs::read_to_string(&mem).unwrap();
    assert!(body.contains("engrym knowledge bases"));
    let repo_canon = fs::canonicalize(ws.repo()).unwrap();
    assert!(body.contains(repo_canon.to_str().unwrap()), "repo path not listed:\n{body}");
    assert!(!ws.repo().join("CLAUDE.md").exists());
}

#[test]
fn install_memory_codex_uses_codex_agents_md() {
    let ws = Workspace::new();
    ws.git_init();
    ws.run(&["install", "memory", "--agent", "codex"]).ok();
    assert!(ws.home().join(".codex/AGENTS.md").is_file());
    assert!(!ws.home().join(".agents/AGENTS.md").exists());
}

#[test]
fn install_memory_is_idempotent_and_uninstall_reverts() {
    let ws = Workspace::new();
    ws.git_init();
    ws.run(&["install", "memory", "--agent", "claude"]).ok();
    // Second add: already present.
    let v = ws.run(&["install", "memory", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["added"], false);

    let v = ws.run(&["uninstall", "memory", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["removed"], true);
    // Block removed once empty.
    let body = fs::read_to_string(ws.home().join(".claude/CLAUDE.md")).unwrap_or_default();
    assert!(!body.contains("engrym knowledge bases"), "block should be gone:\n{body}");

    // Removing again is a no-op.
    let v = ws.run(&["uninstall", "memory", "--agent", "claude", "--json"]).ok().json();
    assert_eq!(v["removed"], false);
}

#[test]
fn install_memory_preserves_existing_global_file_content() {
    let ws = Workspace::new();
    ws.git_init();
    let mem = ws.home().join(".claude/CLAUDE.md");
    fs::create_dir_all(mem.parent().unwrap()).unwrap();
    fs::write(&mem, "# My global instructions\n\nkeep this line\n").unwrap();

    ws.run(&["install", "memory", "--agent", "claude"]).ok();
    let body = fs::read_to_string(&mem).unwrap();
    assert!(body.contains("keep this line"), "clobbered user content:\n{body}");
    assert!(body.contains("engrym knowledge bases"));
}

#[test]
fn install_memory_unknown_agent_fails() {
    let ws = Workspace::new();
    ws.run(&["install", "memory", "--agent", "gemini"]).fail().err_has("memory");
}

// --------------------------------------------------------------------------
// reset
// --------------------------------------------------------------------------

#[test]
fn reset_deletes_docs_and_index_but_keeps_config() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["index", "--no-embed"]).ok();
    assert!(ws.repo().join(".engrym").is_dir());

    let v = ws.run(&["reset", "--yes", "--json"]).ok().json();
    assert_eq!(v["reset"], true);
    assert_eq!(v["docs_deleted"], 2);

    assert!(ws.repo().join("engrym.toml").is_file(), "config must be kept");
    assert!(ws.repo().join("docs").is_dir(), "docs root recreated empty");
    assert_eq!(fs::read_dir(ws.repo().join("docs")).unwrap().count(), 0);
    assert!(!ws.repo().join(".engrym").exists(), "index removed");
}

#[test]
fn reset_requires_confirmation_without_yes() {
    let ws = Workspace::new();
    ws.seed();
    // Non-interactive (no terminal) without --yes must refuse and keep docs.
    ws.run(&["reset"]).fail().err_has("--yes");
    assert!(ws.repo().join("docs/overview.md").is_file());
}

#[test]
fn reset_guards_against_docs_root_being_the_repo() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none"]).ok();
    // A dangerous misconfiguration: docs.root points at the repo itself.
    let cfg = ws.repo().join("engrym.toml");
    let text = fs::read_to_string(&cfg).unwrap().replace("root = \"docs\"", "root = \".\"");
    fs::write(&cfg, text).unwrap();
    ws.run(&["reset", "--yes"]).fail().err_has("refusing");
    assert!(cfg.is_file());
}

// --------------------------------------------------------------------------
// browse — local web server
// --------------------------------------------------------------------------

#[test]
fn browse_serves_rendered_docs_with_connections() {
    let ws = Workspace::new();
    ws.seed(); // overview (alt 0) ← auth (alt 1, refines:overview)
    ws.run(&["index", "--no-embed"]).ok();

    // Pick a free port, then launch the server as a detached child.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(BIN)
        .args(["browse", "--port", &port.to_string()])
        .current_dir(ws.repo())
        .env("HOME", ws.home())
        .env("ENGRYM_HOME", ws.home().join(".engrym"))
        .env("ENGRYM_NO_DAEMON", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn browse");

    let addr = format!("127.0.0.1:{port}");
    let doc = http_get(&addr, "/doc/auth", 50);
    let index = http_get(&addr, "/", 50);
    let _ = child.kill();
    let _ = child.wait();

    // Rendered body + the graph panel (auth refines overview → outbound edge).
    assert!(doc.contains("Sessions use OAuth"), "body not rendered:\n{doc}");
    assert!(doc.contains("Outbound"), "outbound connections panel missing:\n{doc}");
    assert!(doc.contains("/doc/overview"), "link to related doc missing");
    // The index lists docs.
    assert!(index.contains("Knowledge base") && index.contains("/doc/auth"), "{index}");
}

/// Minimal HTTP/1.0 GET (server closes the connection, so read to EOF), retried
/// until the server is up.
fn http_get(addr: &str, path: &str, attempts: u32) -> String {
    use std::io::{Read, Write};
    for _ in 0..attempts {
        if let Ok(mut s) = std::net::TcpStream::connect(addr) {
            let _ = s.write_all(format!("GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").as_bytes());
            let mut buf = String::new();
            if s.read_to_string(&mut buf).is_ok() && !buf.is_empty() {
                return buf;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    String::new()
}

// --------------------------------------------------------------------------
// deinit — full removal (inverse of init)
// --------------------------------------------------------------------------

#[test]
fn deinit_removes_the_whole_in_repo_footprint() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "claude", "--json"]).ok(); // config + skills + memory
    ws.new_doc("a", 0, "core", &[], "# A").ok();
    ws.run(&["index", "--no-embed"]).ok();
    // Sanity: everything is present.
    assert!(ws.repo().join("engrym.toml").is_file());
    assert!(ws.repo().join(".claude/skills/engrym/SKILL.md").is_file());
    assert!(ws.repo().join(".engrym").is_dir());

    let v = ws.run(&["deinit", "--yes", "--json"]).ok().json();
    assert_eq!(v["deinitialized"], true);

    // Per-repo footprint gone.
    assert!(!ws.repo().join("engrym.toml").exists());
    assert!(!ws.repo().join("docs").exists());
    assert!(!ws.repo().join(".engrym").exists());
    assert!(!ws.repo().join(".claude/skills/engrym").exists());
    // .gitignore no longer mentions the index.
    let gi = fs::read_to_string(ws.repo().join(".gitignore")).unwrap_or_default();
    assert!(!gi.contains(".engrym/"), "gitignore still has entry: {gi}");
    // Global memory entry removed.
    let mem = fs::read_to_string(ws.home().join(".claude/CLAUDE.md")).unwrap_or_default();
    assert!(!mem.contains("engrym knowledge bases"), "memory entry left: {mem}");
}

#[test]
fn deinit_removes_local_store_and_leaves_shared_skills() {
    let ws = Workspace::new();
    ws.git_init();
    ws.run(&["init", "--local", "--agent", "claude", "--json"]).ok();
    let store_proj = only_subdir(&ws.store());
    assert!(store_proj.join("engrym.toml").is_file());
    let global_skill = ws.home().join(".claude/skills/engrym/SKILL.md");
    assert!(global_skill.is_file());

    ws.run(&["deinit", "--yes"]).ok();

    // External store gone; memory entry gone.
    assert!(!store_proj.exists(), "local store should be removed");
    let mem = fs::read_to_string(ws.home().join(".claude/CLAUDE.md")).unwrap_or_default();
    assert!(!mem.contains("engrym knowledge bases"));
    // But user-global skills are shared across repos — must NOT be removed.
    assert!(global_skill.is_file(), "shared user-global skill must survive deinit");
}

#[test]
fn deinit_on_a_clean_repo_is_a_noop() {
    let ws = Workspace::new();
    let v = ws.run(&["deinit", "--yes", "--json"]).ok().json();
    assert_eq!(v["deinitialized"], false);
}

#[test]
fn deinit_requires_confirmation_without_yes() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["deinit"]).fail().err_has("--yes");
    assert!(ws.repo().join("engrym.toml").is_file());
}

// --------------------------------------------------------------------------
// index / search / graph navigation
// --------------------------------------------------------------------------

#[test]
fn index_reports_document_and_chunk_counts() {
    let ws = Workspace::new();
    ws.seed();
    let v = ws.run(&["index", "--no-embed", "--json"]).ok().json();
    assert_eq!(v["indexed"], 2);
    assert_eq!(v["embeddings"], false);
    assert_eq!(v["local"], false);
}

#[test]
fn keyword_search_finds_the_right_passage() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["index", "--no-embed"]).ok();
    let v = ws.run(&["search", "OAuth token", "--keyword", "--json"]).ok().json();
    let hits = v.as_array().unwrap();
    assert!(hits.iter().any(|h| h["id"] == "auth"), "expected auth hit: {v}");
}

#[test]
fn topic_lists_the_subtree() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["index", "--no-embed"]).ok();
    // `core` is a prefix of both `core` and `core/auth`.
    let v = ws.run(&["topic", "core", "--json"]).ok().json();
    let ids: Vec<&str> = v.as_array().unwrap().iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&"overview") && ids.contains(&"auth"), "{v}");
}

#[test]
fn related_shows_the_graph_neighborhood() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["index", "--no-embed"]).ok();
    // auth --refines--> overview, so overview is in auth's neighborhood.
    ws.run(&["related", "auth", "--json"]).ok().has("overview");
}

#[test]
fn show_prints_the_document() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["index", "--no-embed"]).ok();
    ws.run(&["show", "overview", "--json"]).ok().has("overview");
    ws.run(&["show", "overview"]).ok().has("entry point");
}

// --------------------------------------------------------------------------
// lint
// --------------------------------------------------------------------------

#[test]
fn lint_passes_on_a_valid_kb() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["lint", "--strict"]).ok();
}

#[test]
fn lint_strict_fails_on_a_dangling_relation() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none"]).ok();
    ws.new_doc("a", 1, "core", &["depends_on:ghost"], "# A").ok();
    ws.run(&["lint", "--strict"]).fail();
    // Fixing the dangling target makes it pass.
    ws.new_doc("ghost", 2, "core", &[], "# Ghost").ok();
    ws.run(&["lint", "--strict"]).ok();
}

// --------------------------------------------------------------------------
// authoring: set / rm / relocate
// --------------------------------------------------------------------------

#[test]
fn set_adds_a_relation() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["set", "auth", "--add-relation", "depends_on:overview"]).ok();
    ws.run(&["index", "--no-embed"]).ok();
    ws.run(&["related", "auth", "--json"]).ok().has("depends_on");
}

#[test]
fn rm_refuses_when_referenced_then_succeeds_with_force() {
    let ws = Workspace::new();
    ws.seed(); // auth --refines--> overview
    ws.run(&["rm", "overview"]).fail();
    assert!(ws.repo().join("docs/overview.md").is_file());
    ws.run(&["rm", "overview", "--force"]).ok();
    assert!(!ws.repo().join("docs/overview.md").exists());
}

#[test]
fn relocate_moves_files_between_layouts() {
    let ws = Workspace::new();
    ws.seed(); // default altitude layout: auth (alt 1) → docs/1/auth.md
    assert!(ws.repo().join("docs/1/auth.md").is_file());

    let v = ws.run(&["relocate", "--layout", "flat", "--json"]).ok().json();
    assert!(v["count"].as_u64().unwrap() >= 1);

    // Flat layout: docs/<id>.md, and the altitude subdir is gone.
    assert!(ws.repo().join("docs/auth.md").is_file());
    assert!(!ws.repo().join("docs/1/auth.md").exists());
}

// --------------------------------------------------------------------------
// where / list / link — discovery and cross-clone linking
// --------------------------------------------------------------------------

#[test]
fn where_reports_an_in_repo_kb() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "none"]).ok();
    let v = ws.run(&["where", "--json"]).ok().json();
    assert_eq!(v["kb"], true);
    assert_eq!(v["mode"], "in-repo");
}

#[test]
fn where_is_a_nonzero_gate_when_no_kb() {
    let ws = Workspace::new();
    ws.git_init();
    let out = ws.run(&["where", "--json"]);
    out.fail(); // exit code gates the skill
    assert_eq!(out.json()["kb"], false);
}

#[test]
fn link_shares_a_local_kb_across_clones_of_the_same_repo() {
    let ws = Workspace::new();
    let root = tempdir();
    let a = root.path().join("clone-a");
    let b = root.path().join("clone-b");
    let origin = "git@github.com:acme/widget.git";
    fake_clone(&a, origin);
    fake_clone(&b, origin); // separate clone, same remote

    // A local KB for clone A.
    ws.run_in(&a, &["init", "--local", "--agent", "none", "--json"]).ok();
    assert_eq!(ws.run_in(&a, &["where", "--json"]).ok().json()["kb"], true);

    // Clone B has no KB of its own, but sees A's as a link candidate.
    let wb = ws.run_in(&b, &["where", "--json"]);
    wb.fail();
    let vb = wb.json();
    assert_eq!(vb["kb"], false);
    assert!(vb["link_candidate"].is_string(), "expected a link candidate: {vb}");

    // Link B → A's KB; now it resolves and is marked shared.
    ws.run_in(&b, &["link", a.to_str().unwrap(), "--json"]).ok();
    let wb2 = ws.run_in(&b, &["where", "--json"]).ok().json();
    assert_eq!(wb2["kb"], true);
    assert_eq!(wb2["shared"], true);

    // One store on disk, now with both anchors registered.
    let stores = ws.run(&["list", "--json"]).ok().json();
    let stores = stores["stores"].as_array().unwrap().clone();
    assert_eq!(stores.len(), 1);
    assert_eq!(stores[0]["anchors"].as_array().unwrap().len(), 2);

    // Unlink B → back to no KB there; A untouched.
    ws.run_in(&b, &["unlink", "--json"]).ok();
    ws.run_in(&b, &["where"]).fail();
    assert_eq!(ws.run_in(&a, &["where", "--json"]).ok().json()["kb"], true);
}

#[test]
fn install_skills_refresh_updates_a_stale_installed_skill() {
    let ws = Workspace::new();
    ws.run(&["init", "--agent", "claude", "--json"]).ok();
    let skill = ws.repo().join(".claude/skills/engrym/SKILL.md");
    assert!(skill.is_file());

    // Freshly installed → current.
    assert_eq!(ws.run(&["where", "--json"]).ok().json()["skill_outdated"], false);

    // Simulate an older install by rewriting the version stamp.
    let text = fs::read_to_string(&skill).unwrap();
    let stale = text.replacen(
        &format!("engrym-skill-version: {}", env!("CARGO_PKG_VERSION")),
        "engrym-skill-version: 0.0.1",
        1,
    );
    assert_ne!(text, stale, "expected a version stamp to rewrite");
    fs::write(&skill, stale).unwrap();
    assert_eq!(ws.run(&["where", "--json"]).ok().json()["skill_outdated"], true);

    // Refresh brings every installed location back to current.
    let v = ws.run(&["install", "skills", "--refresh", "--json"]).ok().json();
    assert!(!v["refreshed"].as_array().unwrap().is_empty());
    assert_eq!(ws.run(&["where", "--json"]).ok().json()["skill_outdated"], false);
}

#[test]
fn registry_backfills_from_disk_on_first_use() {
    let ws = Workspace::new();
    let root = tempdir();
    let a = root.path().join("a");
    let b = root.path().join("b");
    fake_clone(&a, "git@github.com:acme/a.git");
    fake_clone(&b, "git@github.com:acme/b.git");
    ws.run_in(&a, &["init", "--local", "--agent", "none", "--json"]).ok();
    ws.run_in(&b, &["init", "--local", "--agent", "none", "--json"]).ok();

    // Simulate first use after upgrade: the registry file doesn't exist yet.
    let reg = ws.home().join(".engrym/registry.json");
    assert!(reg.is_file());
    fs::remove_file(&reg).unwrap();

    // Using engrym in ONE repo rebuilds the whole registry from the stores on
    // disk — including the sibling store we never visited this run.
    ws.run_in(&a, &["where"]).ok();
    let text = fs::read_to_string(&reg).unwrap();
    assert!(text.contains("acme/a"), "current repo re-registered:\n{text}");
    assert!(text.contains("acme/b"), "sibling store backfilled from disk:\n{text}");
}

// --------------------------------------------------------------------------
// workspaces — several repos side by side under one folder
// --------------------------------------------------------------------------

/// A folder of clones: `<master>/{api,web}`, each an in-repo KB with one doc,
/// both indexed. `auth-overview` deliberately exists in both, to exercise the
/// ambiguity path.
fn seed_master(ws: &Workspace) -> TempDir {
    let master = tempdir();
    for (repo, id, title, body) in [
        ("api", "auth-overview", "Auth overview", "Sessions use OAuth token refresh in the API."),
        ("web", "session-handling", "Session handling", "The frontend keeps the OAuth token in memory."),
        ("web", "auth-overview", "Web auth overview", "The web app's own take on auth."),
    ] {
        let dir = master.path().join(repo);
        if !dir.exists() {
            fs::create_dir_all(dir.join(".git")).unwrap();
            ws.run_in(&dir, &["init", "--agent", "none"]).ok();
        }
        ws.run_in(
            &dir,
            &["new", id, "--title", title, "-a", "1", "--topic", "core/auth", "--body", body],
        )
        .ok();
    }
    master
}

#[test]
fn index_and_search_span_every_repo_under_the_folder() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();

    // One `index` from the parent folder brings every child repo current.
    let indexed = ws.run_in(root, &["index", "--no-embed", "--json"]).ok().json();
    let repos: Vec<&str> =
        indexed["results"].as_array().unwrap().iter().map(|r| r["repo"].as_str().unwrap()).collect();
    assert_eq!(repos, vec!["api", "web"]);

    // Searching from the parent fans out and groups by the repo that answered.
    let v = ws.run_in(root, &["search", "OAuth token", "--keyword", "--json"]).ok().json();
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "both repos should answer: {v}");
    let names: Vec<&str> = groups.iter().map(|g| g["repo"].as_str().unwrap()).collect();
    assert!(names.contains(&"api") && names.contains(&"web"), "got {names:?}");
    // Every hit carries a ref that's directly usable as `engrym show <ref>`.
    let first = &groups[0]["hits"][0];
    assert_eq!(
        first["ref"].as_str().unwrap(),
        format!("{}:{}", groups[0]["repo"].as_str().unwrap(), first["id"].as_str().unwrap())
    );
    assert_eq!(v["workspace"]["repos"].as_array().unwrap().len(), 2);
}

#[test]
fn a_repo_with_its_own_kb_searches_alone_until_all_is_asked_for() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let api = master.path().join("api");
    ws.run_in(master.path(), &["index", "--no-embed"]).ok();

    // Inside a repo, nothing changes: the flat, single-KB result shape.
    let solo = ws.run_in(&api, &["search", "OAuth token", "--keyword", "--json"]).ok().json();
    assert!(solo.is_array(), "single-repo search must stay an array: {solo}");
    assert_eq!(solo.as_array().unwrap().len(), 1);

    // `--all` opts into the siblings.
    let all = ws.run_in(&api, &["search", "--all", "OAuth token", "--keyword", "--json"]).ok().json();
    assert_eq!(all["groups"].as_array().unwrap().len(), 2, "{all}");
}

#[test]
fn documents_in_other_repos_are_addressed_by_a_repo_qualifier() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();
    ws.run_in(root, &["index", "--no-embed"]).ok();

    // Unique across the workspace → the bare id is enough.
    let v = ws.run_in(root, &["show", "session-handling", "--json"]).ok().json();
    assert_eq!(v["repo"], "web");

    // Defined in both repos → we ask rather than guess.
    ws.run_in(root, &["show", "auth-overview"])
        .fail()
        .err_has("exists in 2 repos");

    // The qualifier picks one outright, for `show` and `related` alike.
    assert_eq!(ws.run_in(root, &["show", "api:auth-overview", "--json"]).ok().json()["title"], "Auth overview");
    assert_eq!(ws.run_in(root, &["related", "web:auth-overview", "--json"]).ok().json()["title"], "Web auth overview");
    ws.run_in(root, &["show", "nope:auth-overview"]).fail().err_has("no repo `nope`");
}

#[test]
fn topic_groups_the_subtree_by_repo() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();
    ws.run_in(root, &["index", "--no-embed"]).ok();

    let v = ws.run_in(root, &["topic", "core/auth", "--json"]).ok().json();
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let web = groups.iter().find(|g| g["repo"] == "web").expect("web group");
    assert_eq!(web["docs"].as_array().unwrap().len(), 2);
}

#[test]
fn where_and_list_report_the_workspace() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();

    // `where` is a "yes" from the parent folder — engrym does apply here.
    let w = ws.run_in(root, &["where", "--json"]).ok().json();
    assert_eq!(w["kb"], true);
    assert_eq!(w["mode"], "workspace");
    assert_eq!(w["repos"].as_array().unwrap().len(), 2);

    let l = ws.run_in(root, &["list", "--json"]).ok().json();
    assert_eq!(l["workspace"]["repos"].as_array().unwrap().len(), 2);
}

#[test]
fn commands_that_touch_one_kb_refuse_an_ambiguous_workspace() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();
    ws.run_in(root, &["index", "--no-embed"]).ok();

    for args in [
        vec!["lint"],
        vec!["reset", "--yes"],
        vec!["rm", "auth-overview", "--force"],
    ] {
        ws.run_in(root, &args).fail().err_has("works on one at a time");
    }
    // `--repo` still targets one of them.
    ws.run_in(root, &["lint", "--repo", master.path().join("api").to_str().unwrap()]).ok();
}

#[test]
fn a_missing_index_in_one_repo_does_not_sink_the_search() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();
    // Only `api` gets indexed; `web` has a KB but no index yet.
    ws.run_in(&master.path().join("api"), &["index", "--no-embed"]).ok();

    let out = ws.run_in(root, &["search", "OAuth token", "--keyword", "--json"]);
    out.ok().err_has("web skipped");
    assert_eq!(out.json()["groups"].as_array().unwrap().len(), 1);
}

#[test]
fn a_subdirectory_of_a_repo_is_never_a_workspace_member() {
    let ws = Workspace::new();
    ws.seed();
    ws.run(&["index", "--no-embed"]).ok();
    // `docs/` sits under a KB but carries none of its own, so searching from the
    // repo root still resolves to exactly one KB — not a two-member workspace.
    let v = ws.run(&["search", "OAuth", "--keyword", "--json"]).ok().json();
    assert!(v.is_array(), "expected the single-KB shape, got {v}");
}

/// `<root>/org-a/{api,web}` plus a flat `<root>/solo` — the nested grouping a
/// deeper scan exists for.
fn seed_org_folders(ws: &Workspace) -> TempDir {
    let root = tempdir();
    for path in ["org-a/api", "org-a/web", "org-b/api", "solo"] {
        let dir = root.path().join(path);
        fs::create_dir_all(dir.join(".git")).unwrap();
        ws.run_in(&dir, &["init", "--agent", "none"]).ok();
        ws.run_in(
            &dir,
            &["new", "svc-doc", "--title", "Service doc", "-a", "1", "--topic", "core",
              "--body", &format!("OAuth token handling in {path}.")],
        )
        .ok();
    }
    root
}

#[test]
fn depth_reaches_repos_grouped_under_an_org_folder() {
    let ws = Workspace::new();
    let root_dir = seed_org_folders(&ws);
    let root = root_dir.path();

    // One level down only sees the flat clone; the org folders are too deep.
    let shallow = ws.run_in(root, &["where", "--json"]).ok().json();
    assert_eq!(shallow["repos"].as_array().unwrap().len(), 1);
    assert_eq!(shallow["repos"][0]["name"], "solo");

    // `--depth 2` reaches them, named by their path so two orgs' `api` don't collide.
    let deep = ws.run_in(root, &["where", "--depth", "2", "--json"]).ok().json();
    let names: Vec<&str> =
        deep["repos"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["org-a/api", "org-a/web", "org-b/api", "solo"]);
    assert_eq!(deep["depth"], 2);

    // And the whole read surface follows.
    ws.run_in(root, &["index", "--no-embed", "--depth", "2"]).ok();
    let hits = ws
        .run_in(root, &["search", "OAuth token", "--keyword", "--depth", "2", "--json"])
        .ok()
        .json();
    assert_eq!(hits["groups"].as_array().unwrap().len(), 4);
    assert_eq!(
        ws.run_in(root, &["show", "org-b/api:svc-doc", "--depth", "2", "--json"]).ok().json()["repo"],
        "org-b/api"
    );

    // Without the reach, the qualifier can't resolve — and says why.
    ws.run_in(root, &["show", "org-b/api:svc-doc"]).fail().err_has("try `--depth 2`");
}

#[test]
fn a_deeper_scan_still_never_descends_into_a_repo() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();
    // A vendored checkout with its own KB, buried inside `api`.
    let vendored = root.join("api/vendor/lib");
    fs::create_dir_all(vendored.join(".git")).unwrap();
    ws.run_in(&vendored, &["init", "--agent", "none"]).ok();

    // Even at full reach, `api` is a repo — the walk stops there, so the
    // vendored KB is not a sibling of anything.
    let v = ws.run_in(root, &["where", "--depth", "5", "--json"]).ok().json();
    let names: Vec<&str> =
        v["repos"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["api", "web"]);
}

#[test]
fn depth_widens_downward_from_a_root_we_never_guess() {
    let ws = Workspace::new();
    let root_dir = seed_org_folders(&ws);
    let root = root_dir.path();
    ws.run_in(root, &["index", "--no-embed", "--depth", "2"]).ok();
    let api = root.join("org-a/api");

    // Setting a depth implies `--all`, so this fans out with no extra flag. The
    // root is the one folder holding the repo — we never walk *up* to guess a
    // wider one — so it reaches org-a's repos, not org-b's.
    let v = ws
        .run_in(&api, &["search", "OAuth token", "--keyword", "--depth", "2", "--json"])
        .ok()
        .json();
    let names: Vec<&str> =
        v["groups"].as_array().unwrap().iter().map(|g| g["repo"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["api", "web"], "{v}");

    // A wider root is asked for explicitly, never inferred.
    let all = ws
        .run_in(
            &api,
            &["search", "OAuth token", "--keyword", "--repo", root.to_str().unwrap(), "--depth", "2", "--json"],
        )
        .ok()
        .json();
    assert_eq!(all["groups"].as_array().unwrap().len(), 4, "{all}");
}

#[test]
fn a_repo_without_a_kb_falls_back_to_its_siblings() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let root = master.path();
    ws.run_in(root, &["index", "--no-embed"]).ok();

    // A plain checkout beside the others, with no engrym of its own. Its own
    // tree holds nothing, so the folder holding *it* is what we meant.
    let legacy = root.join("legacy");
    fs::create_dir_all(legacy.join(".git")).unwrap();

    let v = ws.run_in(&legacy, &["search", "OAuth token", "--keyword", "--json"]).ok().json();
    assert_eq!(v["groups"].as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v["workspace"]["depth"], 1, "the fallback doesn't widen the depth");

    // Same from deep inside that repo — the enclosing checkout is what anchors it.
    let deep = legacy.join("src/lib");
    fs::create_dir_all(&deep).unwrap();
    let w = ws.run_in(&deep, &["where", "--json"]).ok().json();
    assert_eq!(w["mode"], "workspace");
    assert_eq!(w["repos"].as_array().unwrap().len(), 2);

    // A lone repo with no engrym and no siblings is still a plain "no KB".
    let empty = tempdir();
    let alone = empty.path().join("alone");
    fs::create_dir_all(alone.join(".git")).unwrap();
    ws.run_in(&alone, &["where"]).fail();
}

#[test]
fn where_answers_for_the_same_scope_the_other_commands_resolve() {
    let ws = Workspace::new();
    let master = seed_master(&ws);
    let api = master.path().join("api");

    // Plain: this repo's own KB.
    assert_eq!(ws.run_in(&api, &["where", "--json"]).ok().json()["mode"], "in-repo");

    // `--all` changes what `search` resolves, so the gate must agree — reporting
    // one in-repo KB while `search --all` spans the siblings is a lie.
    let w = ws.run_in(&api, &["where", "--all", "--json"]).ok().json();
    assert_eq!(w["mode"], "workspace");
    assert_eq!(w["repos"].as_array().unwrap().len(), 2, "{w}");
}

#[test]
fn nested_checkouts_are_members_of_the_repo_that_contains_them() {
    let ws = Workspace::new();
    let work = tempdir();
    // /work/repoA (a checkout, no KB) containing repoB + repoC (both with KBs),
    // plus a sibling of repoA one level up.
    let repo_a = work.path().join("repoA");
    fs::create_dir_all(repo_a.join(".git")).unwrap();
    for nested in ["repoB", "repoC"] {
        let dir = repo_a.join(nested);
        fs::create_dir_all(dir.join(".git")).unwrap();
        ws.run_in(&dir, &["init", "--agent", "none"]).ok();
    }
    let sibling = work.path().join("sibling");
    fs::create_dir_all(sibling.join(".git")).unwrap();
    ws.run_in(&sibling, &["init", "--agent", "none"]).ok();

    // From repoA: the nested checkouts are members. Pruning only stops us
    // descending *into* a checkout — it never stops us adding one.
    let inside = ws.run_in(&repo_a, &["where", "--json"]).ok().json();
    let names: Vec<&str> =
        inside["repos"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["repoB", "repoC"]);

    // From /work: repoA is a checkout, so we never descend into it — the nested
    // repos stay invisible at *any* depth. Only repoA's sibling shows up.
    for depth in ["1", "5"] {
        let above = ws.run_in(work.path(), &["where", "--depth", depth, "--json"]).ok().json();
        let names: Vec<&str> =
            above["repos"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["sibling"], "at depth {depth}");
    }
}
