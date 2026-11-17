# miniVNA Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A native Rust + egui desktop program that replaces IG_miniVNA for the original mRS miniVNA and shows the same numbers for the same device readings.

**Architecture:** One Rust crate. Pure, UI-free modules (`calc`, `cal`, `device`, `sweep`, `store`) carry all measurement logic and are unit-tested against values computed from the original's formulas and against sweeps recorded from the real device. The egui interface (`ui`) keeps its logic in a plain `AppState` struct that is also unit-tested; the drawing code only reads that state.

**Tech Stack:** Rust (stable, edition 2021), `eframe`/`egui`, `egui_plot`, `serialport`, `rfd`, `serde` + `toml`, `image`, `directories`, `thiserror`. Use the latest released versions that `cargo add` resolves; `egui_plot` must match the `egui` version `eframe` pulls in.

**Spec:** `docs/superpowers/specs/2026-11-17-minivna-rewrite-design.md`. Reference for every formula and for the original's behaviour: `docs/reverse-engineering/ig-minivna-findings.md`. Original window layouts: `docs/reverse-engineering/forms/*.txt`. Recorded device data: `docs/captures/capture_open_2026-11-17.json`.

## Global Constraints

- No Java, no web view, no second language for the interface.
- `calc`, `cal`, `device`, `sweep` and `store` must not import anything from `ui`, `egui` or `eframe`. (`store::png` may use `image` only.)
- Measurement maths follows the original exactly, quirks included. The only permitted differences are the rows of the spec's "Deliberate differences" table, plus the negative-start rule in Task 4.
- English only, but every user-visible string goes through `ui::text::t(key)`.
- Settings, calibration and band presets live in the per-user configuration directory (`directories::ProjectDirs::from("", "", "minivna")`), never next to the program.
- Rounding to integers uses round-half-to-even (`f64::round_ties_even`), as the original's x87 rounding does.
- Frequency limits: 100 000 Hz to 180 000 000 Hz, times the extender multiplier (1, 4, 8 or 16). Samples: 10 to 1000, default 500.
- Serial: 115200 8N1, no flow control, DTR and RTS asserted, 300 ms wait after opening.
- Not in version one: old `.VNA`/`.cal` files, printing, BMP, translations, the "VNA 1" variant, the Smith chart.
- Program and crate name: `minivna`. No licence file until the user chooses one.
- Commit after every task. The directory is not yet a git repository; Task 1 creates it.

## Review Focus

1. **Typed frequency or number that is empty, out of range, reversed (start ≥ stop) or uses a decimal comma ("3,5").** Expected: a comma is accepted as a decimal point; anything invalid restores the previous value; the program never sweeps with start ≥ stop. Pinned in Task 9 (`parse_mhz`, `set_range`).
2. **Device unplugged, wrong port, or a short reply mid-sweep.** Expected: one retry, then a readable error in the status bar, the Read light off, continuous sweep stopped, and the program still usable. Pinned in Task 5 (`short_reply_retries_then_errors`) and Task 6 (`error_stops_continuous`).
3. **Corrupt, empty or older settings, calibration or sweep files.** Expected: settings and calibration fall back to defaults with a message; a bad sweep file is refused with a message; nothing panics. Pinned in Tasks 3, 7 and 8.
4. **A sweep so narrow that the whole-Hz step rounds to 0, or a start that would go below 0 with the step-bug option.** Expected: step 0 is refused with a message; the low-start case sweeps without lowering the start. Pinned in Task 4.
5. **Readings that give infinite or huge values (level exactly at the 0 dB point, SWR in the thousands, |Xs| = 0 so C is infinite) and markers left beyond the end after the sample count is reduced.** Expected: no panic, no NaN on screen ("∞" or a capped figure), markers clamped to the last point. Pinned in Task 1 (`extreme_readings_do_not_produce_nan`) and Task 10 (`markers_clamp_when_samples_shrink`).

## File Structure

```
Cargo.toml
src/main.rs                 argument parsing (--sim), starts eframe
src/lib.rs                  module declarations
src/calc/mod.rs             re-exports
src/calc/convert.rs         RawPoint, Point, convert()
src/calc/sign.rs            phase-sign estimate
src/calc/tools.rs           SWR minimum, bandwidth/Q, cable loss/length, 90° search, LC
src/cal.rs                  Calibration tables, lookup, load/save
src/device/mod.rs           Mode, DeviceError, Device<T>
src/device/protocol.rs      SweepRequest, command text, reply parsing
src/device/transport.rs     Transport trait, SerialTransport, port listing/search
src/device/sim.rs           SimTransport (simulated antenna)
src/sweep.rs                background worker, Command/Event
src/store/mod.rs            config directory helper
src/store/settings.rs       Settings, Colors, Band, velocity table, defaults
src/store/sweep_file.rs     .mvna save/load
src/store/csv.rs            ZPlots CSV
src/store/png.rs            PNG writer
src/ui/mod.rs               App (eframe::App), wiring
src/ui/text.rs              t(key) string table
src/ui/state.rs             AppState and all interface logic (tested)
src/ui/side_panel.rs        sweep controls, curve boxes, RL scale
src/ui/marker_panel.rs      marker rows, XL/Xc, M1-M2 panel, mode panels
src/ui/graph.rs             the plot, mouse handling
src/ui/menu.rs              menu bar, keyboard shortcuts
src/ui/dialogs/*.rs         port, calibrate, dds, generator, bands, colors, velocity, lc, entry
tests/data/open_capture.json   copy of the recorded sweeps
packaging/                  desktop file, icon, build scripts
.github/workflows/build.yml
```

---

### Task 1: Project scaffold and raw-to-physical conversion

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `src/main.rs`, `src/lib.rs`, `src/calc/mod.rs`, `src/calc/convert.rs`

**Interfaces:**
- Produces:
  ```rust
  // src/calc/convert.rs, re-exported from calc
  #[derive(Clone, Copy, Debug, PartialEq)] pub struct RawPoint { pub phase: u16, pub level: u16 }
  #[derive(Clone, Copy, Debug)] pub struct Point {
      pub freq_hz: f64, pub phase_deg: f64,
      pub rl_db: f64,            // clamped to >= 0
      pub re: f64, pub im: f64,  // reflection value, from the unclamped RL
      pub swr: f64, pub z_mag: f64, pub rs: f64, pub xs: f64,
      pub l_uh: f64, pub c_pf: f64,
  }
  /// raw, cal and the result all have the same length; point i is at start_hz + i*step_hz.
  pub fn convert(raw: &[RawPoint], cal: &[i16], start_hz: f64, step_hz: f64) -> Vec<Point>;
  ```

- [ ] **Step 1: Create the project.** `git init`; `cargo init --name minivna`; add `[lib]` so `src/lib.rs` and `src/main.rs` coexist; `.gitignore` with `/target`. `main.rs` prints the version for now. Run `cargo build`; expected: builds.

- [ ] **Step 2: Write the failing tests** in `src/calc/convert.rs` (`#[cfg(test)]`). Use a helper `close(a, b)` = `|a-b| <= 1e-4 * max(1, |b|)`.

  ```rust
  #[test] fn resistive_point() {           // phase_raw 0, level_raw 514, cal 0, 7 MHz
      let p = convert(&[RawPoint{phase:0, level:514}], &[0], 7e6, 0.0)[0];
      close(p.phase_deg, 0.0); close(p.rl_db, 9.986628); close(p.swr, 1.927036);
      close(p.rs, 96.351813); close(p.xs, 0.0); close(p.z_mag, 96.351813); close(p.l_uh, 0.0);
  }
  #[test] fn reactive_point() {            // 512, 514, cal 0, 7 MHz
      let p = convert(&[RawPoint{phase:512, level:514}], &[0], 7e6, 0.0)[0];
      close(p.phase_deg, 90.087977); close(p.re, -0.000486); close(p.im, 0.316715);
      close(p.rs, 40.847504); close(p.xs, 28.758747); close(p.z_mag, 49.955822);
      close(p.l_uh, 0.653871); close(p.c_pf, 790.591481);
  }
  #[test] fn calibration_is_added_to_the_raw_level() {   // 256, 600, cal -20, 14.2 MHz
      let p = convert(&[RawPoint{phase:256, level:600}], &[-20], 14.2e6, 0.0)[0];
      close(p.rl_db, 13.857595); close(p.swr, 1.508858); close(p.rs, 63.541124); close(p.xs, 19.022477);
  }
  #[test] fn negative_loss_is_clamped_after_the_maths() { // 100, 300, cal 0, 1 MHz
      let p = convert(&[RawPoint{phase:100, level:300}], &[0], 1e6, 0.0)[0];
      assert_eq!(p.rl_db, 0.0);            // shown as 0 ...
      close(p.swr, 6.822577); close(p.rs, 165.158657); close(p.xs, 166.653487); // ... but derived from -2.564692 dB
  }
  #[test] fn frequencies_follow_start_and_step() {
      let raw = [RawPoint{phase:0, level:514}; 3];
      let p = convert(&raw, &[0; 3], 1e6, 358000.0);
      assert_eq!(p[2].freq_hz, 1_716_000.0);
  }
  #[test] fn extreme_readings_do_not_produce_nan() {
      for (ph, lv) in [(1023u16, 344u16), (0, 0), (1023, 1023), (0, 344), (65535, 65535)] {
          let p = convert(&[RawPoint{phase:ph, level:lv}], &[0], 1e6, 0.0)[0];
          for v in [p.phase_deg, p.rl_db, p.re, p.im, p.swr, p.z_mag, p.rs, p.xs, p.l_uh, p.c_pf] { assert!(!v.is_nan()); }
      }
  }
  ```

- [ ] **Step 3: Run `cargo test calc::convert`.** Expected: fails to compile (`convert` not defined).

- [ ] **Step 4: Implement `convert`** using the block in the spec's "Maths" section, in that order: phase, unclamped RL, rho, SWR, re/im, D, Rs, |Xs|, |Z|, L, C, then clamp `rl_db` to ≥ 0. `c_pf` is `f64::INFINITY` when `xs == 0`; `l_uh` is 0 then. Do not add the original's 2^-100 term.

- [ ] **Step 5: Run `cargo test calc::convert`.** Expected: 6 passed.

- [ ] **Step 6: Commit** (`feat: project scaffold and reading conversion`).

---

### Task 2: Marker tools, cable maths, phase sign, LC

**Files:**
- Create: `src/calc/tools.rs`, `src/calc/sign.rs`
- Modify: `src/calc/mod.rs`

**Interfaces:**
- Consumes: `Point` from Task 1.
- Produces:
  ```rust
  // tools.rs
  pub fn swr_min_index(points: &[Point]) -> Option<usize>;      // lowest SWR < 200 over indices 1.., None if none
  pub fn max_rl_index(points: &[Point]) -> Option<usize>;        // over indices 1..
  pub fn min_rl_index(points: &[Point]) -> Option<usize>;        // over indices 1..
  pub fn bandwidth_q(f1_hz: f64, f2_hz: f64) -> Option<(f64, f64)>; // (BW Hz, Q); None when f1 == f2
  pub struct CableLoss { pub min: f64, pub max: f64, pub avg: f64 }
  pub fn cable_loss(rl1_db: f64, rl2_db: f64) -> CableLoss;
  pub fn cable_length_m(f1_hz: f64, f2_hz: f64, velocity: f64) -> Option<f64>;   // None when f1 == f2
  pub fn velocity_from_length(f1_hz: f64, f2_hz: f64, length_m: f64) -> Option<f64>;
  /// Indices of the 1st and 3rd place where phase is between 85 and 95 degrees.
  pub fn quarter_wave_crossings(points: &[Point]) -> Option<(usize, usize)>;
  /// Move idx forward by up to 10 points to the one whose phase is nearest 90 degrees.
  pub fn refine_to_90(points: &[Point], idx: usize) -> usize;
  pub fn lc_resonance(f_mhz: f64, value: f64) -> f64;            // µH in -> pF out, and the reverse
  // sign.rs
  /// true = positive (phase falling with frequency). Same length as points.
  pub fn estimate_sign(points: &[Point]) -> Vec<bool>;
  /// Negate phase_deg, xs and im where the estimate is negative.
  pub fn apply_sign(points: &mut [Point]);
  ```

- [ ] **Step 1: Write the failing tests.** A test helper `pts(phases: &[f64]) -> Vec<Point>` builds points 1 MHz apart starting at 1 MHz with the given `phase_deg` and `swr = 2.0`.

  ```rust
  #[test] fn swr_min_ignores_point_zero_and_values_of_200_or_more() {
      // swr = [1.0, 250.0, 3.0, 1.5, 9.0] -> Some(3);  [1.0, 250.0, 300.0] -> None
  }
  #[test] fn bandwidth_and_q() { let (bw, q) = bandwidth_q(7.0e6, 7.2e6).unwrap(); close(bw, 200_000.0); close(q, 35.5);
                                 assert!(bandwidth_q(7e6, 7e6).is_none()); }
  #[test] fn cable_loss_is_half_the_return_loss() { let c = cable_loss(6.0, 4.0); close(c.min, 2.0); close(c.max, 3.0); close(c.avg, 2.5); }
  #[test] fn cable_length() { close(cable_length_m(10e6, 20e6, 0.66).unwrap(), 9.855);
                              close(velocity_from_length(10e6, 20e6, 10.0).unwrap(), 0.669667);
                              assert!(cable_length_m(1e6, 1e6, 0.66).is_none()); }
  #[test] fn lc() { close(lc_resonance(7.1, 15.0), 33.497693); }
  #[test] fn crossings_found() {
      // phases: 20,60,88,92,130,170,120,90,60,20,50,89,91,140 -> first hit index 2, third hit index 11
      assert_eq!(quarter_wave_crossings(&pts(&[20.,60.,88.,92.,130.,170.,120.,90.,60.,20.,50.,89.,91.,140.])), Some((2, 11)));
      assert_eq!(quarter_wave_crossings(&pts(&[20.,60.,88.,130.])), None);
  }
  #[test] fn refine_picks_nearest_90_within_ten() { assert_eq!(refine_to_90(&pts(&[86., 88., 90.5, 93., 99.]), 0), 2); }
  #[test] fn sign_follows_slope_and_smooths_single_glitches() {
      // rising phase -> negative (false); falling -> positive (true)
      let s = estimate_sign(&pts(&[10., 20., 30., 29., 40., 50., 40., 30., 20.]));
      assert_eq!(s, vec![false, false, false, false, false, false, true, true, true]);
  }
  #[test] fn sign_handles_short_inputs() { assert_eq!(estimate_sign(&pts(&[])).len(), 0); assert_eq!(estimate_sign(&pts(&[10.])).len(), 1); }
  ```

- [ ] **Step 2: Run `cargo test calc`.** Expected: compile failure.

- [ ] **Step 3: Implement.** Formulas are in the spec's "Maths" section; exact scanning rules are in the findings file, sections 1 (sign), 5 (cable) and 6 (BW/Q, LC). Notes that the tests do not fully determine:
  - `quarter_wave_crossings`: scan forward; a "hit" is the first index with 85 < |phase| < 95; then skip forward until |phase| < 84 or > 96 before looking for the next hit; return hits 1 and 3.
  - `refine_to_90`: unlike the original, compare `| |phase| - 90 |` for the starting point too (the original's first comparison is a bug).
  - `estimate_sign`: for i ≥ 2, falling = true, rising = false, equal = previous; then for interior i, if `s[i]` differs from both neighbours and they agree, take the neighbours' value; `s[0] = s[1]`, last = previous. Index 1 takes the value of index 2.
  - `apply_sign` calls `estimate_sign`.

- [ ] **Step 4: Run `cargo test calc`.** Expected: all pass.

- [ ] **Step 5: Commit** (`feat: marker, cable, sign and LC maths`).

---

### Task 3: Calibration tables

**Files:**
- Create: `src/cal.rs`
- Modify: `src/lib.rs`, `Cargo.toml` (add `serde` with `derive`, `toml`, `thiserror`)

**Interfaces:**
- Consumes: `RawPoint`.
- Produces:
  ```rust
  pub const CAL_POINTS: usize = 1000;
  pub const CAL_ZERO: i16 = 344;
  pub const CAL_START_HZ: f64 = 100_000.0;
  pub const CAL_STEP_HZ: f64 = 179_900.0;      // round((180e6 - 100e3) / 1000)
  /// table[k] belongs to frequency CAL_START_HZ + (k + 1) * CAL_STEP_HZ.
  #[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
  pub struct Calibration { pub reflection: Option<Vec<i16>>, pub transmission: Option<Vec<i16>> }
  /// raw is the 1001 points (0..=1000) of a calibration sweep; uses points 1..=1000.
  pub fn table_from_sweep(raw: &[RawPoint]) -> Result<Vec<i16>, CalError>;
  impl Calibration {
      /// One offset per frequency. device_freqs_hz are the frequencies the DDS produced
      /// (displayed frequency divided by the extender). All zeros when that table is absent.
      pub fn offsets(&self, transmission: bool, device_freqs_hz: &[f64]) -> Vec<i16>;
      pub fn load(path: &Path) -> Result<Calibration, CalError>;   // missing file -> Ok(default)
      pub fn save(&self, path: &Path) -> Result<(), CalError>;
  }
  #[derive(thiserror::Error, Debug)] pub enum CalError { Io(..), Format(String) }
  ```

- [ ] **Step 1: Write the failing tests.**
  ```rust
  #[test] fn table_is_344_minus_level() {
      let mut raw = vec![RawPoint{phase:0, level:999}];                       // point 0, ignored
      raw.extend((0..1000).map(|k| RawPoint{phase:0, level:364 + (k % 3) as u16}));
      let t = table_from_sweep(&raw).unwrap();
      assert_eq!(t.len(), 1000); assert_eq!(&t[..3], &[-20, -21, -22]);
  }
  #[test] fn wrong_length_is_refused() { assert!(table_from_sweep(&[RawPoint{phase:0, level:0}; 500]).is_err()); }
  #[test] fn lookup_is_nearest_calibration_frequency() {
      let cal = Calibration { reflection: Some((0..1000).collect()), transmission: None };
      // 279 900 Hz is table[0]; 459 700 Hz is table[1]; halfway rounds to the nearer; out of range clamps
      assert_eq!(cal.offsets(false, &[279_900.0, 459_700.0, 300_000.0, 0.0, 500e6]), vec![0, 1, 0, 0, 999]);
      assert_eq!(cal.offsets(true, &[1e6, 2e6]), vec![0, 0]);            // absent table
  }
  #[test] fn save_then_load() { /* tempdir; a Calibration with both tables; load == saved */ }
  #[test] fn missing_file_is_default_and_garbage_is_an_error() {
      /* load(nonexistent) == Ok(default); file containing "not toml {{{" -> Err(Format);
         file with a 7-entry table -> Err(Format) */
  }
  ```
- [ ] **Step 2: Run `cargo test cal`.** Expected: compile failure.
- [ ] **Step 3: Implement.** File format is TOML with keys `reflection` and `transmission` (arrays of 1000 integers, either may be absent). `load` validates the lengths. Use `tempfile` as a dev-dependency for the tests.
- [ ] **Step 4: Run `cargo test cal`.** Expected: 5 passed.
- [ ] **Step 5: Commit** (`feat: calibration tables`).

---

### Task 4: Sweep request, command text and reply parsing

**Files:**
- Create: `src/device/mod.rs`, `src/device/protocol.rs`, `tests/data/open_capture.json` (copy of `docs/captures/capture_open_2026-11-17.json`)
- Modify: `src/lib.rs`, `Cargo.toml` (dev-dependency `serde_json`)

**Interfaces:**
- Consumes: `RawPoint`.
- Produces:
  ```rust
  // device/mod.rs
  #[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)] pub enum Mode { Reflection, Transmission }
  pub const F_MIN_HZ: u32 = 100_000; pub const F_MAX_HZ: u32 = 180_000_000;
  pub const DDS_FACTOR_DEFAULT: f64 = 10.73741824;
  // device/protocol.rs
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct SweepRequest {
      pub mode: Mode, pub start_hz: u64, pub stop_hz: u64,   // as displayed (extender included)
      pub samples: u16, pub extender: u8, pub dds_factor: f64, pub step_bug: bool,
  }
  #[derive(thiserror::Error, Debug, PartialEq)] pub enum RequestError { Range, StepZero, Samples }
  impl SweepRequest {
      pub fn validate(&self) -> Result<(), RequestError>;
      pub fn step_hz(&self) -> u64;                 // round_ties_even((stop - start) / samples)
      pub fn lowers_start(&self) -> bool;           // step_bug && start_hz >= step_hz() (see the rule below)
      pub fn command_parts(&self) -> [String; 4];   // each ends with '\r'
      pub fn reply_len(&self) -> usize;             // (samples + 2) * 4
      pub fn parse_reply(&self, reply: &[u8]) -> Vec<RawPoint>;   // samples + 1 points
      pub fn freqs_hz(&self) -> Vec<f64>;           // samples + 1 displayed frequencies
      pub fn device_freqs_hz(&self) -> Vec<f64>;    // the same divided by the extender
  }
  pub fn parse_points(bytes: &[u8]) -> Vec<RawPoint>;   // 4 bytes per point, phase then level, low byte first
  pub fn generator_parts(freq_hz: u64, extender: u8, dds_factor: f64) -> [String; 4];
  pub const PARK_PARTS: [&str; 4] = ["0\r", "10\r", "1\r", "0\r"];
  ```

**Negative-start rule (a deliberate difference, decided in this plan):** the original lowers the start by one step even when that goes below zero, and sends a negative number (for the calibration sweep it sends `-857920`). What the firmware does with that is unknown. The rewrite lowers the start only when `start_hz - step_hz >= 0`; otherwise it sends the start unchanged and still copies point 1 over point 0. Task 14 checks this on the device.

- [ ] **Step 1: Write the failing tests.**
  ```rust
  fn req(mode: Mode, start: u64, stop: u64, samples: u16) -> SweepRequest { /* extender 1, default factor, step_bug true */ }
  #[test] fn full_range_command() {
      let r = req(Mode::Reflection, 1_000_000, 180_000_000, 500);
      assert_eq!(r.step_hz(), 358_000);
      assert_eq!(r.command_parts(), ["0\r", "6893423\r", "502\r", "3843996\r"]);
      assert_eq!(r.reply_len(), 2008);
  }
  #[test] fn without_step_bug_the_start_is_not_lowered() {
      let r = SweepRequest { step_bug: false, ..req(Mode::Reflection, 1_000_000, 180_000_000, 500) };
      assert_eq!(r.command_parts(), ["0\r", "10737418\r", "502\r", "3843996\r"]);
  }
  #[test] fn transmission_narrow_sweep() {
      assert_eq!(req(Mode::Transmission, 7_000_000, 7_200_000, 100).command_parts(), ["1\r", "75140453\r", "102\r", "21475\r"]);
  }
  #[test] fn extender_divides_what_is_sent() {
      let r = SweepRequest { extender: 4, ..req(Mode::Reflection, 430_000_000, 440_000_000, 200) };
      assert_eq!(r.command_parts(), ["0\r", "1154138243\r", "202\r", "134218\r"]);
      assert_eq!(r.freqs_hz()[1], 430_050_000.0); assert_eq!(r.device_freqs_hz()[1], 107_512_500.0);
  }
  #[test] fn start_is_not_lowered_below_zero() {
      let r = req(Mode::Reflection, 100_000, 180_000_000, 1000);        // step 179 900 > start
      assert!(!r.lowers_start());
      assert_eq!(r.command_parts(), ["0\r", "1073742\r", "1002\r", "1931662\r"]);
  }
  #[test] fn validation() {
      assert_eq!(req(Mode::Reflection, 7_000_000, 7_000_004, 500).validate(), Err(RequestError::StepZero)); // step rounds to 0
      assert_eq!(req(Mode::Reflection, 9_000_000, 7_000_000, 500).validate(), Err(RequestError::Range));
      assert_eq!(req(Mode::Reflection, 50_000, 7_000_000, 500).validate(), Err(RequestError::Range));
      assert_eq!(req(Mode::Reflection, 1_000_000, 181_000_000, 500).validate(), Err(RequestError::Range));
      assert_eq!(req(Mode::Reflection, 1_000_000, 2_000_000, 5).validate(), Err(RequestError::Samples));
      assert_eq!(req(Mode::Reflection, 1_000_000, 2_000_000, 1001).validate(), Err(RequestError::Samples));
      assert!(SweepRequest { extender: 16, ..req(Mode::Reflection, 2_400_000_000, 2_500_000_000, 500) }.validate().is_ok());
  }
  #[test] fn reply_parsing_with_step_bug() {
      // reply = points (1,101),(2,102),(3,103),(4,104),(5,105) for samples = 3 (5 device points)
      // step_bug on and start lowered: device point 0 dropped, then point 0 := point 1
      //   -> [(3,103),(3,103),(4,104),(5,105)]
      // step_bug off: -> [(1,101),(2,102),(3,103),(4,104)]
      // step_bug on but start not lowered: -> [(2,102),(2,102),(3,103),(4,104)]
  }
  #[test] fn recorded_device_reply_parses() {
      // tests/data/open_capture.json, reflection_open.replies_hex[0]: 2000 bytes
      let p = parse_points(&bytes);
      assert_eq!(p.len(), 500);
      assert_eq!((p[0].phase, p[0].level), (87, 366)); assert_eq!((p[1].phase, p[1].level), (12, 365));
      assert_eq!((p[499].phase, p[499].level), (131, 380));
  }
  #[test] fn generator_and_park() {
      assert_eq!(generator_parts(10_000_000, 1, DDS_FACTOR_DEFAULT), ["0\r", "107374182\r", "1\r", "0\r"]);
      assert_eq!(generator_parts(432_000_000, 4, DDS_FACTOR_DEFAULT), ["0\r", "1159641170\r", "1\r", "0\r"]);
  }
  ```
- [ ] **Step 2: Run `cargo test device::protocol`.** Expected: compile failure.
- [ ] **Step 3: Implement.** `validate`: limits are `F_MIN_HZ * extender ..= F_MAX_HZ * extender`, `start < stop`, samples 10..=1000, `step_hz() >= 1`. Sent values are `round_ties_even(hz / extender * dds_factor)`. `freqs_hz()[i] = start_hz + i * step_hz()` using the displayed start (not the lowered one). `parse_reply` tolerates a short buffer by returning what it can parse (the caller checks length first).
- [ ] **Step 4: Run `cargo test device::protocol`.** Expected: 9 passed.
- [ ] **Step 5: Commit** (`feat: sweep command and reply parsing`).

---

### Task 5: Transport, simulated device, Device

**Files:**
- Create: `src/device/transport.rs`, `src/device/sim.rs`
- Modify: `src/device/mod.rs`, `Cargo.toml` (add `serialport`)

**Interfaces:**
- Consumes: `SweepRequest`, `generator_parts`, `PARK_PARTS`, `RawPoint`.
- Produces:
  ```rust
  // transport.rs
  pub trait Transport: Send {
      fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
      /// Read up to buf.len() bytes, returning how many arrived before the timeout.
      fn read(&mut self, buf: &mut [u8], timeout: Duration) -> std::io::Result<usize>;
      fn discard_input(&mut self) -> std::io::Result<()>;
  }
  pub struct SerialTransport { /* serialport handle */ }
  impl SerialTransport { pub fn open(port: &str) -> Result<Self, DeviceError>; }   // 115200 8N1, DTR+RTS, then sleeps 300 ms
  pub fn list_ports() -> Vec<String>;
  /// Tries each port with a 10-sample sweep; returns the first that answers with 48 bytes.
  pub fn find_minivna() -> Option<String>;
  // sim.rs
  pub struct SimTransport { pub fail_next_replies: usize, pub truncate_to: Option<usize>, pub written: Vec<u8> }
  impl SimTransport { pub fn new() -> Self; }
  // mod.rs
  #[derive(thiserror::Error, Debug)] pub enum DeviceError { Open(String), Permission(String), Timeout { got: usize, wanted: usize }, Io(String), Request(RequestError) }
  pub struct Device<T: Transport> { /* transport, part_pause, reply_timeout */ }
  impl<T: Transport> Device<T> {
      pub fn new(transport: T) -> Self;                    // part_pause 10 ms, reply_timeout 5 s
      pub fn with_timing(transport: T, part_pause: Duration, reply_timeout: Duration) -> Self;
      /// on_progress gets 0.0..=1.0 as bytes arrive. One automatic retry on a short reply.
      pub fn sweep(&mut self, req: &SweepRequest, on_progress: &mut dyn FnMut(f32)) -> Result<Vec<RawPoint>, DeviceError>;
      pub fn generator(&mut self, freq_hz: u64, extender: u8, dds_factor: f64) -> Result<(), DeviceError>;
      pub fn park(&mut self) -> Result<(), DeviceError>;
      pub fn transport(&mut self) -> &mut T;
  }
  ```

- [ ] **Step 1: Write the failing tests** (in `device/mod.rs`, all through `SimTransport` with `with_timing(.., 0 ms, 50 ms)`).
  ```rust
  #[test] fn sweep_sends_the_command_and_returns_samples_plus_one_points() {
      // req 1-180 MHz, 500 samples: written == b"0\r6893423\r502\r3843996\r"; result.len() == 501
  }
  #[test] fn sim_looks_like_an_antenna() {
      // convert() the result with zero cal: min SWR < 1.5 somewhere between 13 and 15 MHz for a 1-30 MHz sweep,
      // and SWR > 5 at 1 MHz
  }
  #[test] fn short_reply_retries_then_errors() {
      // fail_next_replies = 1 -> Ok on the retry, command written twice
      // fail_next_replies = 2 -> Err(DeviceError::Timeout { .. })
      // truncate_to = Some(100) -> Err(Timeout { got: 100, wanted: 2008 })
  }
  #[test] fn invalid_request_is_refused_before_writing() { /* start > stop -> Err(Request(Range)), written is empty */ }
  #[test] fn generator_and_park_bytes() { /* written == b"0\r107374182\r1\r0\r" then b"0\r10\r1\r0\r" */ }
  #[test] fn progress_reaches_one() { /* last on_progress value == 1.0 and values never decrease */ }
  ```
- [ ] **Step 2: Run `cargo test device`.** Expected: compile failure.
- [ ] **Step 3: Implement.**
  - `Device::sweep`: validate; `discard_input`; write the four parts with `part_pause` after each; read until `reply_len()` bytes or `reply_timeout` with no new data; on a short reply, discard input and repeat once; then `parse_reply`.
  - `generator` and `park`: discard input, write the parts, do not read.
  - `SimTransport`: parses the four numbers it is sent; when the count is > 1 it queues `count * 4` bytes for a series-RLC antenna resonant at 14.1 MHz (R = 50 Ω, Q ≈ 20), turning the reflection value back into readings with the inverse of the Task 1 formulas (phase folded into 0..180°), and sets the first point's phase reading to 87 to mimic the real device. For transmission mode it returns level 1000, phase 400. Count 1 (generator, park) queues nothing.
  - `SerialTransport::open`: map a permission error to `DeviceError::Permission` (the UI turns that into the `dialout` hint).
  - `find_minivna`: per port, open, send a 1–2 MHz 10-sample reflection sweep with a 1 s timeout, expect 48 bytes, park.
- [ ] **Step 4: Run `cargo test device`.** Expected: all pass.
- [ ] **Step 5: Check against the real device** (it is on `/dev/ttyUSB0`, nothing connected). Add `examples/probe.rs`: opens the port given as argument, runs a 1–180 MHz 500-sample reflection sweep, prints points 0, 1, 250 and 500 as raw readings and as dB/degrees, then parks. Run `cargo run --example probe /dev/ttyUSB0`. Expected: 501 points; level readings between 360 and 385; phase readings rising from about 12 to about 130; uncalibrated loss between 1 and 2.5 dB.
- [ ] **Step 6: Commit** (`feat: serial transport, simulated device, device driver`).

---

### Task 6: Background sweep worker

**Files:**
- Create: `src/sweep.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Device`, `Transport`, `SerialTransport`, `SimTransport`, `SweepRequest`, `Mode`, `Calibration`, `table_from_sweep`, `convert`, `apply_sign`.
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq)] pub enum PortChoice { Simulated, Serial(String) }
  pub enum Command {
      Single(SweepRequest), Continuous(SweepRequest), Stop,
      Calibrate(Mode, /*step_bug*/ bool, /*dds_factor*/ f64),
      Generator { freq_hz: u64, extender: u8, dds_factor: f64 }, Park,
      SetPort(PortChoice), SetCalibration(Calibration), SetSigned(bool), Shutdown,
  }
  #[derive(Clone)] pub struct SweepResult { pub request: SweepRequest, pub raw: Vec<RawPoint>, pub cal: Vec<i16>, pub points: Vec<Point> }
  pub enum Event { Busy(bool), Progress(f32), Sweep(SweepResult), CalTable(Mode, Vec<i16>), Error(String) }
  pub struct Worker { pub tx: mpsc::Sender<Command>, pub rx: mpsc::Receiver<Event> }
  /// wake is called after every event so the UI repaints.
  pub fn spawn(port: PortChoice, cal: Calibration, wake: Box<dyn Fn() + Send>) -> Worker;
  /// Pure helper, also used when loading a sweep file.
  pub fn build_result(request: SweepRequest, raw: Vec<RawPoint>, cal: Vec<i16>, signed: bool) -> SweepResult;
  ```

Behaviour:
- **Single:** `Busy(true)`, open the port, sweep, park, close, `Sweep`, `Busy(false)`.
- **Continuous:** open once, sweep repeatedly with no pause, checking for `Stop` or a new command between sweeps; on stop: park, close, `Busy(false)`.
- **Calibrate:** a 100 kHz–180 MHz, 1000-sample sweep in the given mode, then `CalTable(mode, table_from_sweep(..))`.
- **Generator:** keeps the port open until `Park`.
- **Any device error:** `Error(text)`, the port is closed, a continuous sweep ends, `Busy(false)`.

- [ ] **Step 1: Write the failing tests** (all with `PortChoice::Simulated`, receiving with a 2 s timeout).
  ```rust
  #[test] fn single_sweep_emits_busy_sweep_idle() { /* Busy(true), .., Sweep(r) with r.points.len()==501, Busy(false) */ }
  #[test] fn continuous_runs_until_stopped() { /* Continuous -> at least 3 Sweep events -> Stop -> Busy(false), then no more Sweep within 200 ms */ }
  #[test] fn calibrate_returns_a_1000_entry_table() { /* CalTable(Mode::Reflection, t) with t.len()==1000 */ }
  #[test] fn calibration_is_applied() {
      // SetCalibration with reflection = Some(vec![-20; 1000]) -> Single -> every r.cal[i] == -20
      // and r.points[i].rl_db == convert(raw, cal)[i].rl_db
  }
  #[test] fn error_stops_continuous() {
      // SetPort(Serial("/nonexistent/port")) then Continuous -> Error(_) then Busy(false), and no Sweep events
  }
  #[test] fn build_result_applies_sign_only_when_asked() { /* signed=false -> all phase_deg >= 0; signed=true -> some negative for the sim antenna */ }
  ```
- [ ] **Step 2: Run `cargo test sweep`.** Expected: compile failure.
- [ ] **Step 3: Implement** with one `std::thread` and `mpsc` channels. The device type differs by `PortChoice`; use a boxed `dyn Transport`. `build_result` = `cal` lookup already done by the caller → `convert(raw, cal, start, step)` → `apply_sign` if `signed`.
- [ ] **Step 4: Run `cargo test sweep`.** Expected: 6 passed.
- [ ] **Step 5: Commit** (`feat: background sweep worker`).

---

### Task 7: Settings, band presets, velocity table

**Files:**
- Create: `src/store/mod.rs`, `src/store/settings.rs`
- Modify: `src/lib.rs`, `Cargo.toml` (add `directories`)

**Interfaces:**
- Produces:
  ```rust
  // store/mod.rs
  pub fn config_dir() -> PathBuf;                 // ProjectDirs("", "", "minivna").config_dir(), created if missing
  pub fn settings_path() -> PathBuf;              // config_dir()/settings.toml
  pub fn calibration_path() -> PathBuf;           // config_dir()/calibration.toml
  // store/settings.rs  (all Serialize + Deserialize + Clone + PartialEq, every field #[serde(default)])
  pub struct Rgb(pub u8, pub u8, pub u8);
  pub struct Colors { pub swr: Rgb, pub z: Rgb, pub rl: Rgb, pub phase: Rgb, pub rs: Rgb, pub xs: Rgb,
                      pub background: Rgb, pub frame: Rgb, pub ohms: Rgb, pub m1: Rgb, pub m2: Rgb, pub swr_min: Rgb }
  pub struct Band { pub name: String, pub start_mhz: f64, pub stop_mhz: f64, pub m1_mhz: f64, pub m2_mhz: f64, pub needs_extender: bool }
  pub struct Velocity { pub factor: f64, pub note: String }
  pub struct Settings {
      pub port: Option<String>, pub dds_factor: f64, pub step_bug: bool,
      pub extender_menu: bool, pub signed_phase: bool,
      pub line_50_ohms: bool, pub vertical_lines: bool, pub m1_at_start: bool, pub markers_from_band: bool,
      pub thick_lines: bool, pub velocity: f64,
      pub start_mhz: f64, pub stop_mhz: f64, pub samples: u16,
      pub screen: Colors, pub png: Colors, pub bands: Vec<Band>, pub velocities: Vec<Velocity>,
      pub window: Option<[f32; 4]>,
  }
  impl Default for Settings;       // values below
  impl Settings { pub fn load(path: &Path) -> (Settings, Option<String>);   // never fails: defaults + a message on any problem
                  pub fn save(&self, path: &Path) -> std::io::Result<()>; }
  ```

Defaults:
- `dds_factor` 10.73741824, `step_bug` true, `extender_menu` false, `signed_phase` false, `line_50_ohms` false, `vertical_lines` true, `m1_at_start` false, `markers_from_band` false, `thick_lines` false, `velocity` 0.66, range 1–180 MHz, `samples` 500.
- **Colours** (both sets), from the original `IG_Colors.ini` (values there are Windows BGR integers): SWR (255,0,0); |Z| (0,128,0); RL (0,0,255); Phase (255,0,255); Rs (128,128,128); Xs (0,128,128); background (255,255,255); frame (0,0,0); ohms (128,128,128); M1 (255,0,0); M2 (0,128,0); SWR min (0,255,0).
- **Bands:** the 19 entries of the original `IG_Bands.ini` (`~/.cxoffice/HamRadioXP/drive_c/Program Files/IW1GEU-miniVNA/IG_Bands.ini`; decimal commas there are decimal points here), in file order: 160m, 80m, 60m, 40m, 30m, 20m, 17m, 15m, 12m, 10m, 6m, 4m, 2m, 70cm, 23cm, 13cm, Personal 1, Personal 2, Personal 3. `needs_extender` is true for 70cm, 23cm and 13cm.
- **Velocities:** read the default table from `docs/reverse-engineering/forms/VELO.txt` and the code around `0x497300` (`FormCreate` of the velocity form) using `docs/reverse-engineering/tools/dis.py`. If the original's defaults cannot be recovered, use: 0.66 "RG-58, RG-213 (solid PE)", 0.78 "foam PE", 0.82 "Aircell 7, H-155", 0.85 "Ecoflex 10, H-2000", 0.95 "open-wire line".

- [ ] **Step 1: Write the failing tests.**
  ```rust
  #[test] fn defaults() { let s = Settings::default(); assert_eq!(s.samples, 500); assert_eq!(s.bands.len(), 19);
      assert_eq!(s.bands[0].name, "160m"); assert_eq!(s.bands[0].start_mhz, 1.7); assert_eq!(s.bands[0].m2_mhz, 1.85);
      assert_eq!(s.bands[5].name, "20m"); assert_eq!(s.bands[5].stop_mhz, 14.5);
      assert!(s.bands[13].needs_extender && !s.bands[12].needs_extender); assert_eq!(s.screen.rl, Rgb(0,0,255)); }
  #[test] fn save_then_load() { /* change port, a colour, a band; save; load == changed, message None */ }
  #[test] fn missing_file_gives_defaults_without_a_message() {}
  #[test] fn garbage_gives_defaults_with_a_message() { /* file "][ nonsense" -> (Settings::default(), Some(_)) */ }
  #[test] fn partial_file_fills_the_rest_with_defaults() { /* file "samples = 200" -> samples 200, bands.len() 19 */ }
  #[test] fn out_of_range_values_are_repaired() { /* samples = 5000 -> 1000; start_mhz = 500, stop_mhz = 2 -> 1 and 180; velocity = 0 -> 0.66 */ }
  ```
- [ ] **Step 2: Run `cargo test store::settings`.** Expected: compile failure.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo test store::settings`.** Expected: 6 passed.
- [ ] **Step 5: Commit** (`feat: settings, band presets, velocity table`).

---

### Task 8: Sweep files, CSV and PNG

**Files:**
- Create: `src/store/sweep_file.rs`, `src/store/csv.rs`, `src/store/png.rs`
- Modify: `src/store/mod.rs`, `Cargo.toml` (add `image` with only the `png` feature)

**Interfaces:**
- Consumes: `SweepRequest`, `RawPoint`, `SweepResult`, `Mode`.
- Produces:
  ```rust
  // sweep_file.rs
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub struct ViewState { pub m1: Option<usize>, pub m2: Option<usize>, pub show: [bool; 6] /* SWR,Z,RL,Phase,Rs,Xs */,
                         pub scale_60db: bool, pub rl_offset_db: f64 }
  pub struct SweepFile { pub request: SweepRequest, pub raw: Vec<RawPoint>, pub cal: Vec<i16>, pub view: ViewState }
  pub fn save(path: &Path, file: &SweepFile) -> Result<(), FileError>;
  pub fn load(path: &Path) -> Result<SweepFile, FileError>;
  #[derive(thiserror::Error, Debug)] pub enum FileError { Io(..), Format(String) }
  // csv.rs
  pub fn to_csv(result: &SweepResult) -> String;
  // png.rs
  pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), FileError>;
  ```

`.mvna` format: a TOML document. Table `[sweep]` holds `format = 1`, mode, start_hz, stop_hz, samples, extender, dds_factor, step_bug. Table `[view]` holds the `ViewState`. Key `points` is an array of `[phase, level, cal]` triples, one per point.

- [ ] **Step 1: Write the failing tests.**
  ```rust
  #[test] fn sweep_file_round_trip() { /* 11 points; save; load == original */ }
  #[test] fn sweep_file_rejects_bad_input() {
      // not TOML -> Format; format = 2 -> Format; points length != samples + 1 -> Format;
      // samples = 5 -> Format (request.validate() fails); a triple with 2 entries -> Format
  }
  #[test] fn csv_reflection() {
      // 2 points: (7 000 000 Hz, rl 9.986628, phase 90.087977), (7 358 000 Hz, 13.857595, 45.043988)
      assert_eq!(to_csv(&r), "\"Frequency(Hz)\",\"Return Loss(dB)\",\"Phase(deg)\"\r\n7000000,9.99,90.09\r\n7358000,13.86,45.04\r\n");
  }
  #[test] fn csv_transmission_header() { /* starts with "\"Frequency(Hz)\",\"Transmission Loss(dB)\",\"Phase(deg)\"" */ }
  #[test] fn png_round_trip() { /* 2x2 RGBA written, read back with image, same pixels */ }
  ```
- [ ] **Step 2: Run `cargo test store`.** Expected: compile failure.
- [ ] **Step 3: Implement.** CSV always uses "." and ",", whatever the system locale.
- [ ] **Step 4: Run `cargo test store`.** Expected: all pass.
- [ ] **Step 5: Commit** (`feat: sweep files, CSV and PNG export`).

---

### Task 9: Interface state, window shell, sweep controls, graph

**Files:**
- Create: `src/ui/mod.rs`, `src/ui/text.rs`, `src/ui/state.rs`, `src/ui/side_panel.rs`, `src/ui/graph.rs`
- Modify: `src/main.rs`, `src/lib.rs`, `Cargo.toml` (add `eframe`, `egui_plot`)

**Interfaces:**
- Consumes: everything from Tasks 1–8.
- Produces:
  ```rust
  // ui/text.rs
  pub fn t(key: &'static str) -> &'static str;      // English table; returns the key itself if missing
  // ui/state.rs
  #[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum UiMode { Antenna, CableLoss, CableLength, Transmission, Generator }
  #[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Curve { Swr, Z, Rl, Phase, Rs, Xs }
  pub fn parse_mhz(text: &str) -> Option<f64>;      // accepts "3.5" and "3,5", trims spaces; None for anything else
  pub struct AppState {
      pub settings: Settings, pub calibration: Calibration,
      pub mode: UiMode, pub extender: u8,
      pub start_text: String, pub stop_text: String, pub samples_text: String,
      pub start_hz: u64, pub stop_hz: u64, pub samples: u16,
      pub show: [bool; 6], pub scale_60db: bool, pub rl_offset_db: f64,
      pub result: Option<SweepResult>, pub busy: bool, pub progress: f32, pub continuous: bool,
      pub status: String,
      pub m1: Option<usize>, pub m2: Option<usize>, pub show_swr_min: bool, pub memo_y: Option<f64>,
      pub zoom_stack: Vec<(u64, u64)>,
  }
  impl AppState {
      pub fn new(settings: Settings, calibration: Calibration) -> Self;
      /// Apply the three text boxes. Invalid or out-of-range text restores the previous text. Returns true if anything changed.
      pub fn commit_range(&mut self) -> bool;
      pub fn set_range(&mut self, start_hz: u64, stop_hz: u64) -> bool;   // false and unchanged if invalid
      pub fn request(&self) -> SweepRequest;                              // from the current state
      pub fn sweep_mode(&self) -> Mode;                                   // Transmission only for UiMode::Transmission
      pub fn on_event(&mut self, event: Event);                           // from the worker
      pub fn step_text(&self) -> String;                                  // "(Step : 358000 Hz)"
  }
  // ui/mod.rs
  pub struct App { pub state: AppState, worker: Worker, /* dialogs added in later tasks */ }
  pub fn run(port: PortChoice) -> eframe::Result<()>;
  ```

- [ ] **Step 1: Write the failing tests** in `ui/state.rs`.
  ```rust
  #[test] fn parse_mhz_accepts_comma_and_point() {
      assert_eq!(parse_mhz("3.5"), Some(3.5)); assert_eq!(parse_mhz(" 3,5 "), Some(3.5)); assert_eq!(parse_mhz("180"), Some(180.0));
      for bad in ["", "abc", "1.2.3", "-5", "1e3x", "NaN", "inf"] { assert_eq!(parse_mhz(bad), None); }
  }
  #[test] fn commit_range_accepts_valid_text() { /* "7", "7,2", "100" -> start_hz 7_000_000, stop_hz 7_200_000, samples 100 */ }
  #[test] fn commit_range_restores_invalid_text() {
      // from 1/180/500: start "200" -> text back to "1"; stop "0.5" -> back to "180"; start "abc" -> "1";
      // start "" -> "1"; samples "5" -> clamped to 10; samples "5000" -> 1000; samples "" -> "500"
  }
  #[test] fn set_range_refuses_reversed_or_out_of_limit() { /* (9e6, 7e6) false; (7e6, 7e6) false; (50_000, 7e6) false; state unchanged */ }
  #[test] fn extender_scales_the_limits() { /* extender 4: set_range(430e6, 440e6) true; extender 1: false */ }
  #[test] fn request_reflects_state() { /* mode Transmission -> Mode::Transmission; step_bug and dds_factor from settings */ }
  #[test] fn events_update_state() {
      // Busy(true) -> busy; Progress(0.5) -> progress; Sweep(r) -> result is Some, status cleared;
      // Error("x") -> status == "x", busy false, continuous false
  }
  #[test] fn step_text() { /* 1-180 MHz, 500 samples -> "(Step : 358000 Hz)" */ }
  ```
- [ ] **Step 2: Run `cargo test ui::state`.** Expected: compile failure.
- [ ] **Step 3: Implement `text.rs` and `state.rs`.** English strings come from `Lng/English.lng` in the original's folder; use its left-hand names as keys where one exists.
- [ ] **Step 4: Run `cargo test ui::state`.** Expected: 8 passed.
- [ ] **Step 5: Implement the window.**
  - `main.rs`: `--sim` selects `PortChoice::Simulated`; otherwise the port from settings, or `find_minivna()`, or simulated with a status message if none is found.
  - `ui/mod.rs`: `App::update` drains worker events into `state.on_event`, then draws the side panel (left), a status bar (bottom, shows `state.status` and the port) and the graph (centre). Settings are saved on exit.
  - `side_panel.rs`: "x Sweep", "1 Sweep" and "Stop" buttons, the Read light, the from/to/samples boxes (committed on Enter or focus loss via `commit_range`), the step text, six curve boxes with colour swatches, thin/thick, 30/60 dB, offset (−60..60, anything else resets to 0).
  - `graph.rs`: frequency on X in MHz. Left axes as in the original screenshot: SWR on a scale with ticks at 1, 1.5, 2, 3, 5, 10 and return loss on a linear 0–30 or 0–60 dB scale with 0 at the top. Each curve is mapped onto the plot's 0..1 vertical range by its own scale so all six can share the plot: |Z|, Rs and |Xs| on 0–500 Ω unless the original's drawing code (`0x49dd5c`, see `docs/reverse-engineering/tools`) shows another range; phase on 0–180°, or −180..180° when signed. The RL offset shifts the RL curve and its tick labels only. Values that are infinite or beyond the scale are drawn at the edge, never skipped into NaN. Optional 50 Ω line and vertical frequency lines per settings.
- [ ] **Step 6: Run it.** `cargo run -- --sim`, press "1 Sweep". Expected: an SWR dip near 14.1 MHz and a return-loss peak at the same place; "x Sweep" keeps redrawing and "Stop" stops it; the window stays responsive throughout. Then `cargo run` with the real device on `/dev/ttyUSB0` and nothing connected: expected a flat high SWR and return loss between 1 and 2.5 dB (no calibration yet).
- [ ] **Step 7: Commit** (`feat: window, sweep controls and graph`).

---

### Task 10: Markers, mouse and keyboard on the graph, read-outs

**Files:**
- Create: `src/ui/marker_panel.rs`
- Modify: `src/ui/state.rs`, `src/ui/graph.rs`, `src/ui/mod.rs`

**Interfaces:**
- Consumes: `AppState`, `calc::tools::*`.
- Produces (added to `AppState`):
  ```rust
  #[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Marker { M1, M2 }
  pub fn index_at(&self, freq_hz: f64) -> Option<usize>;            // nearest point; None without a result
  pub fn place_marker(&mut self, m: Marker, freq_hz: f64);
  pub fn step_marker(&mut self, m: Marker, delta: i32);              // clamped to 0..=last
  pub fn toggle_marker(&mut self, m: Marker);                        // on: M1 at 1/3, M2 at 2/3 of the sweep
  pub fn marker_to_band_default(&mut self, m: Marker);               // uses the current band preset, if any
  pub fn zoom_to_markers(&mut self) -> bool;                         // pushes the old range; false unless both set and different
  pub fn unzoom(&mut self) -> bool;
  pub fn toggle_memo(&mut self, y: f64);
  pub fn marker_row(&self, idx: usize) -> [String; 7];               // freq MHz (6 dp), SWR, |Z|, RL, phase, Rs, |Xs| (2 dp; "∞" for infinite, ">9999" above 9999)
  pub fn reactance_fields(&self) -> Option<(bool /*inductive*/, String, String)>;  // only when both markers are set
  pub fn bandwidth_fields(&self) -> Option<(String, String)>;       // BW Hz (0 dp), Q (2 dp)
  pub fn swr_min(&self) -> Option<usize>;
  ```

- [ ] **Step 1: Write the failing tests** (a helper builds an `AppState` holding a simulated 1–30 MHz, 290-sample result, so points are 100 kHz apart).
  ```rust
  #[test] fn place_and_step() { /* place M1 at 14.12e6 -> index 131; step +1 -> 132; step -500 -> 0; step +5000 -> 290 */ }
  #[test] fn no_result_means_no_markers() { /* place_marker on an empty state leaves m1 None; marker_row not called */ }
  #[test] fn toggle_places_at_thirds() { /* M1 -> round(290/3) = 97; M2 -> round(290*0.66) = 191 */ }
  #[test] fn markers_clamp_when_samples_shrink() { /* m1 = Some(280); new result with 100 samples arrives -> m1 == Some(100) */ }
  #[test] fn zoom_and_unzoom() { /* m1 at 10 MHz, m2 at 20 MHz: zoom -> range 10-20 MHz, texts updated; unzoom -> 1-30; unzoom again -> false */
                                 /* m1 == m2 -> zoom false */ }
  #[test] fn marker_row_formats() { /* frequency "14.100000"; values 2 dp; a point with swr = INFINITY -> "∞"; swr 12345.6 -> ">9999" */ }
  #[test] fn reactance_fields_need_both_markers() { /* None with one marker; Some with two; inductive iff C[M1] > C[M2] */ }
  #[test] fn bandwidth_fields() { /* markers at 7.0 and 7.2 MHz -> ("200000", "35.50"); same index -> None */ }
  #[test] fn memo_toggles() {}
  ```
- [ ] **Step 2: Run `cargo test ui::state`.** Expected: compile failure.
- [ ] **Step 3: Implement the state methods.** `reactance_fields` reads L at the marker itself (the original's 46-point offset is a bug).
- [ ] **Step 4: Run `cargo test ui::state`.** Expected: all pass.
- [ ] **Step 5: Implement the drawing and input.**
  - `marker_panel.rs`: rows for Marker 1, Marker 2 and SWR Mini with a tick box, a colour swatch, +/− buttons and the seven fields; the XL µH / Xc pF column with its "+j" / "-j" label; the "Marker 1 to 2" panel. Double-clicking a marker's frequency field opens a small entry dialog (plain egui window) that calls `place_marker`.
  - `graph.rs`: vertical lines for M1, M2 and SWR minimum in their colours; the memo line. Mouse: left click M1, right click M2, Ctrl+left/right click steps M1 left/right, Shift+left/right steps M2, Alt+left/right sends M1/M2 to the band default, middle click toggles the memo line.
  - Side panel: "Zoom M1/M2" and "unZoom" buttons; zoom triggers a new sweep.
  - Keys: Space = one sweep, F12 = continuous, Esc = stop.
- [ ] **Step 6: Run `cargo run -- --sim`.** Expected: every mouse and key action of the spec's table does what the table says; SWR Mini sits on the dip near 14.1 MHz.
- [ ] **Step 7: Commit** (`feat: markers, mouse and keyboard, read-outs`).

---

### Task 11: Menus, modes, band presets, frequency extender

**Files:**
- Create: `src/ui/menu.rs`
- Modify: `src/ui/state.rs`, `src/ui/marker_panel.rs`, `src/ui/mod.rs`

**Interfaces:**
- Produces (added to `AppState`):
  ```rust
  pub current_band: Option<usize>,
  pub fn set_mode(&mut self, mode: UiMode);            // M1 to 1/3, M2 to 2/3, both on
  pub fn apply_band(&mut self, index: usize) -> bool;  // false if the band needs an extender and none is selected
  pub fn apply_full_range(&mut self);                  // 1-180 MHz times extender
  pub fn apply_hf_range(&mut self);                    // "160 - 10 m": 1.7 to 29.8 MHz
  pub fn set_extender(&mut self, mult: u8);            // 1, 4, 8, 16: range becomes mult..180*mult MHz
  pub fn cable_loss_fields(&self) -> Option<[String; 3]>;      // min, avg, max
  pub fn cable_length_text(&self) -> Option<String>;           // metres, 2 dp
  pub fn set_velocity_from_length(&mut self, length_m: f64) -> bool;
  /// The automatic cable-length search as a small state machine driven by sweep results.
  pub fn auto_length_start(&mut self) -> SweepRequest;
  pub fn auto_length_on_result(&mut self) -> AutoLength;       // called after each result while searching
  pub enum AutoLength { NextSweep(SweepRequest), Done, Failed }
  ```

- [ ] **Step 1: Write the failing tests.**
  ```rust
  #[test] fn mode_change_places_markers() {}
  #[test] fn band_sets_range_and_optional_markers() {
      // apply_band(5) (20m) -> range 13.9-14.5 MHz, current_band Some(5);
      // with settings.markers_from_band and a result present, M1/M2 at 14.0 and 14.35 MHz after the next result
  }
  #[test] fn extender_bands_need_an_extender() { /* apply_band(13) false at x1; after set_extender(4) true, range 429-441 MHz */ }
  #[test] fn extender_resets_range() { /* set_extender(8) -> 8-1440 MHz; set_extender(1) -> 1-180 */ }
  #[test] fn cable_loss_fields() { /* RL 6.0 at M1, 4.0 at M2 -> ["2.00", "2.50", "3.00"] */ }
  #[test] fn cable_length_text() { /* markers at 10 and 21 MHz, velocity 0.67 -> "9.09" (9.091364 m) */ }
  #[test] fn velocity_from_length() { /* markers 10/20 MHz, 10 m -> settings.velocity ≈ 0.67 (rounded to 2 dp) */ }
  #[test] fn auto_length_two_passes() {
      // start -> request 1-180 MHz, 500 samples, markers off;
      // feed a synthetic result whose phase crosses 90° at 20, 40 and 60 MHz -> NextSweep(19-61 MHz);
      // feed a second such result -> Done with m1 and m2 on the 1st and 3rd crossings;
      // a result with no crossings -> Failed
  }
  ```
- [ ] **Step 2: Run `cargo test ui::state`.** Expected: compile failure.
- [ ] **Step 3: Implement the state methods.**
- [ ] **Step 4: Run `cargo test ui::state`.** Expected: all pass.
- [ ] **Step 5: Implement `menu.rs`.** Menus and items as in `docs/reverse-engineering/forms/VNA.txt` (`MainMenu`), in English:
  - **File:** Open, Save, Save CSV, Save PNG, Exit. The first four are wired in Task 13.
  - **Modes:** Antenna, Cable Loss, Cable Length, Transmission, Generator.
  - **Bands predefined:** "160 - 10 m", "1 - 180 MHz", then the bands. Bands that need an extender are greyed out without one.
  - **Frequency Extender:** Off, x4, x8, x16. Shown only when `settings.extender_menu` is set.
  - **Trace options:** the four options, plus "Estimate sign of phase and Xs", which sends `SetSigned` to the worker and recomputes the current result. "Activate Marker 1 at program start" (`m1_at_start`) turns M1 on at one third of the range when the first result arrives. "Position markers by band defaults" (`markers_from_band`) puts M1 and M2 on the band's default frequencies after a band preset's sweep.
  - **Configure:** items open the dialogs of Tasks 12 and 13; "Compensate for firmware Step bug" and "Enable frequency extender menu" are tick items.
  - **About.**
  - Keyboard shortcuts exactly as the spec's table. The window title is `minivna - Mode : <mode> ( <port> )`.
  - `marker_panel.rs`: in Cable Loss mode show the min/avg/max panel; in Cable Length mode show metres, velocity factor, "Find automatically", "?" and "L (m) -> Velo."; in Transmission mode the RL column is labelled "T.L." and only loss and phase curves are offered.
- [ ] **Step 6: Run `cargo run -- --sim`.** Expected: each mode shows its panel; F6 selects 20 m and sweeps 13.9–14.5 MHz; enabling the extender menu and choosing x4 changes the range to 4–720 MHz and enables 70 cm.
- [ ] **Step 7: Commit** (`feat: menus, modes, band presets, extender`).

---

### Task 12: Device dialogs: port, calibration, DDS, generator

**Files:**
- Create: `src/ui/dialogs/mod.rs`, `port.rs`, `calibrate.rs`, `dds.rs`, `generator.rs`
- Modify: `src/ui/mod.rs`, `src/ui/menu.rs`, `src/ui/state.rs`

**Interfaces:**
- Produces:
  ```rust
  // state.rs
  pub fn store_cal_table(&mut self, mode: Mode, table: Vec<i16>);   // updates calibration, saves it, sends SetCalibration
  pub fn nudge_dds(trial: f64, step: f64) -> f64;                   // trial + step, kept within 10.0..=11.5
  pub fn generator_freq_step(freq_hz: u64, delta: i64, extender: u8) -> u64;  // clamped to the frequency limits
  // dialogs: each is a struct with `pub open: bool` and `pub fn show(&mut self, ctx, state: &mut AppState, tx: &Sender<Command>)`
  ```

- [ ] **Step 1: Write the failing tests.**
  ```rust
  #[test] fn nudge_dds() { /* 10.73741824 + 0.001 = 10.73841824; a nudge past 11.5 or below 10.0 stays at the limit */ }
  #[test] fn generator_step_clamps() { /* 10 MHz + 1 MHz = 11 MHz; 100 kHz - 1 MHz = 100 kHz; 180 MHz + 1 Hz = 180 MHz; x4: limits 400 kHz and 720 MHz */ }
  #[test] fn storing_a_table_marks_it_done() { /* reflection table stored -> calibration.reflection is Some; file on disk reloads equal */ }
  ```
- [ ] **Step 2: Run the tests.** Expected: compile failure.
- [ ] **Step 3: Implement the helpers, then the dialogs.** Layouts follow `docs/reverse-engineering/forms/SELCOM.txt`, `CALIBRAGE.txt`, `DDS.txt` and `GENE.txt`.
  - **Port:** list from `list_ports()`, "Automatic search" (`find_minivna()` on a background thread, "MiniVNA not Found !" if none), a "Simulated device" entry, OK sends `SetPort`. A `DeviceError::Permission` anywhere shows: "No permission to open the port. On Linux, add your user to the `dialout` group and log in again."
  - **Calibration:** started from "Calibrate Reflection" or "Calibrate Transmission". Step text from the original: transmission "Connect with the shortest DUT and DET, then click on Ok."; reflection "Remove any connection enter DUT and DET, then click on Ok." OK sends `Command::Calibrate`; progress is shown; on `CalTable` it calls `store_cal_table` and shows "Your MiniVNA is calibrate."
  - **DDS:** frequency box (default 10 MHz), the current trial factor, eight buttons ±0.001, ±0.0001, ±0.00001, ±0.000001. Every change sends `Command::Generator` with the trial factor. OK stores it in settings; Cancel discards it; both send `Park`.
  - **Generator** (opened by the Generator mode): frequency display with up/down buttons per digit from 1 Hz to 100 MHz; every change sends `Command::Generator`; closing sends `Park` and returns to Antenna mode.
  - At start, if no reflection calibration exists, the status bar says "Please calibrate your miniVNA first".
- [ ] **Step 4: Run the tests, then `cargo run` with the real device, nothing connected.** Run "Calibrate Reflection". Expected afterwards: a sweep shows return loss within ±0.2 dB of 0 across 1–180 MHz and SWR above 50 everywhere. Restart the program: the calibration is still applied.
- [ ] **Step 5: Commit** (`feat: port, calibration, DDS and generator dialogs`).

---

### Task 13: Remaining dialogs and file actions

**Files:**
- Create: `src/ui/dialogs/bands.rs`, `colors.rs`, `velocity.rs`, `lc.rs`
- Modify: `src/ui/mod.rs`, `src/ui/menu.rs`, `src/ui/state.rs`, `src/ui/graph.rs`, `Cargo.toml` (add `rfd`)

**Interfaces:**
- Produces:
  ```rust
  // state.rs
  pub fn to_sweep_file(&self) -> Option<SweepFile>;
  pub fn load_sweep_file(&mut self, file: SweepFile);       // sets range, mode, markers, view; result via sweep::build_result
  pub fn lc_result(&self, value: f64, input_is_uh: bool) -> Option<f64>;   // uses M1's frequency
  pub fn move_m1_to(&mut self, target: M1Target);           // MinSwr, MaxRl, MinRl
  pub enum M1Target { MinSwr, MaxRl, MinRl }
  // graph.rs
  pub fn render_png(state: &AppState, width: u32, height: u32) -> Vec<u8>;   // RGBA, drawn with the PNG colour set
  ```

- [ ] **Step 1: Write the failing tests.**
  ```rust
  #[test] fn sweep_file_round_trip_through_state() { /* state with result, markers, offset -> to_sweep_file -> fresh state.load_sweep_file -> same range, markers, show, and equal points */ }
  #[test] fn no_result_no_file() { assert!(empty_state.to_sweep_file().is_none()); }
  #[test] fn lc_uses_m1() { /* M1 at 7.1 MHz, 15 µH -> ≈ 33.4977 pF; no M1 -> None; value 0 -> None */ }
  #[test] fn move_m1() { /* MinSwr puts M1 on swr_min(); MaxRl on the highest RL */ }
  #[test] fn render_png_has_the_requested_size_and_background() { /* 400x300 -> 480 000 bytes; pixel (0,0) is the PNG background colour */ }
  ```
- [ ] **Step 2: Run the tests.** Expected: compile failure.
- [ ] **Step 3: Implement.**
  - **File menu:** Open/Save `.mvna` through `rfd` dialogs; Save CSV writes `to_csv`; Save PNG writes `render_png` at the graph's current size. Every failure goes to the status bar; a file that fails to load leaves the current state untouched.
  - `render_png` draws the same curves, axes, markers and legend as the screen graph, with its own simple line rasteriser onto an RGBA buffer, so it does not depend on a running window. Share the scale-mapping functions with `graph.rs`.
  - **Bands editor** (`forms/BANDES.txt`): a table of name, begin, end, mark 1, mark 2; cells accept `parse_mhz`; invalid cells restore the old value.
  - **Colours** (`forms/COULEURS.txt`): two columns (Screen, PNG) of colour pickers for the twelve colours.
  - **Velocity table** (`forms/VELO.txt`): editable factor and note; clicking a row sets the velocity factor.
  - **LC calculator** (`forms/CALCULE.txt`): value box (default 15), µH/pF choice, "=" button, and the three "move M1" buttons.
  - **"Visual" boxes:** read `GrosM1MouseUp` (`0x4aa4d0`), `GrosM2MouseUp` (`0x4aa4a0`) and `GrosSWRMouseUp` (`0x4aa470`) with `docs/reverse-engineering/tools/dis.py` to learn what they do, then implement that. If it cannot be determined, leave the boxes out and record that in the commit message.
- [ ] **Step 4: Run the tests, then `cargo run -- --sim`.** Save a sweep, restart, open it: same curves and markers. Export CSV and PNG and open both.
- [ ] **Step 5: Commit** (`feat: file actions and remaining dialogs`).

---

### Task 14: Checks on the real device with the user

No code unless a check fails. Each check that needs the user to connect something is a stop: ask, wait, then continue. Record results in `docs/hardware-checks.md`.

- [ ] **Step 1: Open port, calibrated.** Nothing connected. Expected: RL within ±0.2 dB of 0 from 1 to 180 MHz; continuous sweep runs for one minute without an error.
- [ ] **Step 2: Negative start.** Using `examples/probe.rs`, send the original's calibration command with the negative start (`0\r-857920\r1002\r1931662\r`) and the rewrite's (`0\r1073742\r1002\r1931662\r`). Compare points 2..1001 of the first with points 1..1000 of the second. Expected: equal within ±2 counts. If they differ by more, report to the user before changing anything.
- [ ] **Step 3: Step-bug option.** One sweep with the option on and one with it off, same range. Expected: with it off, point 0 shows the outlier (phase reading far from point 1); with it on, no outlier.
- [ ] **Step 4 (user connects a 50-ohm load).** Expected: SWR below 1.3 across HF, return loss above 18 dB.
- [ ] **Step 5 (user connects an antenna or other known load, and runs the original under CrossOver on it if it still works).** Compare SWR, RL, phase, Rs and |Xs| at three marker frequencies. Expected: within 0.5 dB and 2 degrees. If the original no longer talks to the device, record that and skip the comparison.
- [ ] **Step 6 (user connects an open-ended cable of known length).** Cable Length → "Find automatically". Expected: within 5% of the real length with the cable's velocity factor.
- [ ] **Step 7 (user joins DUT and DET).** Calibrate Transmission, then sweep in Transmission mode. Expected: loss within ±0.3 dB of 0.
- [ ] **Step 8: Commit** the results file (`docs: hardware check results`).

---

### Task 15: Packaging

**Files:**
- Create: `packaging/minivna.desktop`, `packaging/minivna.png` (256×256 icon), `packaging/build-linux.sh`, `packaging/build-windows.sh`, `.github/workflows/build.yml`, `README.md`
- Modify: `Cargo.toml` (`[package.metadata.deb]`, release profile `lto = true`, `strip = true`)

- [ ] **Step 1: Linux `.deb`.** `cargo install cargo-deb`; metadata installs the binary to `/usr/bin/minivna`, the desktop file and the icon. `packaging/build-linux.sh` runs `cargo deb`. Verify: `dpkg-deb -c target/debian/minivna_*.deb` lists the three files; `dpkg-deb -I` shows the package name `minivna`.
- [ ] **Step 2: Linux AppImage.** The same script builds an AppDir (binary, desktop file, icon, `AppRun`) and runs `appimagetool`. **Stop and ask the user** to install `appimagetool` if it is missing; do not download it without asking. Verify: the `.AppImage` starts with `--sim` and sweeps.
- [ ] **Step 3: Windows zip.** `packaging/build-windows.sh`: `rustup target add x86_64-pc-windows-gnu`, `cargo build --release --target x86_64-pc-windows-gnu`, zip `minivna.exe` with `README.md`. Needs the `mingw-w64` system package; **stop and ask the user** to install it if `x86_64-w64-mingw32-gcc` is missing. Add `#![windows_subsystem = "windows"]` to `main.rs` so no console window opens. Verify: `file` reports a PE32+ GUI executable; if CrossOver/Wine is usable, start it with `--sim`.
- [ ] **Step 4: CI.** `.github/workflows/build.yml` with three jobs (ubuntu-latest, windows-latest, macos-latest), each running `cargo test` and `cargo build --release` and uploading the binary; the macOS job wraps it in an unsigned `.app` inside a `.dmg`. It only takes effect if the user puts the project on GitHub.
- [ ] **Step 5: README.** What the program is, the supported device, how to build, the `dialout` note for Linux, the FTDI driver note for Windows, the unsigned-app note for macOS, and where settings are stored. No licence section until the user chooses one.
- [ ] **Step 6: Run `cargo test` and `cargo clippy -- -D warnings`.** Expected: all tests pass, no warnings.
- [ ] **Step 7: Commit** (`build: packaging for Linux and Windows, CI`).
