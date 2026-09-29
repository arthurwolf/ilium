//! Headless, offline release qualification using the installed client's real
//! locked FastEmbed/ONNX CPU backend. No session, config, audio, or cache opens.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use serde::Serialize;

const MODEL_FILE_LIMIT: u64 = 134_217_728;
const NATIVE_AUDIT_ACKNOWLEDGEMENT: &str = "native-audit-observed\n";

#[derive(Serialize)]
struct EmbeddingVector<'a> {
    #[serde(rename = "type")]
    record_type: &'static str,
    ilium_pid: u32,
    executable_path: std::path::PathBuf,
    input: &'a str,
    embedding: Vec<f32>,
}

fn read_model_file(directory: &Path, name: &str) -> Result<Vec<u8>> {
    let path = directory.join(name);
    let metadata = std::fs::symlink_metadata(&path)
        .with_context(|| format!("could not inspect embedding model file {}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MODEL_FILE_LIMIT {
        bail!(
            "embedding model file {} must be a bounded regular file",
            path.display()
        );
    }
    std::fs::read(&path)
        .with_context(|| format!("could not read embedding model file {}", path.display()))
}

fn validate_vector(vector: &[f32]) -> Result<()> {
    if vector.len() != 384
        || vector.iter().any(|value| !value.is_finite())
        || !vector.iter().any(|value| *value != 0.0)
    {
        bail!("the actual MiniLM model must return a finite nonzero 384-dimensional vector");
    }
    Ok(())
}

fn wait_for_native_audit(reader: impl BufRead) -> Result<()> {
    // Bounded input accepts EOF as cancellation so an auditor failure cannot
    // orphan this process. Keep the inference session alive until this returns.
    let mut reader = reader.take(128);
    let mut acknowledgement = String::new();
    reader.read_line(&mut acknowledgement)?;
    if acknowledgement.is_empty() || acknowledgement == NATIVE_AUDIT_ACKNOWLEDGEMENT {
        return Ok(());
    }
    bail!("invalid native audit acknowledgement");
}

/// Emits one real inference vector and optionally holds this same executable
/// and ONNX session until the auditor acknowledges its independent native map.
/// The external reviewed wrapper validates hashes and binds this PID to the
/// installed binary; native mapping is deliberately not claimed by this probe.
pub fn probe(model_directory: &Path, text: &str, hold_for_native_audit: bool) -> Result<()> {
    if !model_directory.is_absolute() || text.trim().is_empty() || text.chars().count() > 8192 {
        bail!(
            "embedding probe requires an absolute local model directory and bounded nonempty text"
        );
    }
    let metadata = std::fs::symlink_metadata(model_directory)?;
    if !metadata.is_dir() {
        bail!("embedding model directory must be a plain directory");
    }
    let tokenizer_files = TokenizerFiles {
        tokenizer_file: read_model_file(model_directory, "tokenizer.json")?,
        config_file: read_model_file(model_directory, "config.json")?,
        special_tokens_map_file: read_model_file(model_directory, "special_tokens_map.json")?,
        tokenizer_config_file: read_model_file(model_directory, "tokenizer_config.json")?,
    };
    let model = UserDefinedEmbeddingModel::new(
        read_model_file(model_directory, "model.onnx")?,
        tokenizer_files,
    )
    .with_pooling(Pooling::Mean);
    let mut runtime = TextEmbedding::try_new_from_user_defined(
        model,
        InitOptionsUserDefined::new().with_intra_threads(2),
    )
    .context("could not initialize the installed client's CPU ONNX runtime")?;
    let mut vectors = runtime
        .embed([text], Some(1))
        .context("the installed client could not execute real embedding inference")?;
    if vectors.len() != 1 {
        bail!("embedding runtime returned an incomplete inference batch");
    }
    let embedding = vectors.remove(0);
    validate_vector(&embedding)?;
    let proof = EmbeddingVector {
        record_type: "release-embedding-vector",
        ilium_pid: std::process::id(),
        executable_path: ilium_platform::paths::canonicalize(&std::env::current_exe()?)?,
        input: text,
        embedding,
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &proof)?;
    writeln!(stdout)?;
    stdout.flush()?;
    drop(stdout);
    if hold_for_native_audit {
        wait_for_native_audit(std::io::stdin().lock())?;
    }
    // Explicit drop after acknowledgement preserves the mapped native runtime
    // and session through the whole observation interval.
    drop(runtime);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_vector, wait_for_native_audit};

    #[test]
    fn real_model_vector_contract_rejects_invalid_output() {
        assert!(validate_vector(&[0.25; 384]).is_ok());
        assert!(validate_vector(&[0.0; 384]).is_err());
        assert!(validate_vector(&[0.25; 383]).is_err());
        let mut vector = [0.25; 384];
        vector[0] = f32::NAN;
        assert!(validate_vector(&vector).is_err());
        vector[0] = f32::INFINITY;
        assert!(validate_vector(&vector).is_err());
    }

    #[test]
    fn audit_handshake_accepts_exact_acknowledgement_and_eof_only() {
        assert!(wait_for_native_audit(&b"native-audit-observed\n"[..]).is_ok());
        assert!(wait_for_native_audit(&b""[..]).is_ok());
        assert!(wait_for_native_audit(&b"native-audit-observed"[..]).is_err());
        assert!(wait_for_native_audit(&b"other\n"[..]).is_err());
        assert!(wait_for_native_audit(&[b'x'; 256][..]).is_err());
    }
}
