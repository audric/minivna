# minivna

A desktop program for the original **mRS miniVNA** antenna analyser (0.1–180 MHz, USB,
FTDI serial chip). It is a rewrite by IW1GEU (Audric) of IG_miniVNA by F4CLB (Jean-Louis), whose source was lost; the
measurement methods and numbers follow that program. The miniVNA Pro is not supported.

What it does: SWR, |Z|, return loss, phase, Rs and |Xs| curves with two markers, a Smith
chart view of the same sweep, bandwidth
and Q, cable loss, cable length (with an automatic search), transmission loss, a signal
generator, band presets, frequency extenders (x4, x8, x16), reflection, transmission and
oscillator calibration, and saving a sweep as a `.mvna` file, CSV (ZPlots layout) or PNG.

## Running

    minivna            # uses the saved port, or searches for the device
    minivna --sim      # a simulated antenna at 14.1 MHz, no device needed
    minivna --smith    # start on the Smith chart
    minivna --sweep    # make one sweep right after starting

Calibrate once before measuring: *Configure → Calibrate Reflection*, with nothing
connected between DUT and DET.

**Linux:** your user must be allowed to use the serial port:
`sudo usermod -aG dialout $USER`, then log out and in again.

**Windows:** the device needs the FTDI virtual COM port driver (Windows usually installs it
by itself; otherwise get it from ftdichip.com).

**macOS:** the program is not signed. The first time, right-click the app and choose Open.

## Keys and mouse

| | |
|---|---|
| Space / F12 / Esc | one sweep / continuous sweep / stop |
| Left / right click on the graph | place marker 1 / marker 2 |
| Ctrl or Shift + left / right click | step marker 1 or 2 one point down / up |
| Alt + left / right click | marker 1 / 2 to the band's default position |
| Middle click | set or remove the memo line |
| F1–F10, Ctrl+F1–F9 | band presets |
| Ctrl+A / P / L / T / G | mode: antenna, cable loss, cable length, transmission, generator |
| Ctrl+D / Ctrl+F | 160–10 m / 1–180 MHz |
| Ctrl+O / Ctrl+S / Ctrl+Q | open / save / exit |
| Ctrl+C / Ctrl+N | save as CSV / save graph as PNG |

The shortcuts do not act while the cursor is in a text box.

## Where things are stored

Settings (`settings.toml`) and calibration (`calibration.toml`) are in the per-user
configuration folder: `~/.config/minivna/` on Linux, `%APPDATA%\minivna\config\` on Windows,
`~/Library/Application Support/minivna/` on macOS. Both are plain text.

## Building

Needs a Rust toolchain (rustup.rs). On Debian/Ubuntu also:
`sudo apt install libudev-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev`.

    cargo test
    cargo build --release          # target/release/minivna
    packaging/build-linux.sh       # AppImage in dist/ (needs appimagetool)
    packaging/build-windows.sh     # Windows zip in dist/, built on Linux (needs mingw-w64)

`.github/workflows/build.yml` builds Linux, Windows and macOS on GitHub.

## Layout of the source

- `src/device/` — serial protocol and port handling, plus a simulated device
- `src/calc/` — the maths: raw readings to SWR, impedance, cable tools
- `src/cal.rs` — calibration tables
- `src/sweep.rs` — the background thread that talks to the device
- `src/store/` — settings, sweep files, CSV and PNG
- `src/ui/` — the window: `state.rs` holds all logic and is tested without a window,
  `plot.rs` turns it into shapes, the rest paints
- `docs/reverse-engineering/` — what was read from the original program
- `docs/hardware-checks.md` — what was measured on a real device

## A second instrument: the 35-4400 MHz sweeper

The program can also drive the small "35-4400 MHz spectrum analyzer / sweep source"
boxes that speak the NWT protocol (tested with one unit, firmware byte 119). Choose it
in Configure, Device and Com Port. It has a detector without phase, so with it the
program offers:

- transmission loss (output, device under test, input) with markers, bands, zoom,
  continuous sweep and CSV/PNG export;
- the generator.

SWR, impedance, phase, the Smith chart and the cable modes need the miniVNA and are
switched off. Calibrate with Configure, Calibrate Reference (output joined to input).
The dB scale is approximate until the detector's slope has been measured; see
`docs/reverse-engineering/nwt-sweeper-findings.md`.
