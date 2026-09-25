use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::archiver;
use crate::fsutil;

pub const STEP_NAMES: [&str; 12] = [
    "Extract game disc image",
    "Isolate data / build RE1HD",
    "Texture pack 1 - Team X HD",
    "Texture pack 2 - Seamless HD",
    "Texture pack 3 - SHDP patch",
    "Texture pack 4 - RE-ENHANCE",
    "Classic REbirth DLLs",
    "Mediakite 1.01 EXE",
    "High quality audio mod",
    "FMV pack",
    "AMD GPU fix",
    "Final check",
];

#[derive(Debug, Clone)]
pub struct ModPaths {
    pub pack1: PathBuf,     // Resident_Evil_HD_mod_v20220831.zip
    pub pack2: PathBuf,     // RE_SHDP_1.1.zip
    pub pack3: PathBuf,     // RE_SHDP_-_Patch_1.1.zip
    pub pack4: PathBuf,     // RE-ENHANCE_RE1_v2.0.zip
    pub rebirth: PathBuf,   // re1cr-2020-12-06.7z
    pub exe_patch: PathBuf, // mediakite 1.01.7z (Biohazard.exe)
    pub audio: PathBuf,     // RE1 PC - HQ Audio Mod (simple install).zip
    pub fmv: PathBuf,       // RE-ENHANCE_RE1_FMV-Pack_V1.1_2.zip
    pub amd_fix: PathBuf,   // dgVoodoo_AMD_fix.zip (AMD-only compatibility shim)
}

#[derive(Debug, Clone)]
pub enum Event {
    Log(String),
    Phase(usize),
    Progress(usize, f32),
    Status(String),
    Error(String),
    Prep(f32),
    Done,
}

fn send(tx: &Sender<Event>, cx: &egui::Context, ev: Event) {
    let _ = tx.send(ev);
    cx.request_repaint();
}

pub fn beep_triple() {
    // "STARRRS" in International Morse Code: dot(ding)=short, dash(DING)=long.
    // Unit = 100ms. Letters: S ... / T - / A .- / R .-. / R .-. / R .-. / S ...
    const UNIT: u64 = 100;
    let letters: [&[bool]; 7] = [
        &[false, false, false], // S ...
        &[true],                // T -
        &[false, true],         // A .-
        &[false, true, false],  // R .-.
        &[false, true, false],  // R .-.
        &[false, true, false],  // R .-.
        &[false, false, false], // S ...
    ];
    for letter in letters {
        for &is_dash in letter {
            let on_ms = UNIT * if is_dash { 3 } else { 1 };
            unsafe {
                windows_sys::Win32::System::Diagnostics::Debug::Beep(700, on_ms as u32);
            }
            std::thread::sleep(Duration::from_millis(UNIT)); // intra-letter gap
        }
        std::thread::sleep(Duration::from_millis(UNIT * 2)); // makes a 3-unit letter gap
    }
}

fn is_named(p: &Path, names: &[&str]) -> bool {
    p.file_name()
        .map(|n| {
            let n = n.to_string_lossy();
            names.iter().any(|want| n.eq_ignore_ascii_case(want))
        })
        .unwrap_or(false)
}

fn find_named(root: &Path, names: &[&str], max_depth: usize) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if is_named(&p, names) {
                    return Some(p);
                }
                if p.is_dir() && depth < max_depth {
                    stack.push((p, depth + 1));
                }
            }
        }
    }
    None
}

/// The game data lives in a folder called "HORR" on the disc.
fn find_horr(stage: &Path) -> Option<PathBuf> {
    find_named(stage, &["horr"], 2)
}

/// Some discs ship an installer folder named like "BIOHAZARD Mediakite";
/// it is not needed for the game and must not end up in the build.
fn remove_mediakite_folder(stage: &Path) {
    if let Ok(rd) = std::fs::read_dir(stage) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let lower = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if lower.contains("biohazard") && lower.contains("mediakite") {
                    let _ = fsutil::remove_dir_all_including_ro(&p);
                }
            }
        }
    }
}

fn find_exe_file(root: &Path) -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                let name = p.file_name()?.to_string_lossy().to_lowercase();
                if name.ends_with(".exe") {
                    if name.starts_with("biohazard") {
                        return Some(p);
                    }
                    if fallback.is_none() {
                        fallback = Some(p);
                    }
                }
            }
        }
    }
    fallback
}

fn work_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("re1hd_build_{}", tag))
}

fn clear_dir(p: &Path) {
    let _ = fsutil::remove_dir_all_including_ro(p);
    let _ = std::fs::create_dir_all(p);
}

fn copy_step(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    phase: usize,
    src: &Path,
    dst: &Path,
) -> Result<(), String> {
    let total = fsutil::dir_size(src).unwrap_or(0) as f32;
    let mut last_report = Instant::now();
    let mut copied = 0u64;

    fsutil::copy_tree_contents(src, dst, &mut |bytes: u64| -> bool {
        copied = bytes;
        if last_report.elapsed().as_millis() > 80 {
            let frac = if total > 0.0 {
                (bytes as f32 / total).min(1.0)
            } else {
                1.0
            };
            send(tx, ctx, Event::Progress(phase, frac));
            last_report = Instant::now();
        }
        true
    })
    .map_err(|e| format!("copy failed: {e}"))?;

    send(tx, ctx, Event::Progress(phase, 1.0));
    if total > 0.0 {
        send(
            tx,
            ctx,
            Event::Log(format!(
                "[copy] {:.1} MiB merged into RE1HD",
                copied as f32 / 1048576.0
            )),
        );
    }
    Ok(())
}

fn unpack(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    seven: &Path,
    archive: &Path,
    tmp: &Path,
) -> Result<(), String> {
    send(tx, ctx, Event::Log(format!("[unpack] {}", archive.display())));
    archiver::extract(seven, archive, tmp)
}

/// Recognize an AMD-family adapter name (AMD / Radeon / ATI), case-insensitively.
fn is_amd_gpu_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("amd") || lower.contains("radeon") {
        return true;
    }
    // Bare "ati" only counts as its own token so unrelated names that merely
    // contain those letters (e.g. "Hawaii") never trigger a false positive.
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|tok| tok == "ati")
}

/// Query the installed display adapters and report whether any is an AMD GPU.
fn amd_gpu_present() -> bool {
    let Ok(out) = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }",
        ])
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| is_amd_gpu_name(line))
}

/// Copy every file that lives directly inside the FMV archive's sub folder
/// (e.g. the `hires` or `movie` folder) into `dst`, overwriting.
fn copy_subfolder(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    phase: usize,
    tmp: &Path,
    folder_name: &str,
    dst: &Path,
) -> Result<bool, String> {
    let src = find_named(tmp, &[folder_name], 1);
    match src {
        Some(src) => {
            send(
                tx,
                ctx,
                Event::Log(format!("[fmv] copying {folder_name} -> {}", dst.display())),
            );
            copy_step(ctx, tx, phase, &src, dst)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn run_pipeline(cx: egui::Context, tx_event: Sender<Event>, iso: PathBuf, mods: ModPaths) {
    let tx = tx_event;

    let fail = |msg: String| {
        send(&tx, &cx, Event::Error(msg));
    };

    let seven = match archiver::locate_7z() {
        Some(p) => p,
        None => {
            fail(
                "7-Zip not found. Place 7z.exe + 7z.dll in a 'tools' folder next to this app."
                    .to_string(),
            );
            return;
        }
    };

    send(&tx, &cx, Event::Log(format!("7-Zip engine: {}", seven.display())));

    let iso_dir = match iso.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let stem = iso
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "game".to_string());
    let stage = iso_dir.join(format!(".{}.re1hd_stage", stem));
    let re1hd = iso_dir.join("RE1HD");

    // ---- Phase 0: ISO extraction ----
    send(&tx, &cx, Event::Phase(0));
    send(&tx, &cx, Event::Status("Extracting game disc image...".into()));
    clear_dir(&stage);

    let iso_mb = std::fs::metadata(&iso).map(|m| m.len() as f32 / 1048576.0).unwrap_or(0.0);
    send(&tx, &cx, Event::Log(format!("[iso] extracting {:.0} MiB...", iso_mb)));
    if let Err(e) = unpack(&cx, &tx, &seven, &iso, &stage) {
        fail(e);
        return;
    }
    send(&tx, &cx, Event::Progress(0, 1.0));
    send(&tx, &cx, Event::Phase(1));

    // ---- Phase 1: isolate HORR -> RE1HD ----
    let horr = match find_horr(&stage) {
        Some(d) => d,
        None => {
            fail("Could not locate the 'HORR' folder inside the extracted game disc.".into());
            return;
        }
    };

    if re1hd.exists() {
        send(&tx, &cx, Event::Log("Removing previous RE1HD output folder...".into()));
        if let Err(e) = fsutil::remove_dir_all_including_ro(&re1hd) {
            fail(format!("could not remove existing RE1HD folder: {e}"));
            return;
        }
    }

    // The disc installer folder must not survive the copy.
    remove_mediakite_folder(&stage);

    send(&tx, &cx, Event::Status("Moving HORR folder as RE1HD...".into()));
    if let Err(e) = fsutil::move_into_place(&horr, &re1hd) {
        fail(format!("could not finalize RE1HD folder: {e}"));
        return;
    }
    send(&tx, &cx, Event::Log("[iso] HORR renamed to RE1HD".into()));

    // The game writes its save files into a "SaveData" subfolder of the RE1HD
    // root. If that folder is missing, the game crashes the first time the
    // player tries to save, so make sure it always exists. create_dir_all is
    // idempotent, so it is harmless if a mod already shipped one.
    let savedata = re1hd.join("SaveData");
    if let Err(e) = std::fs::create_dir_all(&savedata) {
        fail(format!("could not create SaveData folder: {e}"));
        return;
    }
    send(&tx, &cx, Event::Log("[iso] SaveData folder ready".into()));

    send(&tx, &cx, Event::Progress(1, 1.0));

    if stage.exists() {
        send(&tx, &cx, Event::Log("removing temporary extraction folder...".into()));
        let _ = fsutil::remove_dir_all_including_ro(&stage);
    }

    // ---- Phases 2..8: texture packs, REbirth DLLs, EXE, audio -----
    let packs: [(usize, &str, &PathBuf, &str); 7] = [
        (2, "Team X Textures pack 1", &mods.pack1, "tex1"),
        (3, "Seamless HD pack 2", &mods.pack2, "tex2"),
        (4, "SHDP patch 3", &mods.pack3, "tex3"),
        (5, "RE-ENHANCE pack 4", &mods.pack4, "tex4"),
        (6, "Classic REbirth DLLs", &mods.rebirth, "rebirth"),
        (7, "Mediakite 1.01 EXE", &mods.exe_patch, "exe"),
        (8, "HQ audio mod", &mods.audio, "audio"),
    ];

    for (p, name, archive, tag) in packs {
        send(&tx, &cx, Event::Phase(p));
        send(&tx, &cx, Event::Status(format!("Integrating {name}...")));
        let tmp = work_dir(tag);
        clear_dir(&tmp);
        if let Err(e) = unpack(&cx, &tx, &seven, archive, &tmp) {
            fail(e);
            return;
        }

        if tag == "exe" {
            // Mediakite ships the patched Biohazard.exe.
            let patch_exe = match find_exe_file(&tmp) {
                Some(p) => p,
                None => {
                    fail("No executable found inside the Mediakite archive.".into());
                    return;
                }
            };
            let final_exe = re1hd.join("Biohazard.exe");
            match std::fs::copy(&patch_exe, &final_exe) {
                Ok(_) => {
                    send(
                        &tx,
                        &cx,
                        Event::Log(format!(
                            "[exe] applied {} as Biohazard.exe",
                            patch_exe.file_name().unwrap_or_default().to_string_lossy()
                        )),
                    );
                }
                Err(e) => {
                    fail(format!("could not write patched EXE: {e}"));
                    return;
                }
            }
        } else if let Err(e) = copy_step(&cx, &tx, p, &tmp, &re1hd) {
            fail(e);
            return;
        }

        // Every phase must end with a 100% progress event, otherwise its bar
        // stays blank in the UI (the Mediakite branch copies a single exe and
        // never reaches copy_step's own Progress(p, 1.0)).
        send(&tx, &cx, Event::Progress(p, 1.0));

        let _ = fsutil::remove_dir_all_including_ro(&tmp);
        send(&tx, &cx, Event::Log(format!("[mod] {name} applied")));
    }

    // ---- Phase 9: FMV pack (hires -> root, movie -> JPN) ----
    send(&tx, &cx, Event::Phase(9));
    send(&tx, &cx, Event::Status("Integrating FMV pack...".into()));
    let tmp = work_dir("fmv");
    clear_dir(&tmp);
    if let Err(e) = unpack(&cx, &tx, &seven, &mods.fmv, &tmp) {
        fail(e);
        return;
    }

    let mut saw_hires = false;
    let mut saw_movie = false;
    if let Ok(true) = copy_subfolder(&cx, &tx, 9, &tmp, "hires", &re1hd) {
        saw_hires = true;
    }
    let movie_dst = re1hd.join("JPN").join("movie");
    if let Ok(true) = copy_subfolder(&cx, &tx, 9, &tmp, "movie", &movie_dst) {
        saw_movie = true;
    }
    let _ = fsutil::remove_dir_all_including_ro(&tmp);

    if !saw_hires && !saw_movie {
        fail("No 'hires' or 'movie' folder found inside the FMV pack.".into());
        return;
    }
    send(&tx, &cx, Event::Log("[fmv] hires + movie applied".into()));
    send(&tx, &cx, Event::Progress(9, 1.0));

    // ---- Phase 10: AMD GPU fix (dgVoodoo compatibility shim) ----
    // Only applied when an AMD-family GPU is detected; NVIDIA/Intel are
    // left completely untouched.
    send(&tx, &cx, Event::Phase(10));
    send(&tx, &cx, Event::Status("Detecting GPU vendor...".into()));
    if !amd_gpu_present() {
        send(&tx, &cx, Event::Log("[gpu] no AMD GPU detected - skipping dgVoodoo fix.".into()));
    } else if mods.amd_fix.as_os_str().is_empty() {
        send(&tx, &cx, Event::Log("[gpu] AMD GPU detected but no dgVoodoo archive found - skipping.".into()));
    } else {
        send(&tx, &cx, Event::Status("AMD GPU detected - applying dgVoodoo fix...".into()));
        let tmp = work_dir("amd_fix");
        clear_dir(&tmp);
        if let Err(e) = unpack(&cx, &tx, &seven, &mods.amd_fix, &tmp) {
            fail(e);
            return;
        }
        // The archive ships the three files inside a "dgVoodoo AMD fix" folder.
        match find_named(&tmp, &["dgVoodoo AMD fix"], 1) {
            Some(src) => {
                if let Err(e) = copy_step(&cx, &tx, 10, &src, &re1hd) {
                    fail(e);
                    return;
                }
                send(&tx, &cx, Event::Log("[gpu] dgVoodoo AMD fix (D3DImm.dll, dgVoodoo.conf, re_ddraw.dll) applied".into()));
            }
            None => {
                fail("AMD GPU detected but the dgVoodoo fix folder was not found in the archive.".into());
                return;
            }
        }
        let _ = fsutil::remove_dir_all_including_ro(&tmp);
    }
    send(&tx, &cx, Event::Progress(10, 1.0));

    // ---- Phase 11: final check ----
    send(&tx, &cx, Event::Phase(11));
    send(&tx, &cx, Event::Status("All archives applied. Verifying build...".into()));

    let mut found_any = false;

    if re1hd.join("Biohazard.exe").is_file() {
        found_any = true;
    }
    if re1hd.join("hires").is_dir() {
        found_any = true;
    }
    if re1hd.join("ddraw.dll").is_file() {
        found_any = true;
    }
    if re1hd.join("bio1hd.asi").is_file() {
        found_any = true;
    }
    if re1hd.join("JPN").join("movie").is_dir() {
        found_any = true;
    }
    if re1hd.join("JPN").join("sound").is_dir() {
        found_any = true;
    }

    send(&tx, &cx, Event::Progress(11, 1.0));
    if found_any {
        send(
            &tx,
            &cx,
            Event::Status(format!("BUILD COMPLETE - RE1HD ready at {}", re1hd.display())),
        );
        send(&tx, &cx, Event::Log(format!("[done] output: {}", re1hd.display())));
    } else {
        send(
            &tx,
            &cx,
            Event::Status("Build finished but expected files were not found.".into()),
        );
        send(&tx, &cx, Event::Log("[warn] expected output files missing".into()));
    }
    send(&tx, &cx, Event::Done);

    std::thread::spawn(beep_triple);
}