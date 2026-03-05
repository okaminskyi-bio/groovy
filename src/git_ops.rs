use std::path::Path;
use std::process::Command;

pub fn status(repo: &Path) -> Result<String, String> {
    run_git(repo, &["status", "--short", "--branch"])
}

pub fn pull_rebase(repo: &Path) -> Result<String, String> {
    run_git(repo, &["pull", "--rebase"])
}

pub fn push(repo: &Path) -> Result<String, String> {
    run_git(repo, &["push"])
}

pub fn fetch(repo: &Path) -> Result<String, String> {
    run_git(repo, &["fetch", "--all", "--prune"])
}

pub fn merge(repo: &Path, branch: &str) -> Result<String, String> {
    run_git(repo, &["merge", "--no-edit", branch])
}

pub fn commit_all(repo: &Path, message: &str) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(&run_git(repo, &["add", "-A"])?);
    out.push('\n');

    match run_git(repo, &["commit", "-m", message]) {
        Ok(v) => out.push_str(&v),
        Err(err) => {
            if err.contains("nothing to commit") {
                out.push_str("No staged changes to commit.\n");
            } else {
                return Err(err);
            }
        }
    }
    Ok(out)
}

pub fn commit_and_push(repo: &Path, message: &str) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(&commit_all(repo, message)?);
    out.push('\n');
    out.push_str(&push(repo)?);
    Ok(out)
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("Failed to run git {:?}: {e}", args))?;

    let mut text = String::new();
    if !output.stdout.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}
