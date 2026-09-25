use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use re1hd_builder::{embedded, pipeline};
use re1hd_builder::pipeline::{Event, ModPaths};

fn main() {
    let iso = PathBuf::from(r"C:\Users\wayneamd\Desktop\BIOHAZARD mediakite.iso");

    let (mods, source_note) = match embedded::scan().expect("embedded scan") {
        Some(payloads) => {
            let totals: u64 = payloads.iter().map(|p| p.size).sum();
            if !embedded::all_cached(&payloads) {
                println!("[headless] extracting {:.2} GiB of embedded payloads to temp...", totals as f32 / 1073741824.0);
                embedded::extract_all(&payloads, &mut |done, total| {
                    println!("  prep {:.0}%", done as f32 / total as f32 * 100.0);
                })
                .expect("embedded extraction");
            }
            let bundle = embedded::resolve(&payloads);
            match bundle.mods {
                Some(m) => (m, "embedded payloads".to_string()),
                None => {
                    eprintln!("[headless] embedded payloads present but incomplete");
                    std::process::exit(1);
                }
            }
        }
        None => {
            let m = ModPaths {
                pack1: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\1\Resident_Evil_HD_mod_v20220831.zip",
                ),
                pack2: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\2\RE_SHDP_1.1.zip",
                ),
                pack3: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\3\RE_SHDP_-_Patch_1.1.zip",
                ),
                pack4: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\4\RE-ENHANCE_RE1_v2.0.zip",
                ),
                rebirth: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\5\re1cr-2020-12-06.7z",
                ),
                exe_patch: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\6\mediakite 1.01.7z",
                ),
                audio: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\7\RE1 PC - HQ Audio Mod (ORIGINAL) (simple install).zip",
                ),
                fmv: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\8\RE-ENHANCE_RE1_FMV-Pack_V1.1_2.zip",
                ),
                amd_fix: PathBuf::from(
                    r"C:\Users\wayneamd\Desktop\mods\AMD GPU Detect\dgVoodoo_AMD_fix.zip",
                ),
            };
            (m, "direct file paths".to_string())
        }
    };

    println!("[headless] mode: {source_note}");
    println!("[headless] verifying inputs...");
    for (name, p) in [
        ("ISO", &iso),
        ("Team X pack", &mods.pack1),
        ("SHDP pack", &mods.pack2),
        ("SHDP patch", &mods.pack3),
        ("RE-ENHANCE", &mods.pack4),
        ("REbirth DLLs", &mods.rebirth),
        ("Mediakite EXE", &mods.exe_patch),
        ("HQ audio mod", &mods.audio),
        ("FMV pack", &mods.fmv),
        ("AMD GPU fix", &mods.amd_fix),
    ] {
        if p.is_file() {
            println!("  OK   {name}: {}", p.display());
        } else {
            println!("  MISS {name}: {}", p.display());
        }
    }

    let ctx = eframe::egui::Context::default();
    let (tx, rx) = mpsc::channel();

    let mut last_phase_time = Instant::now();
    let mut current_phase = usize::MAX;
    let total_started = Instant::now();

    let handle = std::thread::spawn(move || {
        pipeline::run_pipeline(ctx, tx, iso, mods);
    });

    while let Ok(ev) = rx.recv() {
        match ev {
            Event::Log(l) => println!("  [log] {l}"),
            Event::Phase(p) => {
                if current_phase != usize::MAX {
                    println!(
                        "  phase {} done in {:.1}s",
                        current_phase,
                        last_phase_time.elapsed().as_secs_f32()
                    );
                }
                current_phase = p;
                last_phase_time = Instant::now();
                println!(
                    "=== PHASE {p}: {} ===",
                    pipeline::STEP_NAMES.get(p).copied().unwrap_or("?")
                );
            }
            Event::Progress(p, f) => {
                if p != current_phase {
                    println!("  phase {p} progress {:.0}%", f * 100.0);
                } else if f > 0.01 {
                    println!("  ... {:.0}%", f * 100.0);
                }
            }
            Event::Prep(f) => println!("  [prep] {:.0}%", f * 100.0),
            Event::Status(s) => println!("  [status] {s}"),
            Event::Error(e) => {
                println!("  [ERROR] {e}");
                break;
            }
            Event::Done => {
                println!("=== DONE in {:.1}s ===", total_started.elapsed().as_secs_f32());
                break;
            }
        }
    }

    let _ = handle.join();
}