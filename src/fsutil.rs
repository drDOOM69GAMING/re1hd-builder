use std::io;
use std::path::Path;

pub fn dir_size(p: &Path) -> io::Result<u64> {
    if p.is_file() {
        return Ok(p.metadata()?.len());
    }
    let mut total = 0u64;
    let mut stack: Vec<std::path::PathBuf> = vec![p.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

fn clean_and_copy_file(src: &Path, dst: &Path) -> io::Result<()> {
    if let Ok(meta) = std::fs::metadata(dst) {
        if meta.permissions().readonly() {
            let mut perms = meta.permissions();
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(dst, perms);
        }
        let _ = std::fs::remove_file(dst);
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

fn rec_copy(
    src: &Path,
    dst: &Path,
    copied: &mut u64,
    on_progress: &mut dyn FnMut(u64) -> bool,
) -> io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        let meta = entry.metadata()?;

        if meta.is_dir() {
            if !d.exists() {
                std::fs::create_dir_all(&d)?;
            }
            rec_copy(&s, &d, copied, on_progress)?;
        } else {
            clean_and_copy_file(&s, &d)?;
            *copied += meta.len();
            if !(on_progress)(*copied) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "aborted by user"));
            }
        }
    }
    Ok(())
}

pub fn copy_tree(
    src: &Path,
    dst: &Path,
    on_progress: &mut dyn FnMut(u64) -> bool,
) -> io::Result<u64> {
    let mut copied = 0u64;
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    rec_copy(src, dst, &mut copied, on_progress)?;
    Ok(copied)
}

pub fn move_into_place(src: &Path, dst: &Path) -> io::Result<()> {
    if let Ok(()) = std::fs::rename(src, dst) {
        return Ok(());
    }
    copy_tree(src, dst, &mut |_| true)?;
    remove_dir_all_including_ro(src)?;
    Ok(())
}

pub fn remove_dir_all_including_ro(p: &Path) -> io::Result<()> {
    let mut stack: Vec<std::path::PathBuf> = vec![p.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let pa = entry.path();
            let meta = entry.metadata()?;
            if meta.is_dir() {
                stack.push(pa);
            } else {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    perms.set_readonly(false);
                    let _ = std::fs::set_permissions(&pa, perms);
                }
                let _ = std::fs::remove_file(&pa);
            }
        }
    }
    std::fs::remove_dir_all(p)
}

pub fn copy_tree_contents(
    src: &Path,
    dst: &Path,
    on_progress: &mut dyn FnMut(u64) -> bool,
) -> io::Result<u64> {
    let mut copied = 0u64;
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            if !d.exists() {
                std::fs::create_dir_all(&d)?;
            }
            rec_copy(&s, &d, &mut copied, on_progress)?;
        } else {
            clean_and_copy_file(&s, &d)?;
            copied += meta.len();
            if !(on_progress)(copied) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "aborted by user"));
            }
        }
    }
    Ok(copied)
}