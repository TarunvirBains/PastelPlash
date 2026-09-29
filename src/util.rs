//! Small helpers shared across modules.

use std::time::Duration;

/// A duration as whole milliseconds, for log lines.
pub fn ms(d: Duration) -> String {
    format!("{:.0}ms", d.as_secs_f64() * 1000.0)
}

/// Runs `f(state, item)` for every item on `workers` plain threads (all cores when 0), each with
/// its own `init()` state, and returns the results in item order.
///
/// Per-file work waits on shared budgets (the memory budget for enlarged images, the GPU slots)
/// while it holds another, and runs nested rayon operations (resampling, analysis) in between.
/// On a rayon worker such a nested operation may steal another file's job while the budget is
/// still held; that job then waits for the budget its own thread holds, and the run hangs (as
/// it did with a few dozen enlarged pre-rendered backgrounds in flight). Plain threads never
/// steal, so files are driven from them and only the nested operations run on the pool.
pub fn map_on_threads<T: Sync, S, R: Send>(
    items: &[T],
    workers: usize,
    init: impl Fn() -> S + Sync,
    f: impl Fn(&mut S, &T) -> R + Sync,
) -> Vec<R> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workers = match workers {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    }
    .min(items.len())
    .max(1);
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut state = init();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    *results[i].lock().unwrap() = Some(f(&mut state, item));
                }
            });
        }
    });
    results
        .into_iter()
        .map(|r| r.into_inner().unwrap().expect("every item ran"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::map_on_threads;
    use rayon::prelude::*;
    use std::sync::{Condvar, Mutex, mpsc};
    use std::time::Duration;

    /// A budget of one: the pattern of the enlarged-image memory budget and the GPU slots.
    struct Budget(Mutex<bool>, Condvar);

    impl Budget {
        fn with<R>(&self, f: impl FnOnce() -> R) -> R {
            let mut busy = self.0.lock().unwrap();
            while *busy {
                busy = self.1.wait(busy).unwrap();
            }
            *busy = true;
            drop(busy);
            let r = f();
            *self.0.lock().unwrap() = false;
            self.1.notify_all();
            r
        }
    }

    /// Each item takes the budget, then runs nested rayon work while holding it.
    fn work(budget: &Budget, i: &u64) -> u64 {
        budget.with(|| (0..20_000u64).into_par_iter().map(|k| k % 7 + i).sum())
    }

    #[test]
    fn items_run_in_order_on_plain_threads() {
        let items: Vec<u64> = (0..50).collect();
        let out = map_on_threads(
            &items,
            4,
            || 0u64,
            |n, &i| {
                *n += 1;
                i * 2
            },
        );
        assert_eq!(out, (0..50).map(|i| i * 2).collect::<Vec<_>>());
    }

    #[test]
    fn nested_rayon_work_under_a_held_budget_never_deadlocks() {
        // Driven from rayon workers (par_iter), a worker holding the budget may steal another
        // item inside its nested par_iter and wait on the budget it holds itself. Plain threads
        // never steal: this completes.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let budget = Budget(Mutex::new(false), Condvar::new());
            let items: Vec<u64> = (0..64).collect();
            let out = map_on_threads(&items, 8, || (), |(), i| work(&budget, i));
            tx.send(out.len()).unwrap();
        });
        let n = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("deadlocked");
        assert_eq!(n, 64);
    }
}

/// Synthetic images for unit tests.
#[cfg(test)]
pub(crate) mod test_images {
    use crate::image::{Image, SourceColor, SourceFormat};

    pub fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Image {
        Image {
            width: w,
            height: h,
            pixels: (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| f(x, y))
                .collect(),
            source: SourceFormat {
                color: SourceColor::Rgba,
                bit_depth: 8,
                has_alpha: true,
            },
            source_scale: None,
            tint_safe: None,
        }
    }

    /// Deterministic hash noise in 0..1.
    pub fn noise(x: u32, y: u32) -> f32 {
        let mut v = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B);
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        (v & 0xFFFF) as f32 / 65535.0
    }
}
