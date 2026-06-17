//! The warm embedding daemon.
//!
//! Loading the ONNX model costs ~120ms per process; the cosine math itself is
//! microseconds. The daemon pays that load once and stays resident, so repeated
//! semantic queries (e.g. an agent hammering `engrym search`) drop from ~130ms
//! to single-digit ms. It is auto-spawned by `search` on the first semantic
//! query and exits itself after `idle_secs` of inactivity.
//!
//! Scope is deliberately tiny: the daemon only answers "embed this query string
//! → vector". It never touches the index, so `engrym index` rebuilds never
//! invalidate it. The client does BM25 + cosine + fusion against its own DB.
//!
//! Anything that goes wrong on the daemon path falls back to in-process
//! embedding — a search never fails because the daemon misbehaved.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Request {
    op: String, // "embed" | "shutdown" | "ping"
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Response {
    ok: bool,
    #[serde(default)]
    embedding: Option<Vec<f32>>,
    #[serde(default)]
    error: Option<String>,
}

#[cfg(unix)]
pub use imp::{query_embedding, serve, stop};

// Non-unix: no unix sockets. Callers fall back to in-process embedding.
#[cfg(not(unix))]
mod stub {
    use crate::config::Config;
    use anyhow::{bail, Result};

    pub fn query_embedding(_config: &Config, _query: &str) -> Result<Vec<f32>> {
        bail!("daemon not supported on this platform")
    }
    pub fn serve(_config: &Config, _idle_secs: u64) -> Result<()> {
        bail!("daemon not supported on this platform")
    }
    pub fn stop(_config: &Config) -> Result<bool> {
        Ok(false)
    }
}
#[cfg(not(unix))]
pub use stub::{query_embedding, serve, stop};

#[cfg(unix)]
mod imp {
    use super::{Request, Response};
    use crate::config::Config;
    use crate::embed::Embedder;
    use anyhow::{anyhow, bail, Context, Result};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// How long the client waits for a freshly-spawned daemon to come up. The
    /// model is already cached by the time vectors exist, so load is ~120ms;
    /// this is generous headroom before falling back to in-process.
    const SPAWN_WAIT: Duration = Duration::from_secs(10);
    const READ_TIMEOUT: Duration = Duration::from_secs(20);

    /// Get a query embedding via the daemon, spawning it if necessary. Returns
    /// `Err` only when the daemon is genuinely unavailable — the caller then
    /// embeds in-process.
    pub fn query_embedding(config: &Config, query: &str) -> Result<Vec<f32>> {
        let path = config.socket_path();
        let model = &config.embedding.model;

        // Fast path: a daemon is already warm.
        if let Ok(v) = embed_request(&path, model, query) {
            return Ok(v);
        }

        // Spawn detached and wait for it to bind.
        spawn_detached(config)?;
        let deadline = Instant::now() + SPAWN_WAIT;
        loop {
            if let Ok(v) = embed_request(&path, model, query) {
                return Ok(v);
            }
            if Instant::now() >= deadline {
                bail!("daemon did not become ready within {:?}", SPAWN_WAIT);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    /// Tell a running daemon to exit. Returns whether one was stopped.
    pub fn stop(config: &Config) -> Result<bool> {
        let path = config.socket_path();
        if !path.exists() {
            return Ok(false);
        }
        match request(&path, &Request { op: "shutdown".into(), model: None, query: None }) {
            Ok(resp) if resp.ok => Ok(true),
            _ => {
                // No live daemon answered; clean up a stale socket.
                std::fs::remove_file(&path).ok();
                Ok(false)
            }
        }
    }

    /// Run the daemon loop (foreground). Used both as the spawned child and as
    /// the user-facing `engrym serve`.
    pub fn serve(config: &Config, idle_secs: u64) -> Result<()> {
        let path = config.socket_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let listener = match bind(&path)? {
            Some(l) => l,
            None => {
                // Another live daemon already owns the socket; nothing to do.
                return Ok(());
            }
        };

        let mut embedder = Embedder::load(&config.embedding.model, false)
            .context("loading embedding model for daemon")?;
        let model = config.embedding.model.clone();

        // Watchdog: self-terminate after idle_secs of no requests.
        let last = Arc::new(Mutex::new(Instant::now()));
        spawn_watchdog(last.clone(), idle_secs, path.clone());

        for conn in listener.incoming() {
            let stream = match conn {
                Ok(s) => s,
                Err(_) => continue,
            };
            *last.lock().unwrap() = Instant::now();
            if handle(stream, &mut embedder, &model) == Outcome::Shutdown {
                break;
            }
        }

        std::fs::remove_file(&path).ok();
        Ok(())
    }

    #[derive(PartialEq)]
    enum Outcome {
        Continue,
        Shutdown,
    }

    fn handle(stream: UnixStream, embedder: &mut Embedder, model: &str) -> Outcome {
        stream.set_read_timeout(Some(READ_TIMEOUT)).ok();
        let mut writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => return Outcome::Continue,
        };
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            return Outcome::Continue;
        }

        let req: Request = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                respond_err(&mut writer, &format!("bad request: {}", e));
                return Outcome::Continue;
            }
        };

        match req.op.as_str() {
            "ping" => {
                respond(&mut writer, &Response { ok: true, embedding: None, error: None });
                Outcome::Continue
            }
            "shutdown" => {
                respond(&mut writer, &Response { ok: true, embedding: None, error: None });
                Outcome::Shutdown
            }
            "embed" => {
                // A model mismatch means config changed under us; tell the
                // client so it can fall back, and exit so the next spawn loads
                // the right model.
                if req.model.as_deref() != Some(model) {
                    respond_err(&mut writer, "model mismatch");
                    return Outcome::Shutdown;
                }
                let query = req.query.unwrap_or_default();
                match embedder.embed_query(&query) {
                    Ok(v) => respond(
                        &mut writer,
                        &Response { ok: true, embedding: Some(v), error: None },
                    ),
                    Err(e) => respond_err(&mut writer, &format!("{:#}", e)),
                }
                Outcome::Continue
            }
            other => {
                respond_err(&mut writer, &format!("unknown op `{}`", other));
                Outcome::Continue
            }
        }
    }

    fn respond(writer: &mut impl Write, resp: &Response) {
        if let Ok(mut s) = serde_json::to_string(resp) {
            s.push('\n');
            let _ = writer.write_all(s.as_bytes());
            let _ = writer.flush();
        }
    }

    fn respond_err(writer: &mut impl Write, msg: &str) {
        respond(
            writer,
            &Response { ok: false, embedding: None, error: Some(msg.to_string()) },
        );
    }

    /// Bind the socket, handling stale leftovers. `Ok(None)` means a *live*
    /// daemon already owns it (so the caller should bow out).
    fn bind(path: &Path) -> Result<Option<UnixListener>> {
        match UnixListener::bind(path) {
            Ok(l) => Ok(Some(l)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if UnixStream::connect(path).is_ok() {
                    return Ok(None); // a live daemon is already serving
                }
                // Stale socket from a crashed daemon: remove and rebind.
                std::fs::remove_file(path).ok();
                Ok(Some(UnixListener::bind(path).with_context(|| {
                    format!("rebinding daemon socket {}", path.display())
                })?))
            }
            Err(e) => Err(e).with_context(|| format!("binding daemon socket {}", path.display())),
        }
    }

    fn spawn_watchdog(last: Arc<Mutex<Instant>>, idle_secs: u64, path: std::path::PathBuf) {
        let idle = Duration::from_secs(idle_secs);
        let tick = Duration::from_secs((idle_secs / 4).clamp(1, 30));
        std::thread::spawn(move || loop {
            std::thread::sleep(tick);
            let elapsed = last.lock().map(|t| t.elapsed()).unwrap_or_default();
            if elapsed >= idle {
                std::fs::remove_file(&path).ok();
                std::process::exit(0);
            }
        });
    }

    /// Spawn `engrym serve` as a detached background process (new session via
    /// setsid, stdio to /dev/null) so it outlives the invoking CLI.
    fn spawn_detached(config: &Config) -> Result<()> {
        let exe = std::env::current_exe().context("locating engrym binary")?;
        let mut cmd = Command::new(exe);
        cmd.arg("serve")
            .arg("--repo")
            .arg(&config.repo_root)
            .arg("--idle")
            .arg(config.daemon.idle_secs.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: setsid is async-signal-safe and we touch nothing else in the
        // child between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        cmd.spawn().context("spawning daemon")?;
        Ok(())
    }

    fn embed_request(path: &Path, model: &str, query: &str) -> Result<Vec<f32>> {
        let resp = request(
            path,
            &Request {
                op: "embed".into(),
                model: Some(model.to_string()),
                query: Some(query.to_string()),
            },
        )?;
        if resp.ok {
            resp.embedding.ok_or_else(|| anyhow!("daemon returned no embedding"))
        } else {
            bail!("daemon error: {}", resp.error.unwrap_or_default())
        }
    }

    fn request(path: &Path, req: &Request) -> Result<Response> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(READ_TIMEOUT)).ok();
        let mut writer = stream.try_clone()?;
        let mut payload = serde_json::to_string(req)?;
        payload.push('\n');
        writer.write_all(payload.as_bytes())?;
        writer.flush()?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let resp: Response = serde_json::from_str(line.trim())
            .with_context(|| "parsing daemon response")?;
        Ok(resp)
    }
}
