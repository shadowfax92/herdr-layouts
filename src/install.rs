//! Installs a foreground menu handoff without replacing the user's menu or
//! shortcuts. The dialog runs inside the existing popup and captures its source.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use toml_edit::{value, Table};

use crate::herdr::Herdr;

pub fn install_menu() -> Result<()> {
    let directory = Herdr::from_env().output(&["plugin", "config-dir", "shadowfax.menu"])?;
    let path = PathBuf::from(directory.trim()).join("config.toml");
    match add_menu_item(&path, &std::env::current_exe()?)? {
        Some(backup) => println!(
            "Installed Create layout on Alt-q → g. Backup: {}",
            backup.display()
        ),
        None => println!("Create layout is already installed in the menu."),
    }
    Ok(())
}

pub fn add_menu_item(path: &Path, executable: &Path) -> Result<Option<PathBuf>> {
    let target =
        fs::canonicalize(path).context("Install Herdr Menu before adding Create layout")?;
    let original = fs::read_to_string(&target)?;
    let mut document = original.parse::<toml_edit::DocumentMut>()?;
    let items = document
        .get_mut("items")
        .and_then(|items| items.as_array_of_tables_mut())
        .context("Menu config must contain [[items]]")?;
    // Single-quoted paths are valid in both Fish and POSIX shells. Escape only
    // characters special inside those quotes; never interpolate source IDs.
    let executable = executable.to_str().context("Plugin path is not UTF-8")?;
    let quoted = executable
        .chars()
        .map(|c| match c {
            '\\' => "'\"\\\\\"'".into(),
            '\'' => "'\"'\"'".into(),
            c => c.to_string(),
        })
        .collect::<String>();
    let command = format!("exec '{quoted}' grid-dialog --pane \"$HERDR_MENU_SOURCE_PANE_ID\"");
    if items
        .iter()
        .any(|item| item.get("command").and_then(|v| v.as_str()) == Some(&command))
    {
        return Ok(None);
    }
    if items
        .iter()
        .any(|item| item.get("key").and_then(|v| v.as_str()) == Some("g"))
    {
        bail!("Menu key 'g' is already used; add a Create layout item with another key");
    }
    let mut item = Table::new();
    item["type"] = value("shell");
    item["key"] = value("g");
    item["label"] = value("create layout…");
    item["command"] = value(command);
    item["mode"] = value("foreground");
    items.push(item);
    let rendered = document.to_string();
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let backup = target.with_extension(format!("toml.layouts-{suffix}.bak"));
    let temporary = target.with_extension(format!("toml.layouts-{suffix}.tmp"));
    let mut backup_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)?;
    backup_file.set_permissions(fs::metadata(&target)?.permissions())?;
    backup_file.write_all(original.as_bytes())?;
    backup_file.sync_all()?;
    // Rename a complete file atomically; never truncate the user's config if a
    // write fails. Resolve symlinks first and reject intervening edits.
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.set_permissions(fs::metadata(&target)?.permissions())?;
        file.write_all(rendered.as_bytes())?;
        file.sync_all()?;
        if fs::read_to_string(&target)? != original {
            bail!("Menu changed during install; retry");
        }
        fs::rename(&temporary, &target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(Some(backup))
}
