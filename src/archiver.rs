use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn locate_7z() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    candidates.push(
        std::env::temp_dir()
            .join("re1hd_embedded")
            .join("7z.exe"),
    );

    if let Some(exe) = std::env::current_exe().ok() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("7z.exe"));
            candidates.push(dir.join("tools").join("7z.exe"));
            if let Some(parent) = dir.parent() {
                candidates.push(parent.join("7z.exe"));
                candidates.push(parent.join("tools").join("7z.exe"));
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("7z.exe"));
        candidates.push(cwd.join("tools").join("7z.exe"));
        candidates.push(cwd.join("..").join("tools").join("7z.exe"));
    }

    if let Some(p) = std::env::var_os("RE1HD_7Z") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("RE2HD_7Z") {
        candidates.push(PathBuf::from(p));
    }

    candidates.into_iter().find(|p| p.is_file())
}

pub fn extract(seven: &Path, archive: &Path, dest: &Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dest);
    let out = Command::new(seven)
        .arg("x")
        .arg("-y")
        .arg("-bsp1")
        .arg(format!("-o{}", dest.display()))
        .arg(archive)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not launch 7-Zip: {e}"))?;

    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr);
        let tail: String = msg.lines().rev().take(6).collect::<Vec<_>>().join(" | ");
        Err(format!(
            "7-Zip failed on '{}': {}",
            archive.display(),
            if tail.trim().is_empty() {
                "unknown error".to_string()
            } else {
                tail
            }
        ))
    }
}