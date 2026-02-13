use std::process::Command;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub success: bool,
}

pub fn run_shell_command(command: &str) -> Result<ProcessOutput> {
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .arg("/C")
        .arg(command)
        .output()
        .with_context(|| format!("failed executing command: {command}"))?;

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .with_context(|| format!("failed executing command: {command}"))?;

    Ok(ProcessOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        success: output.status.success(),
    })
}
