# miniVNA desktop app rewrite: design

Date: 2026-11-17
Status: draft, awaiting approval

Items marked **(my call)** were decided by Claude, not by the user, and are open to correction.

Companion documents:
- `docs/reverse-engineering/ig-minivna-findings.md`: everything read from the original program's machine code, with addresses. It is the reference for every formula and constant below. Where this design and that file disagree on what the original does, that file wins.
- `docs/captures/capture_open_2026-11-17.json`: sweeps recorded from the user's device with nothing connected.

## Intent

IG_miniVNA is a 2008 Windows freeware program by IW1GEU (Jean-Louis) for the mRS miniVNA antenna analyser. Its Delphi source code was lost. The user runs it under CrossOver/Wine on Linux. The only existing rewrite is in Java, which the user does not want.

The user wants a native replacement whose source they own, so that they can modify and expand it. A Smith chart is one expansion they have in mind.

## Goals

- A native desktop program that does what IG_miniVNA does today, without Wine, and shows the same numbers for the same device readings.
- Source code organised so that new views and features (first candidate: a Smith chart) can be added without reworking the core.
- Packages for Linux and Windows. macOS is wanted "maybe": the build must not rule it out.

## Non-goals for the first version (my call)

- Reading the original `.VNA` and `.cal` files. Their record layout is now known (see the findings file), but the compression wrapper (`!HZL` header, Tlzrw1 component) has not been read. This is a candidate for a later version.
- Printing. PNG export replaces it.
- BMP export.
- The nine translations. The first version is English only, but every user-visible string goes through one lookup table so languages can be added later.
- The Smith chart itself. It is the first planned extension.
- The "VNA 1 + Stand alone graphic USB" option (a different device variant, 1 to 65 MHz; the user's setting for it is off).
- The miniVNA Pro and other analysers.

## Anti-goals

- No Java, no web view, no second language for the interface.
- The device, maths and file code must not depend on the interface code.
- No silent "improvements" to the measurement maths. Where the rewrite deliberately differs from the original, it is listed under "Deliberate differences" below.

## Constraints

- **Hardware.** The user's unit is an original miniVNA (confirmed by the user), on an FTDI FT232 adapter (USB id 0403:6001), `/dev/ttyUSB0` on the development laptop. The user has encouraged the builder to send commands to it.
- **Phase sign.** The device reports phase as 0 to 180 degrees with no sign (confirmed in the original's code). The original guesses the sign from the slope of phase against frequency, only in a hidden "360" mode.
- **Toolchain targets.** Current Rust targets Windows 10 and later.
- **macOS.** A build needs a Mac or a GitHub Actions macOS runner. Without a paid Apple developer account the app is unsigned and shows a warning on first open.
- **Known loads.** Checks that need a 50-ohm load, a short, or a cable of known length need the user to connect them. Each is a stop point in the plan.

## Approaches considered

1. **Rust + egui (chosen by the user).** Ready-made interactive plot widget, menu bar, colour pickers, tables and dialogs. One program per platform.
2. **Go + Gio.** Simpler language. The graph and most widgets would be built by hand.
3. **Go + Fyne.** More conventional widgets, but weak drawing of arcs and clipped curves, which counts against the planned Smith chart.

Web-view routes (Tauri, Wails) were dropped because they add JavaScript and a browser-engine dependency. The Smith chart and the three target platforms were both checked and found not to separate options 1 and 2. The user chose option 1.

## What the program does (version one)

### Modes

| Mode | Device sweep | What is shown |
|---|---|---|
| Antenna | Reflection | All six curves, markers, SWR minimum, bandwidth and Q. |
| Cable Loss | Reflection, far end of the cable open | Loss at M1 and M2 (half the return loss at each): minimum, maximum and average of the two. |
| Cable Length | Reflection | Length in metres from the frequency gap between M1 and M2, the velocity factor, a table of usual velocity factors, "Find automatically", and "L (m) -> Velo." |
| Transmission | Transmission | Loss (labelled "T.L.") and phase. |
| Generator | None | Fixed-frequency output with digit up/down buttons from 1 Hz upward. |

On every mode change M1 goes to one third of the sweep and M2 to two thirds, as in the original.

### Main window

Layout follows the original (screenshot: `ShortCut-IgminiVNA.jpg` in the original's folder):

```
+--------------------------------------------------------------------+
| File  Modes  Bands  Frequency Extender  Trace options  Configure   |
+-----------+--------------------------------------------------------+
| [x Sweep] |          Freq   SWR  |Z|  R.L. Phase  Rs  |Xs|  XL/Xc  |
| [1 Sweep] | Marker 1  ...                         | Marker 1 to 2  |
| (o) Read  | Marker 2  ...                         | Band Width   Q |
| Frequency | SWR Mini  ...                         | [LC calc]      |
|  from     +--------------------------------------------------------+
|  to       |                                                        |
|  Steps    |                    graph                               |
|  Zoom     |   SWR and return-loss axes on the left, frequency      |
| Graphic   |   along the bottom                                     |
|  [x] SWR  |                                                        |
|  [ ] |Z|  |                                                        |
|  [x] R.L. |                                                        |
|  [ ] Phase|                                                        |
|  [ ] Rs   |                                                        |
|  [ ] |Xs| |                                                        |
|  thin/thick                                                        |
| R.L. scale|                                                        |
|  30/60 dB |                                                        |
|  Offset   |                                                        |
+-----------+--------------------------------------------------------+
```

- **Sweep controls:** continuous sweep, single sweep, stop. Start and stop frequency in MHz, number of samples (10 to 1000, default 500), computed step shown. A "Read" light is on while the device is being read.
- **Curves:** SWR, |Z|, return loss, phase, Rs, |Xs|, each with a show/hide box and its own colour. Thin or thick lines.
- **Return-loss scale:** 30 dB or 60 dB, plus an offset between -60 and 60 dB. The offset shifts the plotted curve and its axis labels only; marker values and the CSV are not shifted.
- **Markers:** M1 and M2, each showing frequency (MHz, 6 decimals) and the six values (2 decimals), with +/- buttons to step them. An SWR-minimum row shows the point with the lowest SWR below 200.
- **XL / Xc fields:** shown when both markers are on. They show the equivalent inductance in µH or capacitance in pF at each marker.
- **Marker 1 to 2 panel:** bandwidth and Q, shown when both markers are on.
- **"Visual" boxes:** one per marker row. Meaning in the original not yet read (handlers `GrosM1/GrosM2/GrosSWR`); **left to the builder to find out** from the findings tools. Likely a larger read-out.
- **Zoom:** "Zoom M1/M2" sets the sweep range to the two markers; "unZoom" restores the previous range.
- **LC calculator:** a small dialog. Given the M1 frequency and an inductance in µH it gives the resonating capacitance in pF, or the reverse: `result = 1 / (3.948e-05 * f_MHz^2 * value)`. It also has buttons to move M1 to the lowest SWR, the highest return loss and the lowest return loss.
- **Trace options:** 50-ohm reference line, vertical frequency lines, place M1 at one third of the range at start, place M1/M2 at the band's default positions.
- **Memo line:** middle-click shows or removes a horizontal reference line.

### Mouse and keyboard (as in the original)

| Input | Action |
|---|---|
| Left click on graph | Place M1 |
| Right click on graph | Place M2 |
| Ctrl + left / right click | Move M1 one step left / right |
| Shift + left / right click | Move M2 one step left / right |
| Alt + left / right click | M1 / M2 to the band preset's default position |
| Double-click a marker's frequency | Type the frequency |
| Middle click | Toggle memo line |
| Space | One sweep |
| F12 | Start continuous sweep |
| Esc | Stop sweep |
| F1 to F10 | Bands 160 m to 10 m |
| Ctrl+F1 to Ctrl+F9 | 6 m, 4 m, 2 m, 70 cm, 23 cm, 13 cm, Personal 1 to 3 |
| Ctrl+D / Ctrl+F | 160 to 10 m / 1 to 180 MHz |
| Ctrl+A, P, L, T, G | Modes: Antenna, Cable Loss, Cable Length, Transmission, Generator |
| Ctrl+O, S, C, N, Q | Open, save, CSV export, PNG export, quit |

On macOS the same keys are used **(my call)**.

### Band presets

Each preset has a sweep start, sweep stop and default M1 and M2 positions. Defaults are copied from the original `IG_Bands.ini`. An editable table changes them. The 70 cm, 23 cm and 13 cm presets are enabled only when a frequency extender is selected.

### Frequency extender

A menu with Off, x4, x8 and x16, hidden until "Enable frequency extender menu" is ticked in Configure, as in the original.

- Selecting a multiplier sets the sweep range to 4 to 720, 8 to 1440 or 16 to 2880 MHz and scales the allowed frequency limits by the multiplier.
- All displayed, stored and exported frequencies are the multiplied ones.
- The values sent to the device (start and step) and the calibration lookup are divided by the multiplier.

### Calibration

Level only. Phase is never calibrated. This matches the original.

- **Reflection:** with nothing connected between DUT and DET, sweep 0.1 to 180 MHz with 1000 samples.
- **Transmission:** with DUT and DET joined by the shortest possible link, sweep the same range.
- For each of the 1000 points the stored value is `344 - level reading`. It is a signed count.
- **Applying it:** for each point of a measurement, the stored value for the nearest calibration frequency is added to the raw level reading before conversion to dB.
- **DDS frequency:** a dialog that puts the device in generator mode at an editable frequency (default 10 MHz) and lets the user nudge the Hz-to-device factor (default 10.73741824) by ±0.001, ±0.0001, ±0.00001 and ±0.000001 while watching a frequency counter. OK stores the factor.
- If no calibration exists, the program says so at start and measures uncalibrated.
- The calibration wizard uses the three-step wording of the original.

### Files

| Action | Format |
|---|---|
| Save / open a sweep | New text format, extension `.mvna` **(my call)**: a TOML header (start, stop, samples, mode, extender, markers, visible curves, scale, offset) and one line per point holding the two raw readings and the calibration value used. Same content as the original `.VNA`. Derived values are recomputed on load. |
| Export for ZPlots | CSV, comma separated, decimal point. Header `"Frequency(Hz)","Return Loss(dB)","Phase(deg)"` (middle column `"Transmission Loss(dB)"` in transmission mode). One row per point: frequency in whole Hz, loss with 2 decimals, phase with 2 decimals. |
| Save the graph | PNG, using the PNG colour set. |
| Calibration | Own text file in the settings directory: the two 1000-value tables and which of them are done. |

### Settings

Stored in the per-user configuration directory of each system, not next to the program. Contents: serial port, window position, trace options, colours (one set for the screen, one for PNG), band presets, velocity-factor table, DDS factor, step-bug option, extender menu enabled, last mode and sweep range.

### Serial port

- A port chooser listing the system's serial ports, and an "Automatic search" that tries each port and reports "MiniVNA not found" if none answers.
- Linux users need to be in the `dialout` group. The program says so when opening the port fails with a permission error.

## Device protocol

Read from the original's code and confirmed on the user's device.

- Serial, 115200 baud, 8 data bits, no parity, 1 stop bit, no handshake. DTR and RTS asserted.
- **After opening the port, wait before the first command.** On the user's device a command written immediately after opening got no reply; a 0.3 s wait worked every time. The original waits 0.1 s.
- Sweep command, sent as four ASCII numbers each followed by a carriage return, with a 10 ms pause after each:
  1. `0` for reflection, `1` for transmission
  2. start value = round(start Hz / extender × DDS factor)
  3. samples + 2
  4. step value = round(step Hz / extender × DDS factor)
- Step in Hz = round((stop - start) / samples), a whole number. The last point can therefore differ slightly from the stop frequency.
- Before the command, both serial buffers are flushed and any pending input is discarded.
- Reply: 4 bytes per point for samples + 2 points, no end marker. Each point is two readings, low byte first: phase, then level.
- Timeout: the original waits up to 51.84 s with no retry. The rewrite uses 5 s plus one retry **(my call)**; a 500-point sweep takes about half a second on the user's device.
- **Firmware step bug option** (on by default): the start is lowered by one step, the first returned point is dropped, and displayed point 0 is then a copy of point 1. With the option off, points are used as returned. What the firmware bug is, is not known; on the user's device the first returned point of a sweep is visibly wrong (phase reading 87 where its neighbours read 12), which the option hides.
- Points 0 to samples are used (samples + 1 points).
- **Parking:** after a single sweep, and when a continuous sweep or the generator is stopped, the program sends `0`, `10`, `1`, `0` (each with a carriage return) and closes the port without reading the reply. This parks the device's output near 1 Hz.
- **Continuous sweep:** the port stays open; sweeps follow one another. The original leaves about a second between sweeps; the rewrite starts the next one as soon as the previous one is drawn **(my call)**.
- **Generator:** flush, then `0`, round(frequency Hz × DDS factor), `1`, `0`. The reply is not read. Re-sent on every frequency change. Unlike the original, the value is divided by the extender multiplier (see "Deliberate differences").
- Frequency limits: 0.1 to 180 MHz, times the extender multiplier.

### Measured on the device (2026-11-17)

Nothing connected to DUT or DET. Recordings are in `docs/captures/`.

- Three 500-point sweeps in each mode: readings repeat within ±1 count from sweep to sweep.
- Reflection, 1 to 180 MHz: phase reading 10 to 131, level reading 363 to 380. With the formulas below and no calibration that is 2 to 23 degrees and 1.2 to 2.1 dB. After a calibration taken in the same state it is 0 dB, as expected for an open port.
- The first point of every sweep is an outlier.
- The device sends nothing after the last point.

These recordings become test data for the `device` and `calc` parts.

## Maths

All of this is read from the original's code. Per point, with `phase_raw` and `level_raw` the two readings and `cal` the calibration value for that point:

```
phase  = phase_raw * 180 / 1023                      degrees, 0 to 180
RL     = (level_raw + cal) * 60 / 1023 - 20.16       dB, positive = loss
rho    = 10^(-RL / 20)
SWR    = |(1 + rho) / (1 - rho)|
re     = rho * cos(phase) ;  im = rho * sin(phase)
D      = (1 - re)^2 + im^2
Rs     = |1 - re^2 - im^2| / D * 50
|Xs|   = |2 * im| / D * 50
|Z|    = sqrt(Rs^2 + Xs^2)
L (µH) = 1e6 * |Xs| / (2 * pi * f)
C (pF) = 1e12 / (2 * pi * f * |Xs|)
```

- After these are computed, RL below 0 is shown as 0. SWR, Rs and the rest are computed from the value before that clamp, as in the original.
- The same level formula serves transmission mode.
- **SWR minimum:** the point with the lowest SWR below 200, ignoring point 0.
- **XL / Xc fields:** if C at M1 is greater than C at M2, the fields show L in µH for both markers and the reactance label reads "+j"; otherwise they show C in pF and the label reads "-j".
- **Bandwidth and Q:** BW = |f(M1) - f(M2)|; Q = ((f(M1) + f(M2)) / 2) / BW. There is no automatic -3 dB search; the user places the markers.
- **Cable loss:** RL / 2 at M1 and at M2; minimum, maximum and average of those two.
- **Cable length:** `L = 3e8 / |f(M1) - f(M2)| / 2 * velocity - 0.045` metres. Default velocity factor 0.66. "L (m) -> Velo." solves the same formula for the velocity factor given a known length.
- **Find automatically (cable length):** sweep 1 to 180 MHz with 500 samples; find the first and third places where phase crosses 90 degrees (between 85 and 95); zoom the sweep to those two frequencies ± 1 MHz; sweep again; put M1 and M2 on the first and third crossings, each moved to the nearest point to 90 degrees within the next 10 samples. If three crossings are not found, show "!!!".
- **Signed phase:** the sign of phase and Xs can be estimated from whether phase rises or falls with frequency (rising = negative), with single-point glitches smoothed out. See "Deliberate differences".

Each measured point keeps the complex reflection value (`re`, `im`) as well as the derived values, so that a Smith chart can be added later without changing stored data.

## Deliberate differences from the original (my call, all of them)

| Original | Rewrite | Why |
|---|---|---|
| The µH field reads the inductance 46 points to the left of the marker | Reads it at the marker | Evident bug |
| Calibration values are looked up about one sample off, on a grid that does not quite match where the calibration points were taken | Looked up by the nearest actual calibration frequency | Evident bug; the effect on values is small |
| Generator frequency is not divided by the extender multiplier | Divided | Otherwise the displayed frequency is wrong with an extender |
| Progress indicator reaches 100% at a quarter of the data | Correct progress | Cosmetic bug |
| Signed phase and Xs only behind a hidden setting (`F4CLB=OUI`) | A visible option "Estimate sign of phase and Xs", off by default | Needed for the planned Smith chart; off by default so numbers match the original |
| Waits up to 51.84 s for data, never retries, busy-waits | 5 s, one retry, background thread | Window stays responsive |
| About 1 s between continuous sweeps | Next sweep starts immediately | Faster updates |
| Numbers shown with thousands separators | Plain numbers, 2 decimals | Simpler to read and copy |

Everything else, including the formulas' quirks (absolute values, clamping order), follows the original so that both programs show the same numbers.

## Structure

One Rust program, with the code split so that only one part knows about the interface:

| Part | Responsibility | Depends on |
|---|---|---|
| `device` | Serial port, sweep and generator commands, parking, port search. Includes a simulated device that returns a plausible antenna response, for tests and for running with no hardware. | nothing else in the program |
| `calc` | Pure maths: readings to RL, phase, reflection value, SWR, Z, L, C; markers; SWR minimum; bandwidth and Q; cable loss and length; phase-sign estimate; LC calculator. | nothing |
| `cal` | Calibration tables, loading and saving them, looking up the value for a frequency. | nothing |
| `sweep` | Runs sweeps on a background thread so the window never freezes. Sends results and progress to the interface, takes start/stop commands. | `device`, `cal`, `calc` |
| `store` | Settings, band presets, sweep files, CSV and PNG export. | `calc`, `cal` |
| `ui` | The egui window: menus, panels, graph, dialogs, text lookup table. | all of the above |

Libraries **(my call)**: `eframe`/`egui` and `egui_plot` for the interface, `serialport` for the device, `rfd` for file dialogs, `serde` and `toml` for files, `image` for PNG, `directories` for settings paths.

A new view such as the Smith chart is one new file in `ui` that reads the same sweep data.

## Testing

- `calc`: unit tests with values worked out by hand from the formulas above, and with the recorded open-port sweep (calibrated against itself it must read 0 dB at every point).
- `cal`: lookup and save-then-load tests.
- `device`: command-format tests (exact bytes for a given start, stop and samples, with and without the step-bug option and an extender) and reply parsing against the recorded sweeps, all through the simulated device.
- `store`: save-then-load tests; CSV checked against the original's header and number format.
- `ui`: checked by hand on the real device. Where the original still runs under CrossOver with the device, the two are compared on the same load. Stop points for the user: a 50-ohm load, an antenna, a cable of known length.

## Packaging

| System | Output | How |
|---|---|---|
| Linux | `.deb` and AppImage | Built on the laptop |
| Windows | Zip containing the `.exe` | Cross-compiled from the laptop. No installer in version one **(my call)**. |
| macOS | Unsigned `.app` in a `.dmg` | GitHub Actions macOS runner. Only if the user confirms they want it and the project is on GitHub. |

## Left to the builder

- Program name. Working name: `minivna` **(my call)**.
- Licence: none set until the user chooses one.
- Exact egui layout details, fonts and spacing, as long as the panels and controls above are present.
- Whether the graph uses `egui_plot` throughout or custom painting where the original's dual axes need it.
- Layout of the Cable Loss, Cable Length, Transmission, Generator, calibration and DDS screens: the original's form definitions can be printed with the tools named in the findings file, and are the guide.
- The meaning of the "Visual" boxes.

## Open questions for the user

- Does the original still work with the device under CrossOver today? (Only needed for side-by-side comparison.)
- Is macOS wanted in version one, later, or not at all?
- Licence.
