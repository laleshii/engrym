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
