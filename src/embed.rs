//! Local, offline embeddings via `fastembed` (bundled ONNX models).
//!
//! The model loads lazily — only commands that actually need vectors pay the
//! few-hundred-ms load (and the one-time model download). Non-semantic commands
//! never touch this module. Passages and queries are embedded into the same
//! space; per BGE's retrieval recipe the *query* gets an instruction prefix
//! while passages are embedded verbatim.

use crate::vector;
use anyhow::{anyhow, Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::PathBuf;

/// BGE v1.5 retrieval instruction, applied to queries only.
const QUERY_INSTRUCTION: &str = "Represent this sentence for searching relevant passages: ";

pub struct Embedder {
    model: TextEmbedding,
    dim: usize,
}

impl Embedder {
    /// Load (downloading + caching the model on first use). `show_progress`
    /// controls the download progress bar on stderr.
    pub fn load(model_name: &str, show_progress: bool) -> Result<Embedder> {
        let model = map_model(model_name)?;
        let dim = TextEmbedding::get_model_info(&model)
            .map(|info| info.dim)
            .with_context(|| format!("looking up model info for `{}`", model_name))?;
        let model = TextEmbedding::try_new(
            InitOptions::new(model)
                .with_show_download_progress(show_progress)
                // Cache models in one global location so the binary can be
                // pointed at any repo without downloading per-repo or polluting
                // the target with a `.fastembed_cache/`.
                .with_cache_dir(model_cache_dir()),
        )
        .with_context(|| format!("initializing embedding model `{}`", model_name))?;
        Ok(Embedder { model, dim })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Embed passages verbatim, normalized for cosine-as-dot-product.
    pub fn embed_passages(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut vecs = self.model.embed(texts, None).context("embedding passages")?;
        for v in vecs.iter_mut() {
            vector::normalize(v);
        }
        Ok(vecs)
    }

    /// Embed a single query (with the retrieval instruction prefix), normalized.
    pub fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
        let text = format!("{}{}", QUERY_INSTRUCTION, query);
        let mut v = self
            .model
            .embed(&[text], None)
            .context("embedding query")?
            .pop()
            .ok_or_else(|| anyhow!("embedding model returned no vector"))?;
        vector::normalize(&mut v);
        Ok(v)
    }
}

/// A single, user-global model cache: `$XDG_CACHE_HOME/engrym/models` (falling
/// back to `$HOME/.cache/engrym/models`, then a local dir as a last resort).
fn model_cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".engrym-cache"));
    base.join("engrym").join("models")
}

/// Map an `engrym.toml` model string to a fastembed model. Kept explicit so the
/// supported set is obvious and errors are actionable.
fn map_model(name: &str) -> Result<EmbeddingModel> {
    match name.trim().to_ascii_lowercase().as_str() {
        "bge-small-en-v1.5" | "bgesmallenv15" => Ok(EmbeddingModel::BGESmallENV15),
        other => Err(anyhow!(
            "unsupported local embedding model `{}` \
             (supported: bge-small-en-v1.5)",
            other
        )),
    }
}
