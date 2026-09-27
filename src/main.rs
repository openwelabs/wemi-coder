mod agent;
mod data;
mod model;
mod tools;
mod tui;

use anyhow::{Context, Result};
use data::UserDataDirs;
use std::path::PathBuf;
use tui::App;

#[tokio::main]
async fn main() -> Result<()> {
    let data_dirs = UserDataDirs::discover()?;
    data_dirs.ensure_exists()?;

    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().context("unable to determine current directory")?);

    App::new(root, data_dirs.api_settings_file())?.run().await
}
