//! Manual performance probe for desktop discovery and context.
//!
//! Usage: `cargo run -p ct-runtime --release --example desktop_perf -- <session-id>`
//! Prints only timings and aggregate counts, never session content.
//!
//! Content analysis runs first; the plain load follows with a warmer cache.
//! The first read is not necessarily disk-cold because the OS cache persists
//! across processes. Record input size and cache state alongside measurements.
//! `content_analysis_path_total_ms` ends at the first rendered snapshot, before
//! the plain load and the separate cached turn switch.

use ct_application::SessionFilter;
use ct_domain::ports::TokenEstimator;
use std::hint::black_box;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args()
        .nth(1)
        .ok_or("usage: desktop_perf <session-id-or-prefix>")?;

    // The content-analysis path: doctor and `ct context` both run this, and
    // it is measured first, cold, on purpose -- see the module doc comment.
    // Timed from here (process start, before the runtime even exists) so
    // `content_analysis_path_total_ms` reflects everything a user actually
    // waits through, not just the load call.
    let path_total_started = Instant::now();

    let runtime_started = Instant::now();
    let runtime = ct_runtime::build();
    let runtime_time = runtime_started.elapsed();

    let discovery_started = Instant::now();
    let sessions = runtime.app.list_sessions(&SessionFilter::default());
    let discovery_time = discovery_started.elapsed();

    let content_analysis_load_started = Instant::now();
    let (session, resolved) = runtime.app.load_with_content_analysis(&id)?;
    let content_analysis_load_time = content_analysis_load_started.elapsed();

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

    // Everything a user waits through on the content-analysis path, from
    // process start to the first turn being rendered: runtime start,
    // discovery, the hashed/deflated load, calibration and the first
    // snapshot. This is the slowest path a user can reach, measured with the
    // OS page cache least warm (see the module doc comment). It stops here,
    // before the cached turn switch below, because switching turns is a
    // separate, later user action, not part of getting the first turn on
    // screen.
    let content_analysis_path_total = path_total_started.elapsed();

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

    // Plain load, measured second and therefore against a warm OS page
    // cache for this session's file -- the same bytes were just read above
    // by the content-analysis load. Not a fair cold-to-cold comparison; a
    // lower bound on the plain path's true cold cost, reported for
    // reference so the cost of content analysis is visible.
    let plain_load_started = Instant::now();
    let (plain_session, plain_resolved) = runtime.app.load(&id)?;
    let plain_load_time = plain_load_started.elapsed();
    black_box(plain_session.turn_count());
    black_box(plain_resolved.binding);

    println!("sessions={}", sessions.len());
    println!("agent={}", session.agent());
    println!("turns={}", session.turn_count());
    print_timing("runtime", runtime_time);
    print_timing("discovery", discovery_time);
    print_timing("load_content_analysis_cold", content_analysis_load_time);
    print_timing("calibration", calibration_time);
    print_timing("first_snapshot", first_snapshot_time);
    print_timing("cached_turn_switch", cached_snapshot_time);
    print_timing("content_analysis_path_total", content_analysis_path_total);
    print_timing("load_plain_warmcache", plain_load_time);
    Ok(())
}

fn print_timing(label: &str, duration: Duration) {
    println!("{label}_ms={:.1}", duration.as_secs_f64() * 1_000.0);
}
