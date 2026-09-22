use std::fs;
use std::path::Path;
use std::process::Command;

use herdr_layouts::install::add_menu_item;

#[test]
fn menu_install_preserves_custom_content_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "# My custom menu\ntitle = 'Mine'\n[[items]]\nkey = 'd'\ntype = 'redistribute'\nlabel = 'Rearrange'\n";
    fs::write(&path, original).unwrap();
    let backup = add_menu_item(&path, Path::new("/tmp/herdr-layouts"))
        .unwrap()
        .unwrap();
    assert_eq!(fs::read_to_string(backup).unwrap(), original);
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.starts_with(original));
    assert!(content.contains("HERDR_MENU_SOURCE_PANE_ID"));
    assert!(add_menu_item(&path, Path::new("/tmp/herdr-layouts"))
        .unwrap()
        .is_none());
    assert_eq!(fs::read_to_string(path).unwrap(), content);
}

#[test]
fn conflicting_menu_key_leaves_config_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "[[items]]\nkey = 'g'\ncommand = 'my-command'\n";
    fs::write(&path, original).unwrap();
    assert!(add_menu_item(&path, Path::new("/tmp/herdr-layouts")).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn installed_command_quotes_executable_and_source_for_fish_and_posix() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[[items]]\nkey = 'd'\ncommand = 'true'\n").unwrap();
    let executable = dir.path().join("layout ' \\ $() tool");
    fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    add_menu_item(&path, &executable).unwrap();
    let document = fs::read_to_string(path)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    let items = document["items"].as_array_of_tables().unwrap();
    let command = items.get(1).unwrap()["command"].as_str().unwrap();
    for shell in ["/bin/sh", "/opt/homebrew/bin/fish"] {
        if !Path::new(shell).exists() {
            continue;
        }
        let output = Command::new(shell)
            .args(["-c", command])
            .env("HERDR_MENU_SOURCE_PANE_ID", "w1:p7; touch bad")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "grid-dialog\n--pane\nw1:p7; touch bad\n"
        );
    }
}
