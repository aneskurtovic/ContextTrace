//! Manual performance probe for the desktop discovery-to-context workflow.
//!
//! Usage:
//! `cargo run -p ct-runtime --release --example desktop_perf -- <session-id>`
//!
//! It prints timings and aggregate counts only. No prompt, event, path, label or
//! other session content crosses stdout.

use ct_application::SessionFilter;
use ct_domain::ports::TokenEstimator;
use std::hint::black_box;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args()
        .nth(1)
        .ok_or("usage: desktop_perf <session-id-or-prefix>")?;

    let total_started = Instant::now();
    let runtime_started = Instant::now();
    let runtime = ct_runtime::build();
    let runtime_time = runtime_started.elapsed();

    let discovery_started = Instant::now();
    let sessions = runtime.app.list_sessions(&SessionFilter::default());
    let discovery_time = discovery_started.elapsed();

    let load_started = Instant::now();
    let (session, resolved) = runtime.app.load(&id)?;
    let load_time = load_started.elapsed();

    let calibration_started = Instant::now();
    let (calibrated, _) = ct_runtime::calibrate_session(&runtime.app, &session, resolved.binding);
    let calibration_time = calibration_started.elapsed();
    let estimator: &dyn TokenEstimator = match calibrated.as_ref() {
        Some(estimator) => estimator,
        None => runtime.app.binding_estimator(resolved.binding),
    };

    let peak = runtime
        .app
        .peak_turn(&session)
        .ok_or("session has no measured prompt turn")?;
    let first_snapshot_started = Instant::now();
    let first = runtime
        .app
        .snapshot_with(&session, resolved.binding, peak, estimator)?;
    let first_snapshot_time = first_snapshot_started.elapsed();
    black_box(first.total());

    let alternate = session
        .turns()
        .iter()
        .rev()
        .find(|turn| turn.number != peak && turn.prompt_tokens().is_some())
        .map(|turn| turn.number)
        .unwrap_or(peak);
    let cached_snapshot_started = Instant::now();
    let cached = runtime
        .app
        .snapshot_with(&session, resolved.binding, alternate, estimator)?;
    let cached_snapshot_time = cached_snapshot_started.elapsed();
    black_box(cached.total());

    println!("sessions={}", sessions.len());
    println!("agent={}", session.agent());
    println!("turns={}", session.turn_count());
    print_timing("runtime", runtime_time);
    print_timing("discovery", discovery_time);
    print_timing("load", load_time);
    print_timing("calibration", calibration_time);
    print_timing("first_snapshot", first_snapshot_time);
    print_timing("cached_turn_switch", cached_snapshot_time);
    print_timing("cold_total", total_started.elapsed());
    Ok(())
}

fn print_timing(label: &str, duration: Duration) {
    println!("{label}_ms={:.1}", duration.as_secs_f64() * 1_000.0);
}
