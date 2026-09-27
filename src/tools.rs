use anyhow::{Context, Result};
use diffy::create_patch;
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, process::Stdio};
use tokio::process::Command;
use walkdir::WalkDir;

pub fn tool_definitions() -> Vec<Value> {
    vec![
    json!({"type":"function","function":{"name":"read_file","description":"Read a UTF-8 text file.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}),
    json!({"type":"function","function":{"name":"write_file","description":"Replace a UTF-8 text file. The user will be shown a diff first.","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}}}),
    json!({"type":"function","function":{"name":"search_files","description":"Search text recursively in project files.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}}),
    json!({"type":"function","function":{"name":"shell","description":"Run a shell command in the project. Dangerous commands require confirmation.","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}}),
    ]
}

pub fn resolve(root: &Path, path: &str) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let path = root.join(path);
    let candidate = if path.exists() {
        path.canonicalize()?
    } else {
        let parent = path.parent().context("path has no parent")?.canonicalize()?;
        parent.join(path.file_name().context("path has no filename")?)
    };
    if !candidate.starts_with(&root) { anyhow::bail!("path escapes project root"); }
    Ok(candidate)
}

pub fn read_file(root: &Path, path: &str) -> Result<String> {
    Ok(fs::read_to_string(resolve(root, path)?).context("read_file failed")?)
}

pub fn diff_for_write(root: &Path, path: &str, content: &str) -> Result<String> {
    let target = resolve(root, path)?;
    let old = fs::read_to_string(&target).unwrap_or_default();
    Ok(create_patch(&old, content).to_string())
}

pub fn write_file(root: &Path, path: &str, content: &str) -> Result<()> {
    let target = resolve(root, path)?;
    if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
    fs::write(target, content)?;
    Ok(())
}

pub fn search_files(root: &Path, query: &str) -> Result<String> {
    let mut output = String::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
        if entry.path().to_string_lossy().contains("/target/") { continue; }
        if let Ok(text) = fs::read_to_string(entry.path()) {
            for (line, value) in text.lines().enumerate() {
                if value.contains(query) {
                    output.push_str(&format!("{}:{}:{}\n", entry.path().strip_prefix(root).unwrap_or(entry.path()).display(), line + 1, value.trim()));
                }
            }
        }
    }
    Ok(if output.is_empty() { "No matches.".into() } else { output })
}

pub async fn shell(root: &Path, command: &str) -> Result<String> {
    let output = Command::new("sh").arg("-c").arg(command).current_dir(root)
        .stdin(Stdio::null()).output().await.context("shell failed")?;
    Ok(format!("status: {}\nstdout:\n{}\nstderr:\n{}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
}

pub fn is_dangerous(command: &str) -> bool {
    ["rm ", "rm\t", "sudo", "shutdown", "reboot", "mkfs", "dd ", "git reset", "git clean"]
        .iter().any(|needle| command.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("wemi-coder-tools-{}-{}", std::process::id(), unique));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn rejects_paths_outside_project() {
        let root = temp_root();
        assert!(resolve(&root, "../outside.txt").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn produces_file_diff() {
        let root = temp_root();
        fs::write(root.join("note.txt"), "old\n").unwrap();
        let patch = diff_for_write(&root, "note.txt", "new\n").unwrap();
        assert!(patch.contains("old"));
        assert!(patch.contains("new"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn classifies_dangerous_commands() {
        assert!(is_dangerous("rm -rf build"));
        assert!(is_dangerous("git reset --hard"));
        assert!(!is_dangerous("cargo test"));
    }
}
