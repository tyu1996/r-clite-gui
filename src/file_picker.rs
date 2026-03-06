use std::path::{Path, PathBuf};

use anyhow::Result;
use rfd::FileDialog;

pub fn pick_open_file(initial_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut dialog = FileDialog::new().set_title("Open file in rcte");

    if let Some(dir) = initial_dir {
        dialog = dialog.set_directory(dir);
    }

    Ok(dialog.pick_file())
}
