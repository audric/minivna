//! Sweep the simulated antenna and write it as .mvna, CSV and PNG, then read the .mvna back.
//! Usage: cargo run --example export <directory>

use minivna::cal::Calibration;
use minivna::store::Settings;
use minivna::sweep::{spawn, Command, Event, PortChoice};
use minivna::ui::files;
use minivna::ui::state::{AppState, Big, Marker};

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: export <directory>"));
    let mut state = AppState::new(Settings::default(), Calibration::default());
    state.set_range(13_000_000, 15_000_000);
    let worker = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
    worker.tx.send(Command::Single(state.request())).unwrap();
    loop {
        let event = worker.rx.recv().unwrap();
        let done = matches!(event, Event::Sweep(_) | Event::Error(_));
        state.on_event(event);
        if done {
            break;
        }
    }
    state.place_marker(Marker::M1, 14.1e6);
    state.place_marker(Marker::M2, 14.4e6);
    state.show_swr_min = true;
    state.big = Some(Big::M1);
    for written in [
        files::save_sweep(&state, &dir.join("sim")),
        files::save_csv(&state, &dir.join("sim")),
        files::save_png(&state, &dir.join("sim"), 900, 560),
    ] {
        println!("{}", written.expect("write").display());
    }
    // The same sweep as a Smith chart, with the sign of the reactance estimated.
    state.view = minivna::ui::state::View::Smith;
    state.big = None;
    state.settings.signed_phase = true;
    let r = state.result.take().unwrap();
    state.result = Some(minivna::sweep::build_result(r.request, r.raw, r.cal, true, minivna::device::nwt::DB_PER_COUNT_DEFAULT));
    println!("{}", files::save_png(&state, &dir.join("sim-smith"), 900, 560).expect("write").display());
    let mut again = AppState::new(Settings::default(), Calibration::default());
    files::open_sweep(&mut again, &dir.join("sim.mvna")).expect("open");
    println!("reopened: {} points, markers {:?} {:?}, row {:?}", again.points().len(), again.m1, again.m2, again.marker_row(again.m1.unwrap()));
    worker.tx.send(Command::Shutdown).unwrap();
}
