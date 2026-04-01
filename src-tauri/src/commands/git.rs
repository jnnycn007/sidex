use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct GitChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GitStatus {
    pub branch: String,
    pub changes: Vec<GitChange>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GitLogEntry {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
}

fn run_git(path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute git: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git error: {}", stderr.trim()))
    }
}

#[tauri::command]
pub async fn git_status(path: String) -> Result<GitStatus, String> {
    let output = run_git(&path, &["status", "--porcelain=v1", "-b"])?;
    let mut lines = output.lines();

    let branch = lines
        .next()
        .unwrap_or("## HEAD (no branch)")
        .trim_start_matches("## ")
        .split("...")
        .next()
        .unwrap_or("HEAD")
        .to_string();

    let changes = lines
        .filter(|l| !l.is_empty())
        .map(|line| {
            let xy = &line[..2];
            let file_path = line[3..].trim().to_string();
            let x = xy.chars().next().unwrap_or(' ');
            let y = xy.chars().nth(1).unwrap_or(' ');

            let (status, staged) = if x != ' ' && x != '?' {
                (format!("{}", x), true)
            } else {
                (format!("{}", y), false)
            };

            let status = match status.as_str() {
                "M" => "modified".to_string(),
                "A" => "added".to_string(),
                "D" => "deleted".to_string(),
                "R" => "renamed".to_string(),
                "C" => "copied".to_string(),
                "?" => "untracked".to_string(),
                "!" => "ignored".to_string(),
                other => other.to_string(),
            };

            GitChange {
                path: file_path,
                status,
                staged,
            }
        })
        .collect();

    Ok(GitStatus { branch, changes })
}

#[tauri::command]
pub async fn git_diff(path: String, file: Option<String>, staged: bool) -> Result<String, String> {
    let mut args = vec!["diff"];
    if staged {
        args.push("--staged");
    }
    if let Some(ref f) = file {
        args.push("--");
        args.push(f.as_str());
    }
    run_git(&path, &args)
}

#[tauri::command]
pub async fn git_log(path: String, limit: Option<u32>) -> Result<Vec<GitLogEntry>, String> {
    let limit_str = format!("-{}", limit.unwrap_or(50));
    let output = run_git(
        &path,
        &[
            "log",
            "--format=%H%n%s%n%an%n%aI",
            &limit_str,
        ],
    )?;

    let lines: Vec<&str> = output.lines().collect();
    let entries = lines
        .chunks(4)
        .filter(|chunk| chunk.len() == 4)
        .map(|chunk| GitLogEntry {
            hash: chunk[0].to_string(),
            message: chunk[1].to_string(),
            author: chunk[2].to_string(),
            date: chunk[3].to_string(),
        })
        .collect();

    Ok(entries)
}

#[tauri::command]
pub async fn git_add(path: String, files: Vec<String>) -> Result<(), String> {
    let mut args = vec!["add", "--"];
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend(refs);
    run_git(&path, &args)?;
    Ok(())
}

#[tauri::command]
pub async fn git_commit(path: String, message: String) -> Result<String, String> {
    run_git(&path, &["commit", "-m", &message])?;
    let hash = run_git(&path, &["rev-parse", "HEAD"])?;
    Ok(hash.trim().to_string())
}

#[tauri::command]
pub async fn git_checkout(path: String, branch: String) -> Result<(), String> {
    run_git(&path, &["checkout", &branch])?;
    Ok(())
}

#[tauri::command]
pub async fn git_branches(path: String) -> Result<Vec<GitBranch>, String> {
    let output = run_git(&path, &["branch", "-a"])?;

    let branches = output
        .lines()
        .filter(|l| !l.is_empty())
        .filter(|l| !l.contains("->"))
        .map(|line| {
            let current = line.starts_with('*');
            let name = line
                .trim_start_matches('*')
                .trim()
                .to_string();
            let remote = name.starts_with("remotes/");
            let name = name
                .trim_start_matches("remotes/")
                .to_string();

            GitBranch {
                name,
                current,
                remote,
            }
        })
        .collect();

    Ok(branches)
}

#[tauri::command]
pub async fn git_init(path: String) -> Result<(), String> {
    run_git(&path, &["init"])?;
    Ok(())
}

#[tauri::command]
pub async fn git_is_repo(path: String) -> Result<bool, String> {
    let output = Command::new("git")
        .current_dir(&path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|e| format!("Failed to execute git: {}", e))?;

    Ok(output.status.success())
}
