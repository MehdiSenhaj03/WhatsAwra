//! Where the window's time goes, for WHATSAWRA_TURNS: each second it prints
//! how many turns ran and how long each part of them took in all.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

thread_local! {
    static ON: Cell<Option<bool>> = const { Cell::new(None) };
    static SPENT: RefCell<BTreeMap<&'static str, (Duration, Duration, u32)>> =
        RefCell::default();
}

fn on() -> bool {
    ON.with(|on| match on.get() {
        Some(known) => known,
        None => {
            let known = std::env::var_os("WHATSAWRA_TURNS").is_some();
            on.set(Some(known));
            known
        }
    })
}

/// Runs `work`, counting its time under `name` when measuring.
pub fn span<R>(name: &'static str, work: impl FnOnce() -> R) -> R {
    if !on() {
        return work();
    }
    let started = Instant::now();
    let result = work();
    let took = started.elapsed();
    SPENT.with(|spent| {
        let mut spent = spent.borrow_mut();
        let entry = spent.entry(name).or_default();
        entry.0 += took;
        entry.1 = entry.1.max(took);
        entry.2 += 1;
    });
    result
}

/// What the last second spent, as one line per part, emptied for the next.
pub fn report() -> String {
    SPENT.with(|spent| {
        std::mem::take(&mut *spent.borrow_mut())
            .into_iter()
            .map(|(name, (total, longest, count))| {
                format!(
                    "  {name}: {:.1} ms in {count} (longest {:.1} ms)",
                    total.as_secs_f64() * 1000.0,
                    longest.as_secs_f64() * 1000.0
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}
