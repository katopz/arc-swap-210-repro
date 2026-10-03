//! Minimal reproducer attempt for vorner/arc-swap#210.
//!
//! One `ArcSwap<Inner>` per test, ONE writer thread storing a fresh `Arc` with a
//! monotonically increasing version, ONE reader thread taking `load()` guards and
//! asserting (a) version monotonicity across successive guards and (b) internal
//! coherence of each snapshot. Two such tests run concurrently in one libtest
//! binary (`--test-threads=2`). Shape, sizes and iteration counts mirror the
//! original tests exactly.

use arc_swap::{ArcSwap, Guard};
use std::sync::Arc;

pub struct Inner {
    pub version: u64,
    pub a: Vec<f32>,
    pub b: Vec<f32>,
}

/// Single-writer versioned slot: the whole `(version, a, b)` triple moves as one pointer.
pub struct Slot {
    inner: ArcSwap<Inner>,
}

impl Slot {
    pub fn new(a_len: usize, b_len: usize) -> Self {
        Self {
            inner: ArcSwap::from_pointee(Inner {
                version: 0,
                a: vec![0.0; a_len],
                b: vec![0.0; b_len],
            }),
        }
    }

    /// Single writer only: version = current + 1, published as one pointer store.
    pub fn update(&self, a: Vec<f32>, b: Vec<f32>) -> u64 {
        let version = self.inner.load().version + 1;
        self.inner.store(Arc::new(Inner { version, a, b }));
        version
    }

    pub fn load(&self) -> Guard<Arc<Inner>> {
        self.inner.load()
    }

    pub fn version(&self) -> u64 {
        self.inner.load().version
    }
}

#[cfg(all(test, feature = "tracking_alloc"))]
mod tracking_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    pub struct Tracking;

    unsafe impl GlobalAlloc for Tracking {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            COUNT.with(|c| {
                let (n, b) = c.get();
                c.set((n.wrapping_add(1), b.wrapping_add(layout.size())));
            });
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Tracking = Tracking;
}

#[cfg(test)]
mod tests {
    use super::Slot;
    use std::sync::{Arc, Barrier};
    use std::thread;

    // Same dimensions as the original: a = 4*8, b = 16*4.
    const A_LEN: usize = 4 * 8;
    const B_LEN: usize = 16 * 4;

    // Miri cannot run 100k iterations; scale down there.
    const UPDATE_READ_TOTAL: usize = if cfg!(miri) { 200 } else { 1_000 };
    const NO_TORN_TOTAL: usize = if cfg!(miri) { 400 } else { 100_000 };
    const SPIN_LIMIT: u64 = if cfg!(miri) { 10_000 } else { 1_000_000 };

    /// Bounded wait for the writer's first update (otherwise a reader that
    /// outruns the writer under load false-fires the final sanity assert).
    fn wait_first_update(slot: &Slot) {
        let mut waits = 0u64;
        while slot.version() == 0 && waits < SPIN_LIMIT {
            std::hint::spin_loop();
            waits += 1;
        }
    }

    /// Writer fills a == b == i.
    #[test]
    fn concurrent_lora_update_read() {
        let slot = Arc::new(Slot::new(A_LEN, B_LEN));
        let total = UPDATE_READ_TOTAL;
        let start = Arc::new(Barrier::new(2));

        let (ws, wb) = (Arc::clone(&slot), Arc::clone(&start));
        let writer = thread::spawn(move || {
            wb.wait();
            for i in 1..=total {
                let v = ws.update(vec![i as f32; A_LEN], vec![i as f32; B_LEN]);
                assert!(v == i as u64, "version should be {i}, got {v}");
            }
        });

        let (rs, rb) = (Arc::clone(&slot), Arc::clone(&start));
        let reader = thread::spawn(move || {
            rb.wait();
            wait_first_update(&rs);
            let mut last = 0u64;
            for _ in 0..total {
                let g = rs.load();
                let addr = std::ptr::from_ref(&**g) as usize;
                let v = g.version;
                let fa = g.a.first().copied().unwrap_or(0.0);
                let fb = g.b.first().copied().unwrap_or(0.0);
                assert!(
                    v >= last,
                    "MYSTERY version went backwards: {last} -> {v} (inner={addr:#x}, tid={:?})",
                    thread::current().id()
                );
                last = v;
                if v > 0 {
                    assert!(g.a.iter().all(|&x| x == fa), "MYSTERY A not uniform at v={v} (inner={addr:#x})");
                    assert!(g.b.iter().all(|&x| x == fb), "MYSTERY B not uniform at v={v} (inner={addr:#x})");
                    assert!(
                        fa == fb,
                        "MYSTERY torn/foreign read at v={v}: a={fa} b={fb} (inner={addr:#x}, tid={:?})",
                        thread::current().id()
                    );
                }
            }
            last
        });

        writer.join().expect("writer panicked");
        let last = reader.join().expect("reader panicked");
        assert!(last > 0, "MUNDANE reader should have seen at least one update");
    }

    /// Writer fills a == i, b == 2*i — so `a == b` (the sibling's pattern) is impossible here.
    #[test]
    fn concurrent_lora_no_torn_read() {
        let slot = Arc::new(Slot::new(A_LEN, B_LEN));
        let total = NO_TORN_TOTAL;

        let ws = Arc::clone(&slot);
        let writer = thread::spawn(move || {
            for i in 1..=total {
                let v = ws.update(vec![i as f32; A_LEN], vec![(i as f32) * 2.0; B_LEN]);
                assert!(v == i as u64, "version should be {i}, got {v}");
            }
        });

        let rs = Arc::clone(&slot);
        let reader = thread::spawn(move || {
            wait_first_update(&rs);
            let mut consistent = 0u64;
            let mut last = 0u64;
            for _ in 0..total {
                let g = rs.load();
                let addr = std::ptr::from_ref(&**g) as usize;
                if g.version == 0 {
                    continue;
                }
                assert!(
                    g.version >= last,
                    "MYSTERY version went backwards: {last} -> {} (inner={addr:#x}, tid={:?})",
                    g.version,
                    thread::current().id()
                );
                last = g.version;
                let fa = g.a.first().copied().unwrap_or(0.0);
                let fb = g.b.first().copied().unwrap_or(0.0);
                let (ea, eb) = (g.version as f32, g.version as f32 * 2.0);
                assert!(
                    fa == ea && fb == eb,
                    "MYSTERY torn/foreign read at v={}: a={fa} (expected {ea}), b={fb} (expected {eb}) (inner={addr:#x}, tid={:?})",
                    g.version,
                    thread::current().id()
                );
                consistent += 1;
            }
            consistent
        });

        writer.join().expect("writer panicked");
        let consistent = reader.join().expect("reader panicked");
        assert!(consistent > 0, "MUNDANE reader should have seen at least one consistent snapshot");
    }
}
