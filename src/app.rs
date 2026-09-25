use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;
use eframe::egui::{Color32, FontId, Margin, RichText, Stroke};

use crate::embedded;
use crate::pipeline::{self, Event, ModPaths, STEP_NAMES};

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/VT323-Regular.ttf");

const BG: Color32 = Color32::from_rgb(10, 12, 26);
const PANEL: Color32 = Color32::from_rgb(18, 22, 44);
const PANEL_BORDER: Color32 = Color32::from_rgb(58, 70, 130);
const CYAN: Color32 = Color32::from_rgb(64, 224, 208);
const GREEN: Color32 = Color32::from_rgb(0, 255, 128);
const RED: Color32 = Color32::from_rgb(255, 80, 80);
const AMBER: Color32 = Color32::from_rgb(255, 200, 80);
const TEXT: Color32 = Color32::from_rgb(220, 228, 255);

struct PrepState {
    payloads: Vec<embedded::Payload>,
    frac: f32,
    rx: mpsc::Receiver<Event>,
}

pub struct AppState {
    iso: Option<PathBuf>,
    mods: Option<ModPaths>,
    embedded: bool,
    prep: Option<PrepState>,
    music_files: Vec<PathBuf>,
    music: Option<crate::music::MusicPlayer>,
    running: bool,
    done: bool,
    phases: [f32; 12],
    current_phase: Option<usize>,
    status: String,
    error: Option<String>,
    log: Vec<String>,
    rx: Option<mpsc::Receiver<Event>>,
    /// True while the "clear temp files on exit?" prompt is showing.
    confirm_exit: bool,
    /// Set once the user answers the prompt so we stop intercepting the close
    /// request and let the window actually shut down.
    allow_close: bool,
}

impl AppState {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        set_visuals(&cc.egui_ctx);

        let mut app = Self {
            iso: None,
            mods: None,
            embedded: false,
            prep: None,
            music_files: Vec::new(),
            music: None,
            running: false,
            done: false,
            phases: [0.0; 12],
            current_phase: None,
            status: "AWAITING DISC IMAGE".to_string(),
            error: None,
            log: Vec::new(),
            rx: None,
            confirm_exit: false,
            allow_close: false,
        };

        match embedded::scan() {
            Ok(Some(payloads)) => {
                let totals: f32 = payloads.iter().map(|p| p.size as f32).sum();
                app.add_log(&format!(
                    "[bundle] this copy carries {:.2} GiB of embedded archives.",
                    totals / 1073741824.0
                ));
                if embedded::all_cached(&payloads) {
                    let bundle = embedded::resolve(&payloads);
                    if let Some(m) = bundle.mods {
                        app.mods = Some(m);
                        app.embedded = true;
                        app.add_log("[bundle] embedded archives extracted in temp and ready.");
                    }
                    if !bundle.music.is_empty() {
                        app.music_files = bundle.music;
                    }
                } else {
                    let (tx, rx) = mpsc::channel();
                    let cx = cc.egui_ctx.clone();
                    let pl = payloads.clone();
                    std::thread::spawn(move || {
                        let result = embedded::extract_all(&pl, &mut |done, total| {
                            let _ = tx.send(Event::Prep(done as f32 / total.max(1) as f32));
                            cx.request_repaint();
                        });
                        if let Err(e) = result {
                            let _ = tx.send(Event::Error(format!(
                                "embedded extraction failed: {e}"
                            )));
                        }
                        let _ = tx.send(Event::Done);
                    });
                    app.status = "PREPARING EMBEDDED ARCHIVES...".to_string();
                    app.prep = Some(PrepState {
                        payloads,
                        frac: 0.0,
                        rx,
                    });
                }
            }
            Ok(None) => {
                app.add_log("[bundle] not packed - searching for mods next to the disc image.");
            }
            Err(e) => {
                app.add_log(&format!("[bundle] {e}"));
            }
        }

        app
    }

    fn poll_prep(&mut self) {
        let Some(mut state) = self.prep.take() else {
            return;
        };
        let mut failed: Option<String> = None;
        while let Ok(ev) = state.rx.try_recv() {
            match ev {
                Event::Prep(f) => state.frac = f,
                Event::Log(l) => self.add_log(&l),
                Event::Error(e) => failed = Some(e),
                _ => {}
            }
        }
        if let Some(e) = failed {
            self.error = Some(e);
            self.status = "EMBEDDED PREP FAILED".to_string();
            return;
        }
        if state.frac >= 1.0 {
            let bundle = embedded::resolve(&state.payloads);
            if let Some(m) = bundle.mods {
                self.mods = Some(m);
                self.embedded = true;
                self.add_log("[bundle] embedded archives extracted to temp.");
            }
            if !bundle.music.is_empty() {
                self.music_files = bundle.music;
            }
            self.add_log("[bundle] embedded archives ready.");
        } else {
            self.prep = Some(state);
        }
    }

    fn draw_splash(&mut self, ctx: &egui::Context) {
        let frac = self.prep.as_ref().map(|p| p.frac).unwrap_or(0.0);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(Margin::same(30.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(90.0);
                    ui.label(RichText::new("RE1HD").size(64.0).color(CYAN).strong());
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new("PREPARING EMBEDDED ARCHIVES")
                            .size(30.0)
                            .color(AMBER)
                            .strong(),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("unpacking the bundled mods into a temp folder - one time only")
                            .size(18.0)
                            .color(TEXT),
                    );
                    ui.add_space(18.0);
                    let bar = egui::ProgressBar::new(frac)
                        .desired_width(420.0)
                        .fill(AMBER)
                        .text(format!("{:.0}%", frac * 100.0));
                    ui.add(bar);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("{:.2} GiB total", {
                            let t: f32 = self
                                .prep
                                .as_ref()
                                .map(|p| p.payloads.iter().map(|x| x.size as f32).sum())
                                .unwrap_or(0.0);
                            t / 1073741824.0
                        }))
                        .size(16.0)
                        .color(Color32::from_rgb(120, 132, 180)),
                    );
                });
            });
        ctx.request_repaint();
    }

    fn add_log(&mut self, line: &str) {
        self.log.push(line.to_string());
        if self.log.len() > 500 {
            self.log.drain(..self.log.len() - 500);
        }
    }

    fn pick_iso(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("ISO disc image", &["iso", "img", "bin"])
            .pick_file();
        if let Some(p) = picked {
            self.set_iso(p);
        }
    }

    fn set_iso(&mut self, path: PathBuf) {
        if path.is_file() {
            self.iso = Some(path.clone());
            self.done = false;
            if !self.embedded {
                let dir = path.parent();
                if let Some(d) = dir {
                    let m = find_mods_next_to(d);
                    if self.mods.is_none() || m_is_complete(&m, &self.mods.as_ref().unwrap()) {
                        self.mods = Some(m);
                    }
                }
            }
        }
    }

    fn browse_mod(&mut self, slot: usize) {
        let filt = match slot {
            0 | 1 | 2 | 3 => ("Texture pack", &["zip", "7z"][..]),
            4 => ("REbirth DLLs", &["7z", "zip"][..]),
            5 => ("Mediakite EXE", &["7z"][..]),
            6 => ("HQ audio mod", &["zip", "7z"][..]),
            7 => ("FMV pack", &["zip", "7z"][..]),
            _ => ("AMD GPU dgVoodoo fix", &["zip", "7z"][..]),
        };
        let picked = rfd::FileDialog::new()
            .add_filter(filt.0, filt.1)
            .pick_file();
        if let Some(p) = picked {
            if let Some(m) = self.mods.as_mut() {
                match slot {
                    0 => m.pack1 = p,
                    1 => m.pack2 = p,
                    2 => m.pack3 = p,
                    3 => m.pack4 = p,
                    4 => m.rebirth = p,
                    5 => m.exe_patch = p,
                    6 => m.audio = p,
                    7 => m.fmv = p,
                    _ => m.amd_fix = p,
                }
            }
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let iso = match self.iso.clone() {
            Some(iso) => iso,
            None => return,
        };
        if self.running {
            return;
        }
        let mods = self.mods.clone().unwrap_or_else(|| ModPaths {
            pack1: PathBuf::new(),
            pack2: PathBuf::new(),
            pack3: PathBuf::new(),
            pack4: PathBuf::new(),
            rebirth: PathBuf::new(),
            exe_patch: PathBuf::new(),
            audio: PathBuf::new(),
            fmv: PathBuf::new(),
            amd_fix: PathBuf::new(),
        });

        self.running = true;
        self.done = false;
        self.error = None;
        self.phases = [0.0; 12];
        self.current_phase = Some(0);
        self.status = "READY".to_string();
        self.add_log(&format!("[start] disc image: {}", iso.display()));
        if self.embedded {
            self.add_log("[mod] all archives come from the embedded bundle.");
        } else {
            if !mods.pack1.as_os_str().is_empty() {
                self.add_log(&format!("[mod] Team X pack: {}", mods.pack1.display()));
            }
            if !mods.pack2.as_os_str().is_empty() {
                self.add_log(&format!("[mod] Seamless HD pack: {}", mods.pack2.display()));
            }
            if !mods.pack3.as_os_str().is_empty() {
                self.add_log(&format!("[mod] SHDP patch: {}", mods.pack3.display()));
            }
            if !mods.pack4.as_os_str().is_empty() {
                self.add_log(&format!("[mod] RE-ENHANCE pack: {}", mods.pack4.display()));
            }
            if !mods.rebirth.as_os_str().is_empty() {
                self.add_log(&format!("[mod] REbirth DLLs: {}", mods.rebirth.display()));
            }
            if !mods.exe_patch.as_os_str().is_empty() {
                self.add_log(&format!("[mod] Mediakite EXE: {}", mods.exe_patch.display()));
            }
            if !mods.audio.as_os_str().is_empty() {
                self.add_log(&format!("[mod] HQ audio mod: {}", mods.audio.display()));
            }
            if !mods.fmv.as_os_str().is_empty() {
                self.add_log(&format!("[mod] FMV pack: {}", mods.fmv.display()));
            }
            if !mods.amd_fix.as_os_str().is_empty() {
                self.add_log(&format!("[mod] AMD GPU fix: {}", mods.amd_fix.display()));
            }
        }

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let cx = ctx.clone();
        std::thread::spawn(move || {
            pipeline::run_pipeline(cx, tx, iso, mods);
        });
    }

    fn poll_events(&mut self) {
        let mut rx_opt = std::mem::take(&mut self.rx);
        if let Some(rx) = &mut rx_opt {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    Event::Log(l) => self.add_log(&l),
                    Event::Phase(p) => self.current_phase = Some(p),
                    Event::Progress(p, f) => {
                        self.phases[p] = self.phases[p].max(f);
                        self.current_phase = Some(p);
                    }
                    Event::Status(s) => self.status = s,
                    Event::Prep(_) => {}
                    Event::Error(e) => {
                        self.error = Some(e.clone());
                        self.add_log(&format!("[ERROR] {e}"));
                        self.running = false;
                    }
                    Event::Done => {
                        self.running = false;
                        self.done = true;
                    }
                }
            }
        }
        self.rx = rx_opt;
    }

    /// Everything the builder unpacks lands under the system temp folder:
    /// the embedded payload cache plus any leftover per-mod work folders.
    fn temp_targets() -> Vec<PathBuf> {
        let mut out = vec![embedded::embedded_root()];
        if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir()
                    && p.file_name()
                        .map(|n| n.to_string_lossy().starts_with("re1hd_build_"))
                        .unwrap_or(false)
                {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Delete the temp payload cache and build folders. Returns the number of
    /// bytes reclaimed so the prompt can show something meaningful.
    fn clear_temp_files(&mut self) -> u64 {
        let mut freed = 0u64;
        let mut failed = 0usize;
        for p in Self::temp_targets() {
            if !p.exists() {
                continue;
            }
            freed += crate::fsutil::dir_size(&p).unwrap_or(0);
            if crate::fsutil::remove_dir_all_including_ro(&p).is_err() {
                failed += 1;
            }
        }
        if failed > 0 {
            self.add_log(&format!(
                "[exit] temp cleanup: {failed} folder(s) could not be fully removed"
            ));
        } else {
            self.add_log(&format!(
                "[exit] temp files cleared ({:.2} GiB reclaimed)",
                freed as f32 / 1073741824.0
            ));
        }
        freed
    }

    /// "Would you like to clear the temporary files?" - Yes removes them,
    /// No keeps them on disk. Either way the app then closes.
    fn draw_exit_prompt(&mut self, ctx: &egui::Context) {
        let mut clear = false;
        let mut keep = false;

        let pending: Vec<PathBuf> = Self::temp_targets()
            .into_iter()
            .filter(|p| p.exists())
            .collect();
        let reclaimable: u64 = pending
            .iter()
            .map(|p| crate::fsutil::dir_size(p).unwrap_or(0))
            .sum();

        egui::Window::new("Exit RE1HD Builder")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(430.0);
                ui.label(
                    RichText::new("Would you like to clear the temporary files?")
                        .size(21.0)
                        .color(CYAN)
                        .strong(),
                );
                ui.add_space(6.0);
                if reclaimable > 0 {
                    ui.label(
                        RichText::new(format!(
                            "This frees up {:.2} GiB from the temp folder.",
                            reclaimable as f32 / 1073741824.0
                        ))
                        .size(17.0)
                        .color(TEXT),
                    );
                } else {
                    ui.label(
                        RichText::new("There are no temporary files to clear.")
                            .size(17.0)
                            .color(TEXT),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("YES - clear temp files").size(18.0).color(Color32::BLACK),
                            )
                            .fill(GREEN),
                        )
                        .clicked()
                    {
                        clear = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("NO - keep them").size(18.0).color(Color32::BLACK),
                            )
                            .fill(AMBER),
                        )
                        .clicked()
                    {
                        keep = true;
                    }
                });
            });

        if clear {
            self.clear_temp_files();
            self.confirm_exit = false;
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if keep {
            self.add_log("[exit] temp files kept.");
            self.confirm_exit = false;
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Intercept the window close (X / Alt+F4) so we can offer to clear the
        // temp files. The close is cancelled and re-issued once the user picks.
        if !self.allow_close && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_exit = true;
        }

        self.poll_prep();
        if self.prep.is_some() {
            self.draw_splash(ctx);
            return;
        }

        if self.confirm_exit {
            self.draw_exit_prompt(ctx);
            return;
        }

        if self.music.is_none() && !self.music_files.is_empty() {
            let files = self.music_files.clone();
            match crate::music::MusicPlayer::new(files) {
                Ok(mp) => {
                    let title = mp.now_playing();
                    self.music = Some(mp);
                    self.add_log(&format!("[music] chiptune player started - \"{title}\""));
                }
                Err(e) => self.add_log(&format!("[music] {e}")),
            }
        }

        self.poll_events();

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            for d in dropped {
                let lower = d.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if lower == "iso" || lower == "img" || lower == "bin" {
                    self.set_iso(d);
                }
            }
        }

        let panel = egui::Frame::none()
            .fill(BG)
            .inner_margin(Margin::same(22.0));

        egui::CentralPanel::default()
            .frame(panel)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        self.draw_header(ui);
                        self.draw_music(ui);
                        ui.add_space(14.0);
                        self.draw_iso_row(ui);
                        ui.add_space(14.0);
                        self.draw_mods(ui);
                        ui.add_space(14.0);
                        self.draw_steps(ui);
                        ui.add_space(14.0);
                        self.draw_log(ui);
                        ui.add_space(12.0);
                        self.draw_footer(ctx, ui);
                    });
            });
    }
}

impl AppState {
    fn frame() -> egui::Frame {
        egui::Frame::none()
            .fill(PANEL)
            .stroke(Stroke::new(1.5_f32, PANEL_BORDER))
            .inner_margin(Margin::same(12.0))
    }

    fn draw_music(&mut self, ui: &mut egui::Ui) {
        let Some(mp) = &mut self.music else {
            return;
        };
        ui.add_space(8.0);
        AppState::frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("> MUSIC").size(18.0).color(AMBER).strong());
                ui.add_space(6.0);
                let title = mp.now_playing();
                ui.label(
                    RichText::new(if title.is_empty() {
                        "now playing".to_string()
                    } else {
                        format!("\"{}\"", title)
                    })
                    .size(18.0)
                    .color(TEXT),
                );
                ui.label(
                    RichText::new(format!("[{} tracks]", mp.track_count()))
                        .size(14.0)
                        .color(Color32::from_rgb(110, 118, 160)),
                );
                let next_btn = egui::Button::new(RichText::new("[ >> NEXT ]").size(16.0)).fill(PANEL_BORDER);
                if ui.add(next_btn).clicked() {
                    mp.next();
                }
                let mute_label = if mp.is_muted() {
                    "[ UNMUTE ]"
                } else {
                    "[ MUTE ]"
                };
                let mute_btn = egui::Button::new(RichText::new(mute_label).size(16.0)).fill(PANEL_BORDER);
                if ui.add(mute_btn).clicked() {
                    mp.toggle_mute();
                }
            });
        });
    }

    fn draw_header(&mut self, ui: &mut egui::Ui) {
        AppState::frame().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("RE1HD")
                        .size(52.0)
                        .color(CYAN)
                        .strong(),
                );
                ui.label(
                    RichText::new("biohazard 1 (jp sourcenext / mediakite) mod builder")
                        .size(24.0)
                        .color(TEXT),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("[ drDOOM69GAMING ]")
                        .size(16.0)
                        .color(Color32::from_rgb(120, 132, 180)),
                );
            });
        });
    }

    fn draw_iso_row(&mut self, ui: &mut egui::Ui) {
        AppState::frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let lit = self.iso.is_some();
                let (c, ring) = if lit { (GREEN, GREEN) } else { (Color32::from_rgb(60, 64, 90), Color32::from_rgb(120, 122, 150)) };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 9.0, Color32::from_rgb(0, 0, 0));
                ui.painter().circle_filled(rect.center(), 6.5, c);
                ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(1.5_f32, ring));

                ui.label(
                    RichText::new("DISC IMAGE")
                        .size(22.0)
                        .color(if lit { GREEN } else { AMBER })
                        .strong(),
                );
                ui.add_space(8.0);

                let label = self
                    .iso
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "no disc loaded - type a path or drop the .iso  >".to_string());
                let mut line = label.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut line)
                        .font(FontId::monospace(18.0))
                        .desired_width((ui.available_width() - 220.0).max(120.0))
                        .text_color(TEXT),
                );
                if line != label {
                    let p = PathBuf::from(line.trim());
                    if p.is_file() {
                        self.set_iso(p);
                    }
                }

                if ui
                    .add(egui::Button::new(RichText::new("[ BROWSE ]").size(20.0)).fill(PANEL_BORDER))
                    .clicked()
                {
                    self.pick_iso();
                }

                ui.label(
                    RichText::new("drag & drop also works")
                        .size(15.0)
                        .color(Color32::from_rgb(110, 118, 160)),
                );
            });
        });
    }

    fn draw_mods(&mut self, ui: &mut egui::Ui) {
        AppState::frame().show(ui, |ui| {
            ui.label(
                RichText::new("MOD ARCHIVES - load order is fixed, contents overwrite")
                    .size(20.0)
                    .color(CYAN)
                    .strong(),
            );
            ui.add_space(6.0);

            let rows: [(&str, &str, fn(&ModPaths) -> &PathBuf); 9] = [
                ("01", "Team X  HD textures  pack 1", |m| &m.pack1),
                ("02", "SEAMLESS HD textures pack 2", |m| &m.pack2),
                ("03", "SEAMLESS HD textures patch", |m| &m.pack3),
                ("04", "RE-ENHANCE textures pack 4", |m| &m.pack4),
                ("05", "CLAssic REbirth DLLs", |m| &m.rebirth),
                ("06", "Mediakite 1.01 EXE", |m| &m.exe_patch),
                ("07", "HQ audio mod (voice & sound)", |m| &m.audio),
                ("08", "RE-ENHANCE FMV pack  (hires+movie)", |m| &m.fmv),
                ("09", "AMD GPU dgVoodoo fix  (AMD only)", |m| &m.amd_fix),
            ];

            for (idx, (tag, name, get)) in rows.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tag).size(18.0).color(Color32::from_rgb(120, 132, 180)));
                    ui.label(RichText::new(name).size(18.0).color(TEXT).weak());
                    ui.add_space(4.0);

                    let present = self
                        .mods
                        .as_ref()
                        .map(|m| get(m).is_file() || self.embedded)
                        .unwrap_or(false);
                    let dot = if present { GREEN } else { Color32::from_rgb(90, 80, 40) };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, dot);

                    if self.embedded {
                        ui.label(RichText::new("embedded in this app").size(15.0).color(GREEN));
                    } else {
                        let mut text = self
                            .mods
                            .as_ref()
                            .map(|m| get(m).display().to_string())
                            .unwrap_or_else(|| "(auto-find next to the disc image)".to_string());
                        if self
                            .mods
                            .as_ref()
                            .map(|m| get(m).as_os_str().is_empty())
                            .unwrap_or(true)
                        {
                            text = "(auto-find next to the disc image)".to_string();
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .font(FontId::monospace(15.0))
                                .desired_width((ui.available_width() - 96.0).max(120.0))
                                .text_color(TEXT),
                        );
                        if ui
                            .add(
                                egui::Button::new(RichText::new("[ ... ]").size(18.0))
                                    .fill(PANEL_BORDER),
                            )
                            .clicked()
                        {
                            self.browse_mod(idx);
                        }
                    }
                });
            }
        });
    }

    fn draw_steps(&mut self, ui: &mut egui::Ui) {
        AppState::frame().show(ui, |ui| {
            ui.label(
                RichText::new("BUILD SEQUENCE")
                    .size(20.0)
                    .color(CYAN)
                    .strong(),
            );
            ui.add_space(6.0);

            egui::Grid::new("steps")
                .num_columns(4)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for (i, name) in STEP_NAMES.iter().enumerate() {
                        let frac = self.phases[i];
                        let is_active = self.current_phase == Some(i) && self.running;
                        let is_done = frac >= 1.0;

                        let bullet = if is_done {
                            RichText::new("OK").size(16.0).color(GREEN)
                        } else if is_active {
                            RichText::new(">>").size(16.0).color(AMBER)
                        } else {
                            RichText::new("  ").size(16.0).color(Color32::from_rgb(110, 118, 160))
                        };
                        ui.label(bullet);

                        let name_color = if is_done {
                            Color32::from_rgb(140, 200, 170)
                        } else if is_active {
                            AMBER
                        } else {
                            TEXT
                        };
                        ui.label(RichText::new(*name).size(17.0).color(name_color));

                        let pb = egui::ProgressBar::new(frac)
                            .desired_width((ui.available_width() - 60.0).max(120.0))
                            .fill(if is_done { GREEN } else if is_active { AMBER } else { Color32::from_rgb(60, 70, 110) })
                            .text(
                                if fraction_visible(frac) {
                                    format!("{:.0}%", frac * 100.0)
                                } else {
                                    String::new()
                                },
                            );
                        ui.add(pb);

                        ui.label(
                            RichText::new(if is_done { "DONE" } else { "" })
                                .size(15.0)
                                .color(GREEN),
                        );
                        ui.end_row();
                    }
                });
        });
    }

    fn draw_log(&mut self, ui: &mut egui::Ui) {
        AppState::frame().show(ui, |ui| {
            ui.label(RichText::new("EVENT LOG").size(20.0).color(CYAN).strong());
            ui.add_space(4.0);
            let mut log_text = self.log.join("\n");
            ui.add(
                egui::TextEdit::multiline(&mut log_text)
                    .font(FontId::monospace(16.0))
                    .desired_rows(10)
                    .desired_width(f32::INFINITY)
                    .text_color(TEXT)
                    .interactive(false),
            );
        });
    }

    fn draw_footer(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(PANEL)
            .stroke(Stroke::new(1.5_f32, PANEL_BORDER))
            .inner_margin(Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let status_color = if self.error.is_some() {
                        RED
                    } else if self.running {
                        AMBER
                    } else if self.done {
                        GREEN
                    } else {
                        TEXT
                    };
                    ui.label(
                        RichText::new(&self.status)
                            .size(20.0)
                            .color(status_color)
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready = self.iso.is_some() && !self.running;
                        let label = if self.running {
                            "WORKING..."
                        } else {
                            "READY"
                        };
                        let btn = egui::Button::new(RichText::new(label).size(24.0).color(Color32::BLACK))
                            .fill(if self.done { AMBER } else { GREEN })
                            .stroke(Stroke::new(1.5_f32, Color32::from_rgb(120, 255, 190)));
                        if ui.add_enabled(ready, btn).clicked() {
                            self.start(ctx);
                        }
                    });
                });
            });
    }
}

fn fraction_visible(f: f32) -> bool {
    f > 0.0 && f < 1.0
}

fn m_is_complete(new: &ModPaths, old: &ModPaths) -> bool {
    [
        (&new.pack1, &old.pack1),
        (&new.pack2, &old.pack2),
        (&new.pack3, &old.pack3),
        (&new.pack4, &old.pack4),
        (&new.rebirth, &old.rebirth),
        (&new.exe_patch, &old.exe_patch),
        (&new.audio, &old.audio),
        (&new.fmv, &old.fmv),
        (&new.amd_fix, &old.amd_fix),
    ]
    .iter()
    .all(|(n, o)| !n.as_os_str().is_empty() || !o.as_os_str().is_empty())
}

fn candidate(ext: &[&str], hay: &[String]) -> Option<PathBuf> {
    for n in hay {
        let lower = n.to_lowercase();
        if ext.iter().any(|e| lower.ends_with(&format!(".{e}"))) {
            return Some(PathBuf::from(n));
        }
    }
    None
}

fn find_mods_next_to(dir: &Path) -> ModPaths {
    let mut files: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = vec![dir.to_path_buf()];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                dirs.push(e.path());
            }
        }
    }
    for d in &dirs {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    files.push(e.path().display().to_string());
                }
            }
        }
    }

    let pick = |ext: &[&str], needle: &[&str]| -> PathBuf {
        for n in &files {
            let lower = n.to_lowercase();
            let has_ext = ext.iter().any(|e| lower.ends_with(&format!(".{e}")));
            let has_needle = needle.iter().all(|s| lower.contains(s));
            if has_ext && has_needle {
                return PathBuf::from(n);
            }
        }
        candidate(ext, &files).unwrap_or_else(PathBuf::new)
    };

    ModPaths {
        pack1: pick(&["zip", "7z"], &["hd_mod"]),
        pack2: pick(&["zip", "7z"], &["shdp_1.1"]),
        pack3: pick(&["zip", "7z"], &["shdp", "patch"]),
        pack4: pick(&["zip", "7z"], &["enhance", "v2.0"]),
        rebirth: pick(&["7z", "zip"], &["re1cr"]),
        exe_patch: pick(&["7z", "zip"], &["mediakite"]),
        audio: pick(&["zip", "7z"], &["audio"]),
        fmv: pick(&["zip", "7z"], &["fmv"]),
        amd_fix: pick(&["zip", "7z"], &["dgvoodoo"]),
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "vt323".to_owned(),
        egui::FontData::from_static(FONT_BYTES).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "vt323".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn set_visuals(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.faint_bg_color = PANEL;
    v.extreme_bg_color = PANEL;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgb(40, 80, 130);
    v.selection.stroke = Stroke::new(1.0_f32, CYAN);

    let mut style = (*ctx.style()).clone();
    style.visuals = v;
    style.spacing.interact_size.y = 26.0;
    ctx.set_style(style);
}