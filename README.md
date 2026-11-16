# DawnStudio

**DawnStudio** is a free, open-source multiplatform Digital Audio Workstation (DAW) for **Android** and **Desktop (Linux, Windows, macOS)** written 100% in Rust under the **GNU GPLv3** license.

It combines:
1. **MorningDAW-Inspired Tactile Workflow & Aesthetic**:
   - **Smart Radial Pie Menus (`Alt+Q`, `Middle-Click`, or `Tab` on Track Menus)**: 8-slice radial pie menus designed for both touchscreens (**Hold-and-Flick** gesture) and mouse (**Click-to-Open**), with directional vector line indicators and Dawn Amber slice highlights.
   - **Studio Command Palette (`Ctrl+P` / `Ctrl+Shift+P` / `Alt+K`)**: Instant searchable launcher for switching views (`ARRANGE`, `PIANO`, `RACK`, `MIXER`), spawning Lemon Studio instruments, inserting clips, and switching studio themes.
   - **Signature Studio Themes**: Ships with **Morning Ember** (`#FF8C32` Dawn Amber over `#0C0C0E` Obsidian Carbon, active by default), **Wyvern Cyan** (`#28D7EB`), and **Dawn Gold** (`#FFBA2E`).
2. **Lemon Studio & Honeydew DSP Engine**:
   - **23+ Built-In Synths, Samplers & FX**: Includes **Lemon Trio** (3-oscillator subtractive synth), **Citrus** (6-operator FM matrix synth), **Lemon Sampler** (with `signalsmith-stretch` pitch/time-stretching), **Lemon Drum Sampler** (32 pads), **Lemon Slicer**, **SuperSaw**, **MiniSynth**, **GMSynth**, **SoundFont (`.sf2`) Player**, **Lemon Parametric EQ**, **Lemon Vocoder**, **Press** (multiband maximizer), **Pulp Filter**, **Tesla** (audio recorder & editor), **Lemon Peak/Formula Controllers**, and more.
   - **Native Project Compatibility**: Opens and saves **FL Studio (`.flp`)**, **FL Studio Mobile (`.flm`)**, **MIDI (`.mid`)**, and exports **WAV / FLAC / OGG / MP3**.
   - **Mobile & Desktop Native**: Low-latency Android audio via `cpal`/Oboe + `jni` touchscreen bridge, and desktop VST3/VST2/CLAP hosting.

---

## Quick Controls & MorningDAW Workflow

| Action | Shortcut / Gesture |
|---|---|
| **Studio Radial Pie Menu** (8 slices: *Arrange, Piano/Edit, Devices, Mixer, Add Track, New Clip, Clone, Commands*) | `Alt + Q` or **Middle Mouse Click** anywhere (or from `Ctrl+P` → **Radial Pie (`Alt+Q`)**) |
| **Track Header Radial Pie Menu** | Long-press / Right-click any Track Header, then press `Tab` to toggle between Popup List and Radial Pie mode |
| **Studio Command Palette** | `Ctrl + P`, `Ctrl + Shift + P`, or `Alt + K` |
| **Hold-and-Flick Radial Gesture** | Hold trigger button, drag past the deadzone toward a slice, and release to execute immediately |

---

## Architecture Overview

- [`src/ui/pie_menu.rs`](src/ui/pie_menu.rs) — Custom GPU-painted annular wedge Radial Pie Menu (`PieSlice<T>`, `PieResult<T>`, `show<T>()`) + `DawnQuickState` (`Ctrl+K` Command Palette and 8-slice Studio Radial Menu).
- [`src/theme.rs`](src/theme.rs) — Studio themes including `MORNING_EMBER_JSON`, `WYVERN_CYAN_JSON`, and `DAWN_GOLD_JSON`.
- [`src/ui/main_screen.rs`](src/ui/main_screen.rs) — Multi-track Arrangement Playlist, Transport Bar, Piano/Drum/Chord panels, and Track Header menus.
- [`src/ui/piano_roll/`](src/ui/piano_roll/) — Multi-touch Piano Roll editor with scale snapping, velocity lanes, and note generation.
- [`src/ui/rack/`](src/ui/rack/) — Modular Instrument & FX Channel Rack with inline custom cards for Lemon Studio plugins.
- [`src/ui/mixer.rs`](src/ui/mixer.rs) — Multi-channel Mixer with per-channel routing, peak meters, mute/solo, and master bus.
- [`vendor/lemon-studio/`](vendor/lemon-studio/) — Vendored crates from **Lemon Studio** (`lemon_plugins`, `lemon_ParametricEQ`, `lemon_Press`, `lemon_PulpFilter`, `lemon_Tesla`, `flp`).
- [`crates/honeydew-audio/`](crates/honeydew-audio/) — Real-time lock-free audio graph, synth engines, and DSP processors.
- [`android/`](android/) — Native Android Gradle/NDK project (`dev.dawnstudio.app`).

---

## Building from Source

### Linux Desktop

```bash
# Install system dependencies (Debian/Ubuntu)
sudo apt-get update && sudo apt-get install -y \
  libasound2-dev libxkbcommon-dev libwayland-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libclang-dev

# Build and run DawnStudio
cargo run --release --bin honeydew-native
```

### Android APK

```bash
# Requires Android SDK (API 36), NDK 28, and cargo-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi
cargo install cargo-ndk --version 4.1.2 --locked

# Build base APK (downloads sound packs on demand)
tools/android-dist.sh --base
```

---

## Credits & License

**DawnStudio** is licensed under the **GNU General Public License v3.0 (GPLv3)** — see [`LICENSE`](LICENSE) and [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

- **DSP Synths, Effects & FLP Crate**: Derived from [**Lemon Studio**](https://github.com/ItsAlphaNeon/Lemon-Studio) by **ItsAlphaNeon** and contributors (GPLv3).
- **Multiplatform Audio & Mobile Engine Foundation**: Derived from [**Honeydew**](https://github.com/daijro/honeydew) by **daijro** and contributors (GPLv3).
- **UI/UX Ergonomics**: Inspired by the tactile radial pie menu and dark studio workflow of **MorningDAW**.
