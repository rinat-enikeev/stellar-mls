//! Thin wrapper around the `ceremony_tool` binary.
//!
//! The coordinator never reimplements ceremony crypto — it shells out to the
//! exact same binary participants download. This keeps the trust root
//! narrow: if the binary is correct, the coordinator is correct.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use tokio::fs;
use tokio::process::Command;
use uuid::Uuid;

#[derive(Clone)]
pub struct CeremonyTool {
    binary: PathBuf,
    work_dir: PathBuf,
}

pub struct VerifyArtifacts {
    pub state_srs: Vec<u8>,
    pub state_txt: String,
    pub receipt: String,
}

impl CeremonyTool {
    pub fn new(binary: PathBuf, work_dir: PathBuf) -> Self {
        Self { binary, work_dir }
    }

    async fn materialize(&self, name: &str, artifacts: &VerifyArtifacts) -> Result<PathBuf> {
        let dir = self.work_dir.join(format!("{}-{}", name, Uuid::new_v4()));
        fs::create_dir_all(&dir).await.context("mkdir work dir")?;
        fs::write(dir.join("state.srs"), &artifacts.state_srs)
            .await
            .context("write state.srs")?;
        fs::write(dir.join("state.txt"), artifacts.state_txt.as_bytes())
            .await
            .context("write state.txt")?;
        fs::write(dir.join("receipt.txt"), artifacts.receipt.as_bytes())
            .await
            .context("write receipt.txt")?;
        Ok(dir)
    }

    pub async fn verify_contribution(
        &self,
        before: &VerifyArtifacts,
        after: &VerifyArtifacts,
    ) -> Result<String> {
        let before_dir = self.materialize("before", before).await?;
        let after_dir = self.materialize("after", after).await?;
        let output = Command::new(&self.binary)
            .arg("verify-contribution")
            .arg("--before-state-dir")
            .arg(&before_dir)
            .arg("--after-state-dir")
            .arg(&after_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("spawn ceremony_tool")?;
        // Best-effort cleanup regardless of outcome.
        let _ = fs::remove_dir_all(&before_dir).await;
        let _ = fs::remove_dir_all(&after_dir).await;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow!(
                "verify-contribution failed (exit={:?}): {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub async fn verify_state(&self, state: &VerifyArtifacts) -> Result<String> {
        let dir = self.materialize("verify-state", state).await?;
        let output = Command::new(&self.binary)
            .arg("verify-state")
            .arg("--state-dir")
            .arg(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("spawn ceremony_tool")?;
        let _ = fs::remove_dir_all(&dir).await;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow!(
                "verify-state failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub async fn phase2_summary(&self, state: &VerifyArtifacts) -> Result<String> {
        let dir = self.materialize("phase2", state).await?;
        let output = Command::new(&self.binary)
            .arg("phase2-summary")
            .arg("--state-dir")
            .arg(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("spawn ceremony_tool")?;
        let _ = fs::remove_dir_all(&dir).await;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow!(
                "phase2-summary failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }
}
