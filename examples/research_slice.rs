//! The data program's P1 slice, end to end, on synthetic participants — into a folder you name:
//!
//! ```text
//! cargo run --release --example research_slice -- <folder>
//! python3 tools/research_loader.py <folder>/export
//! ```
//!
//! It grants the synthetic participants' consent, records their sessions through the gate (one
//! never consents: nothing of theirs is written), adds a retry's faults to the spool, builds the
//! episodes, exports what may go to this device for external research, and tries two exports the
//! gate refuses. Everything runs on this machine, on the synthetic clock; nothing is sent.
//! See `docs/data-program/DATA_CONTRACTS.md`.

use std::path::PathBuf;

use ms::research::synthetic;

fn main() {
    let Some(out) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: research_slice <folder>   (a new or empty folder)");
        std::process::exit(2);
    };
    if std::fs::read_dir(&out).is_ok_and(|mut d| d.next().is_some()) {
        eprintln!("{} is not empty; name a new folder", out.display());
        std::process::exit(2);
    }
    let run = match synthetic::run_slice(&out) {
        Ok(run) => run,
        Err(e) => {
            eprintln!("the slice stopped: {e}");
            std::process::exit(1);
        }
    };
    let r = &run.recorded;
    println!("SYNTHETIC data — generated participants, game_id \"synthetic\"; nothing was sent.");
    println!();
    println!(
        "recorded: {} events from {:?}; not recorded: {:?}; drafts refused: {:?}",
        r.written, r.opened, r.not_opened, r.rejected
    );
    println!(
        "redactions before writing (kinds, counts): {:?}",
        r.redactions
    );
    println!(
        "spool after a retry's faults: {} duplicate dropped, {} conflicting, quarantined {:?}",
        run.built.duplicates_dropped, run.built.conflicting_duplicates, run.built.quarantined
    );
    let m = &run.exported.manifest;
    println!();
    println!(
        "export {} (revision {}) for {} to {} — {} participants, {} sessions, {} episodes, {} events, {} observed hours",
        m.export_id,
        m.revision,
        m.purpose.code(),
        m.recipient_class.code(),
        m.counts.participants,
        m.counts.sessions,
        m.counts.episodes,
        m.counts.events,
        m.counts.observed_hours
    );
    println!("rows left out by the gate: {:?}", m.excluded);
    for file in &m.files {
        println!(
            "  {}  {}",
            file.sha256,
            run.export_dir.join(&file.path).display()
        );
    }
    println!(
        "  (manifest) {}",
        run.export_dir.join("manifest.json").display()
    );
    println!();
    for (name, refusal) in &run.refused {
        println!("refused export '{name}': {refusal}");
    }
    let x = &run.exported.metrics;
    println!();
    println!(
        "report: time to first success (median) {:?} ms over {} episodes; success with advice shown {}/{} attempts, without {}/{}; \
         attempts with help {}/{}; unobserved {}/{}, censored {}/{}; insufficient evidence: {}",
        x.ttfs_median_ms,
        x.ttfs_episodes,
        x.success_assisted.numerator,
        x.success_assisted.denominator,
        x.success_unassisted.numerator,
        x.success_unassisted.denominator,
        x.help_per_attempt.numerator,
        x.help_per_attempt.denominator,
        x.unobserved_rate.numerator,
        x.unobserved_rate.denominator,
        x.censored_rate.numerator,
        x.censored_rate.denominator,
        x.insufficient_evidence
    );
    println!();
    println!("research folder: {}", run.research_root.display());
    println!("consent ledger:  {}", run.ledger_path.display());
    println!(
        "verify and load: python3 tools/research_loader.py {}",
        run.export_dir.display()
    );
}
