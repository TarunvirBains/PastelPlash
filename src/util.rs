//! Small helpers shared across modules.

use std::cell::Cell;
use std::sync::{Condvar, Mutex};
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

/// A budget of units (texels of enlarged images in flight) shared by concurrent work.
/// [`Budget::acquire`] blocks until the units fit; a request larger than the whole budget is
/// cut to it, so it still runs, alone.
///
/// No thread waits while it holds part of the budget, so the waits always end: whoever holds
/// units is running and gives them back when done.
pub struct Budget {
    cap: u64,
    state: Mutex<BudgetState>,
    cv: Condvar,
}

struct BudgetState {
    /// Units held, reservations included.
    used: u64,
    /// Items waiting in [`Budget::acquire`] for more than their reservation (they gave it back
    /// meanwhile): no weighted item starts until they have their units.
    growing: usize,
}

/// Units held by a [`Budget`] until dropped.
pub struct BudgetGuard<'a> {
    budget: &'a Budget,
    units: u64,
}

thread_local! {
    /// The budget and the units the scheduler reserved for the item this thread is running
    /// ([`map_on_threads_budgeted`]): the item's own [`Budget::acquire`] calls are covered.
    static PREPAID: Cell<(*const Budget, u64)> = const { Cell::new((std::ptr::null(), 0)) };
}

impl Budget {
    pub const fn new(cap: u64) -> Self {
        Self {
            cap,
            state: Mutex::new(BudgetState {
                used: 0,
                growing: 0,
            }),
            cv: Condvar::new(),
        }
    }

    /// Waits until `units` fit, then holds them. Units the scheduler already reserved for this
    /// thread's item count toward them. When the item asks for more than its reservation and
    /// the rest doesn't fit, it gives the reservation back while it waits and then takes all
    /// `units` at once: waiting for the rest while holding the reservation could deadlock (two
    /// items that each hold 4 of 10 and ask for 8 would wait for each other forever). An item
    /// holds one guard at a time.
    pub fn acquire(&self, units: u64) -> BudgetGuard<'_> {
        let want = units.min(self.cap);
        let prepaid = match PREPAID.get() {
            (b, n) if std::ptr::eq(b, self) => n,
            _ => 0,
        };
        let extra = want.saturating_sub(prepaid);
        let mut state = self.state.lock().unwrap();
        if extra > 0 && state.used + extra > self.cap {
            state.used -= prepaid;
            state.growing += 1;
            self.cv.notify_all();
            while state.used + want > self.cap {
                state = self.cv.wait(state).unwrap();
            }
            // The reservation is held again, with the rest on top.
            state.used += prepaid;
            state.growing -= 1;
        }
        state.used += extra;
        BudgetGuard {
            budget: self,
            units: extra,
        }
    }
}

impl Drop for BudgetGuard<'_> {
    fn drop(&mut self) {
        if self.units > 0 {
            self.budget.state.lock().unwrap().used -= self.units;
            self.budget.cv.notify_all();
        }
    }
}

/// Like [`map_on_threads`], for items that hold part of `budget` while they run
/// (`weights[i]` units, 0 for none): items start in the given order, except that a worker
/// skips items that don't fit in the budget right now and takes the next one that does,
/// instead of waiting. List the heaviest items first: they start as early as the budget
/// allows, and the light ones fill the workers around them.
pub fn map_on_threads_budgeted<T: Sync, S, R: Send>(
    items: &[T],
    weights: &[u64],
    budget: &Budget,
    workers: usize,
    init: impl Fn() -> S + Sync,
    f: impl Fn(&mut S, &T) -> R + Sync,
) -> Vec<R> {
    use std::time::Duration;
    assert_eq!(items.len(), weights.len());
    let workers = match workers {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    }
    .min(items.len())
    .max(1);
    // Items not started yet, in order.
    let pending = Mutex::new((0..items.len()).collect::<Vec<usize>>());
    let results: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    // The next item that fits (with its reservation), `None` when every item has started, or
    // `Some(None)` when items remain but none fits yet.
    let take = || -> Option<Option<(usize, u64)>> {
        let mut pending = pending.lock().unwrap();
        if pending.is_empty() {
            return None;
        }
        let mut state = budget.state.lock().unwrap();
        // While a running item waits to grow its reservation, the units released go to it
        // rather than to new items (which could keep it waiting until the light ones ran out).
        let pos = pending.iter().position(|&i| {
            let want = weights[i].min(budget.cap);
            want == 0 || (state.growing == 0 && state.used + want <= budget.cap)
        });
        Some(pos.map(|p| {
            let i = pending.remove(p);
            let want = weights[i].min(budget.cap);
            state.used += want;
            (i, want)
        }))
    };
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut state = init();
                while let Some(next) = take() {
                    let Some((i, want)) = next else {
                        // Wait for a release (the timeout covers one that came before the wait).
                        let held = budget.state.lock().unwrap();
                        drop(
                            budget
                                .cv
                                .wait_timeout(held, Duration::from_millis(50))
                                .unwrap(),
                        );
                        continue;
                    };
                    let guard = BudgetGuard {
                        budget,
                        units: want,
                    };
                    PREPAID.set((budget, want));
                    let r = f(&mut state, &items[i]);
                    PREPAID.set((std::ptr::null(), 0));
                    drop(guard);
                    *results[i].lock().unwrap() = Some(r);
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

    /// Runs `f` on a thread and fails if it takes longer than a minute (a deadlock).
    fn within_a_minute<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || tx.send(f()).unwrap());
        rx.recv_timeout(Duration::from_secs(60))
            .expect("deadlocked")
    }

    #[test]
    fn budgeted_items_stay_within_the_budget_and_light_ones_fill_in() {
        use super::{Budget, map_on_threads_budgeted};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Instant;
        static BUDGET: Budget = Budget::new(10);
        let out = within_a_minute(|| {
            // Two heavy items that can't run together, then light ones.
            let weights: Vec<u64> = [8, 8].into_iter().chain([0, 1, 2].repeat(10)).collect();
            let items: Vec<usize> = (0..weights.len()).collect();
            let in_use = AtomicU64::new(0);
            let t0 = Instant::now();
            let out = map_on_threads_budgeted(
                &items,
                &weights,
                &BUDGET,
                4,
                || (),
                |(), &i| {
                    let now = in_use.fetch_add(weights[i], Ordering::SeqCst) + weights[i];
                    assert!(now <= 10, "{now} units in use");
                    let start = t0.elapsed();
                    // What the item itself asks for is covered by its reservation.
                    let _held = BUDGET.acquire(weights[i]);
                    std::thread::sleep(Duration::from_millis(if weights[i] == 8 {
                        300
                    } else {
                        5
                    }));
                    in_use.fetch_sub(weights[i], Ordering::SeqCst);
                    (i, start, t0.elapsed())
                },
            );
            (out, BUDGET.state.lock().unwrap().used)
        });
        let (runs, left) = out;
        assert_eq!(left, 0);
        assert!(runs.iter().enumerate().all(|(k, r)| r.0 == k), "item order");
        // The light items ran while the second heavy one waited for the first.
        let second_heavy = runs[1].1;
        let lights_done = runs[2..].iter().map(|r| r.2).max().unwrap();
        assert!(
            lights_done < second_heavy,
            "{lights_done:?} vs {second_heavy:?}"
        );
    }

    #[test]
    fn items_growing_their_reservations_at_once_take_turns() {
        use super::{Budget, map_on_threads_budgeted};
        use std::sync::Barrier;
        static BUDGET: Budget = Budget::new(10);
        let left = within_a_minute(|| {
            // Both items hold their reservation of 4 before either asks for 8: neither can grow
            // while the other holds its 4, so one gives its reservation back and waits.
            let both_reserved = Barrier::new(2);
            map_on_threads_budgeted(
                &[8u64, 8],
                &[4, 4],
                &BUDGET,
                2,
                || (),
                |(), &units| {
                    both_reserved.wait();
                    let _held = BUDGET.acquire(units);
                    let used = BUDGET.state.lock().unwrap().used;
                    assert!(used <= 10, "{used} units in use");
                },
            );
            BUDGET.state.lock().unwrap().used
        });
        assert_eq!(left, 0);
    }

    #[test]
    fn contended_reservations_never_deadlock_or_overrun_the_budget() {
        use super::{Budget, map_on_threads_budgeted};
        use std::sync::atomic::{AtomicU64, Ordering};
        static BUDGET: Budget = Budget::new(10);
        within_a_minute(|| {
            // A fixed xorshift sequence: random-looking reservations, requests and sleeps.
            let mut seed = 0x9e37_79b9_7f4a_7c15u64;
            let mut next = move |n: u64| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed % n
            };
            for _ in 0..200 {
                let n = 4 + next(8) as usize;
                // (reservation, units asked for (beyond the reservation or not), sleep in µs)
                let items: Vec<(u64, u64, u64)> = (0..n)
                    .map(|_| {
                        let reserved = next(7);
                        (reserved, reserved + next(6), next(400))
                    })
                    .collect();
                let weights: Vec<u64> = items.iter().map(|it| it.0).collect();
                let in_use = AtomicU64::new(0);
                map_on_threads_budgeted(
                    &items,
                    &weights,
                    &BUDGET,
                    4,
                    || (),
                    |(), &(reserved, asked, sleep)| {
                        std::thread::sleep(Duration::from_micros(sleep));
                        let held = BUDGET.acquire(asked);
                        // Requests beyond the budget are cut to it.
                        let units = asked.min(10).max(reserved);
                        let now = in_use.fetch_add(units, Ordering::SeqCst) + units;
                        assert!(now <= 10, "{now} units in use");
                        let used = BUDGET.state.lock().unwrap().used;
                        assert!(used <= 10, "{used} units held");
                        std::thread::sleep(Duration::from_micros(sleep));
                        in_use.fetch_sub(units, Ordering::SeqCst);
                        drop(held);
                    },
                );
                assert_eq!(BUDGET.state.lock().unwrap().used, 0);
            }
        });
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
