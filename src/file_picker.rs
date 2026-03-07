use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use std::process::Command;

use anyhow::{anyhow, Result};

#[cfg(not(target_os = "linux"))]
use rfd::FileDialog;

#[cfg(target_os = "linux")]
pub fn pick_open_file(initial_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut command = Command::new("zenity");
    command.arg("--file-selection");
    command.arg("--title=Open file in rcte");

    if let Some(dir) = initial_dir {
        command.current_dir(dir);
        command.arg(format!("--filename={}/", dir.display()));
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow!("Open error: zenity is not installed."));
        }
        Err(err) => return Err(err.into()),
    };

    match output.status.code() {
        Some(0) => {
            let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if selected.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(selected)))
            }
        }
        Some(1) => Ok(None),
        Some(code) => Err(anyhow!(
            "Open error: zenity exited with status {}: {}",
            code,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        None => Err(anyhow!("Open error: zenity terminated unexpectedly.")),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn pick_open_file(initial_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut dialog = FileDialog::new().set_title("Open file in rcte");

    if let Some(dir) = initial_dir {
        dialog = dialog.set_directory(dir);
    }

    Ok(dialog.pick_file())
}
