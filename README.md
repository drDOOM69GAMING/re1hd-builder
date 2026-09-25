# RE1HD Builder

Self-contained builder for the classic PC edition of Resident Evil 1 (Biohazard, SourceNext / Mediakite): feed it the disc image, it applies the community HD texture, audio, FMV and engine mod archives, and assembles a ready-to-run build folder. The packaged app embeds everything (7-Zip, all mod archives, menu music) into one portable release. No searching for files, no external tools needed.

This is the RE1 sibling of the RE2HD Builder - same music, UI and overall concept, different folders and mods.

## Download

Grab the latest release from the [Releases page](https://github.com/drDOOM69GAMING/re1hd-builder/releases). It is published as four carrier files:

- `re1hd-builder-packed.exe` (the app you run)
- `re1hd-builder-packed-part2.exe`, `-part3.exe`, `-part4.exe` (payload data)

**Download all four and keep them in the same folder.** On first launch the app extracts the embedded archives into `%TEMP%\re1hd_embedded` (about 4.87 GiB, one-time), then you can run the build. When you close the app it asks whether to clear those temp files.

## What the pipeline does

1. Extracts the disc image (any `.iso` / `.img` / `.bin`).
2. Finds the `HORR` game folder, removes the unwanted `BIOHAZARD mediakite` installer folder, and renames `HORR` to `RE1HD` next to the disc image.
3. Applies, in fixed load order (contents overwrite):

| # | Archive | What it is |
|---|---------|------------|
| 1 | `Resident_Evil_HD_mod_v20220831.zip` | Team X HD textures (hires, bio1hd.asi, jpn movie) |
| 2 | `RE_SHDP_1.1.zip` | Seamless HD textures |
| 3 | `RE_SHDP_-_Patch_1.1.zip` | Seamless HD patch |
| 4 | `RE-ENHANCE_RE1_v2.0.zip` | RE-ENHANCE textures |
| 5 | `re1cr-2020-12-06.7z` | Classic REbirth DLLs (ddraw.dll) |
| 6 | `mediakite 1.01.7z` | Patched `Biohazard.exe` |
| 7 | `RE1 PC - HQ Audio Mod (ORIGINAL) (simple install).zip` | High quality BGM + voice |
| 8 | `RE-ENHANCE_RE1_FMV-Pack_V1.1_2.zip` | FMV pack: `hires` folder -> `RE1HD\hires`, `movie` folder -> `RE1HD\JPN\movie` |
| 9 | `dgVoodoo_AMD_fix.zip` | AMD-only compatibility shim (see below) |

4. **AMD GPU fix**: detects the display adapter and, only when an AMD-family GPU is present, extracts `D3DImm.dll`, `dgVoodoo.conf` and `re_ddraw.dll` into the `RE1HD` root. NVIDIA / Intel / virtual adapters are left completely untouched.
5. Creates an empty `SaveData` folder in the `RE1HD` root (the game crashes the first time you try to save if it is missing).
6. Final check for the expected output files.

### AMD GPU detection
The vendor is detected from `Win32_VideoController` (PowerShell / WMI). A controller counts as AMD when its name contains `amd` or `radeon`, or the standalone token `ati` — so `AMD Radeon RX 6800`, `AMD Radeon(TM) Graphics` and `ATI Mobility Radeon` all match, while `NVIDIA GeForce`, `Intel(R) UHD Graphics` and lookalikes such as "Hawaii" do not. If no AMD GPU is found the step logs a skip and changes nothing.

## Features

- Drag & drop the disc image; mods auto-find next to it or are read straight from the embedded payloads
- 12-step pipeline with live per-step progress (yellow = working, green = done)
- Automatic **AMD GPU** dgVoodoo fix, applied only on AMD hardware
- Creates the `SaveData` folder so in-game saving works out of the box
- On exit, asks whether to clear the temporary files (Yes clears, No keeps them for a faster next launch)
- Self-contained release: payloads appended to the exe (and its `-partN.exe` carriers), extracted to `%TEMP%\re1hd_embedded` on first run
- Built-in chiptune menu player (MOD / XM / S3M / IT), shuffle start, volume, mute and skip
- Retro terminal styling with a startup jingle on completion

## Config reminder (important)

In the classic RE1 configuration menu, **do not enable texture filtering / smoothing** with these HD textures, otherwise the HD texture packs render blurred instead of crisp.

## Build

```sh
cargo build --release
```

## Pack into a multi-part exe release

```sh
cargo run --release --bin pack -- <input-exe> <output-name> [music-dir]
```

Example:

```sh
cargo run --release --bin pack -- target\release\re1hd-builder.exe re1hd-builder-packed.exe music
```

The pack tool embeds `tools/7z.exe`, `tools/7z.dll`, the nine mod archives (including the AMD dgVoodoo fix) and every tracker file in `music-dir`.

**Why multiple files?** Two hard limits apply:

1. **Windows** refuses to launch any executable whose file size reaches 4 GiB (`STATUS_INVALID_IMAGE_FORMAT`).
2. **GitHub** caps a single release/upload file at ~1.90 GB.

The RE1 mods alone are ~4.87 GiB and cannot fit in one runnable file, so the packer spills the payload across `-partN.exe` carriers (FitGirl-repack style) with a per-file budget of **1,800,000,000 bytes**, which keeps every output comfortably under both ceilings. The current release ships four files:

| File | Size |
|---|---|
| `re1hd-builder-packed.exe` | 0.661 GB |
| `re1hd-builder-packed-part2.exe` | 1.596 GB |
| `re1hd-builder-packed-part3.exe` | 1.111 GB |
| `re1hd-builder-packed-part4.exe` | 1.510 GB |

All files share the same payload index format, each carries its own slice of the mods, and the main exe reads them all at startup. **Keep every part in the same folder as the main exe**; the parts are data, never run them.

## Repository layout

- `src/`: app (GUI), build pipeline, archive layer, embedded-payload engine, tracker music player
- `src/bin/`: `pack` (embed payloads into an exe split across `-partN.exe` files), `headless` (no-UI end-to-end run), `partstub` (tiny PE base for the part carriers)
- `assets/`: window/exe icon, retro font, Windows resource script
- `tools/`: 7-Zip binaries used by the (re)pack step
- `music/`: tracker music embedded as the menu soundtrack

## Notes

Third-party mods and 7-Zip are the property of their respective authors. This tool is a convenience wrapper that assembles them.