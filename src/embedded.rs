use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::pipeline::ModPaths;

const MAGIC: [u8; 8] = *b"RE1HDPLD";
const NAME_FIELD: usize = 96;
const RECORD_SIZE: usize = 4 + 8 + 8 + NAME_FIELD; // len, offset, size, name
const CELL_SIZE: usize = 4 + 8 + 8; // len + offset + size

#[derive(Debug, Clone)]
pub struct Payload {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    /// File the payload lives in (the app exe or a `*-partN.exe` carrier).
    pub source: PathBuf,
}

pub fn embedded_root() -> PathBuf {
    std::env::temp_dir().join("re1hd_embedded")
}

fn read_exact_at<R: Read + Seek + ?Sized>(r: &mut R, pos: u64, buf: &mut [u8]) -> io::Result<()> {
    r.seek(SeekFrom::Start(pos))?;
    r.read_exact(buf)
}

/// Scan one file for an appended payload index. None means "not packed".
fn scan_file(path: &Path) -> Result<Option<Vec<Payload>>, String> {
    let mut f = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    let len = f.metadata().map_err(|e| format!("cannot stat {}: {e}", path.display()))?.len();
    if len < (12 + RECORD_SIZE as u64) {
        return Ok(None);
    }

    let mut tail = [0u8; 12];
    read_exact_at(&mut f, len - 12, &mut tail).map_err(|e| format!("index read: {e}"))?;

    // tail layout: [count u32][magic 8 bytes]
    let count = u32::from_le_bytes(tail[0..4].try_into().unwrap()) as usize;
    if tail[4..12] != MAGIC {
        return Ok(None);
    }
    if count == 0 || count > 4096 {
        return Err(format!("corrupt embedded index (bad count) in {}.", path.display()));
    }

    let records_start = len - 12 - (count * RECORD_SIZE) as u64;
    let mut payloads = Vec::with_capacity(count);
    let mut pos = records_start;
    for _ in 0..count {
        let mut cell = [0u8; CELL_SIZE];
        read_exact_at(&mut f, pos, &mut cell).map_err(|e| format!("index read: {e}"))?;
        let name_len = u32::from_le_bytes(cell[0..4].try_into().unwrap()) as usize;
        let offset = u64::from_le_bytes(cell[4..12].try_into().unwrap());
        let size = u64::from_le_bytes(cell[12..20].try_into().unwrap());

        let mut name_buf = vec![0u8; NAME_FIELD];
        read_exact_at(&mut f, pos + CELL_SIZE as u64, &mut name_buf)
            .map_err(|e| format!("index read: {e}"))?;
        pos += RECORD_SIZE as u64;
        if name_len > NAME_FIELD {
            return Err(format!("corrupt embedded index (name too long) in {}.", path.display()));
        }
        let name = String::from_utf8(name_buf[..name_len].to_vec())
            .map_err(|_| format!("corrupt embedded index (bad name) in {}.", path.display()))?;
        payloads.push(Payload { name, offset, size, source: path.to_path_buf() });
    }
    Ok(Some(payloads))
}

/// Scan the running exe and its `*-partN.exe` siblings for appended payload
/// indexes. Returns None when the app was not packed. When packed, the main
/// exe plus every part found next to it are merged into one payload list.
///
/// Windows refuses to load PE images whose file size reaches 4 GiB, so the
/// packer splits payloads across the main exe and a series of `-partN.exe`
/// carriers. All of them share the same tail index format.
pub fn scan() -> Result<Option<Vec<Payload>>, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate this executable: {e}"))?;
    let Some(mut payloads) = scan_file(&exe)? else {
        return Ok(None);
    };

    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    for n in 2u32..=16 {
        let part = dir.join(format!("{stem}-part{n}.exe"));
        if !part.is_file() {
            break;
        }
        match scan_file(&part)? {
            Some(p) => payloads.extend(p),
            None => {
                return Err(format!(
                    "corrupt part file (no payload index): {}",
                    part.display()
                ))
            }
        }
    }
    Ok(Some(payloads))
}

/// Extract a single payload region from its source file (exe or part) into `dest`.
pub fn extract_payload(payload: &Payload, dest: &Path) -> io::Result<()> {
    let mut f = File::open(&payload.source)?;
    f.seek(SeekFrom::Start(payload.offset))?;
    let mut limited = f.take(payload.size);
    let mut out = File::create(dest)?;
    io::copy(&mut limited, &mut out)?;
    Ok(())
}

pub fn all_cached(payloads: &[Payload]) -> bool {
    let root = embedded_root();
    payloads
        .iter()
        .all(|p| root.join(&p.name).metadata().map(|m| m.len() == p.size).unwrap_or(false))
}

/// Extract all payloads into TEMP/re1hd_embedded, reporting fraction done.
pub fn extract_all(
    payloads: &[Payload],
    on_progress: &mut dyn FnMut(u64, u64), // (bytes_done, bytes_total)
) -> io::Result<()> {
    let root = embedded_root();
    std::fs::create_dir_all(&root)?;

    let byte_total: u64 = payloads.iter().map(|p| p.size).sum();
    let mut done: u64 = 0;

    for p in payloads {
        let dest = root.join(&p.name);
        if dest.metadata().map(|m| m.len() == p.size).unwrap_or(false) {
            done += p.size;
            on_progress(done, byte_total);
            continue;
        }
        extract_payload(p, &dest)?;
        done += p.size;
        on_progress(done, byte_total);
    }
    Ok(())
}

fn slot_for(name: &str) -> Option<usize> {
    let lower = name.to_lowercase();
    if lower.ends_with(".exe") {
        return Some(0); // 7z.exe
    }
    if lower.ends_with(".dll") {
        return Some(1); // 7z.dll
    }
    if lower.ends_with(".xm") || lower.ends_with(".it") || lower.ends_with(".mod") || lower.ends_with(".s3m")
    {
        return Some(10); // menu music
    }
    let heuristics: [(usize, &[&str]); 9] = [
        (2, &["hd_mod"]),     // team x
        (3, &["shdp_1.1"]),   // seamless hd main
        (4, &["shdp", "patch"]), // seamless hd patch
        (9, &["fmv"]),        // fmv pack (must come before enhance)
        (5, &["enhance"]),    // re-enhance textures
        (6, &["re1cr"]),      // rebirth
        (7, &["mediakite"]),  // mediakite exe
        (8, &["audio"]),      // hq audio
        (11, &["dgvoodoo"]),  // AMD dgVoodoo fix archive
    ];
    heuristics
        .iter()
        .find(|(_, needles)| needles.iter().all(|n| lower.contains(n)))
        .map(|(slot, _)| *slot)
}

pub struct Bundle {
    pub seven: PathBuf,
    pub seven_dll: PathBuf,
    pub mods: Option<ModPaths>,
    pub music: Vec<PathBuf>,
}

/// Map embedded payloads to a ready-to-use bundle (7z engine + mod paths + music tracks).
pub fn resolve(payloads: &[Payload]) -> Bundle {
    let root = embedded_root();
    let mut seven = PathBuf::new();
    let mut seven_dll = PathBuf::new();
    let mut music: Vec<PathBuf> = Vec::new();
    let mut mods = ModPaths {
        pack1: PathBuf::new(),
        pack2: PathBuf::new(),
        pack3: PathBuf::new(),
        pack4: PathBuf::new(),
        rebirth: PathBuf::new(),
        exe_patch: PathBuf::new(),
        audio: PathBuf::new(),
        fmv: PathBuf::new(),
        amd_fix: PathBuf::new(),
    };
    let mut found: Vec<bool> = vec![false; 9];

    for p in payloads {
        let dest = root.join(&p.name);
        match slot_for(&p.name) {
            Some(0) => seven = dest,
            Some(1) => seven_dll = dest,
            Some(2) => {
                mods.pack1 = dest;
                found[0] = true;
            }
            Some(3) => {
                mods.pack2 = dest;
                found[1] = true;
            }
            Some(4) => {
                mods.pack3 = dest;
                found[2] = true;
            }
            Some(5) => {
                mods.pack4 = dest;
                found[3] = true;
            }
            Some(6) => {
                mods.rebirth = dest;
                found[4] = true;
            }
            Some(7) => {
                mods.exe_patch = dest;
                found[5] = true;
            }
            Some(8) => {
                mods.audio = dest;
                found[6] = true;
            }
            Some(9) => {
                mods.fmv = dest;
                found[7] = true;
            }
            Some(10) => music.push(dest),
            Some(11) => {
                mods.amd_fix = dest;
                found[8] = true;
            }
            _ => {}
        }
    }

    music.sort();
    let complete = found.iter().all(|b| *b) && !seven.as_os_str().is_empty();
    Bundle {
        seven,
        seven_dll,
        mods: if complete { Some(mods) } else { None },
        music,
    }
}