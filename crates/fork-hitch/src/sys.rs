//! Process numbers from the operating system, cheaply: `getrusage` on Linux (one system call, no
//! allocation), zeros elsewhere.

use crate::model::Usage;

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
pub fn usage() -> Usage {
    #[repr(C)]
    struct Rusage {
        utime: [i64; 2],
        stime: [i64; 2],
        fields: [i64; 14],
    }
    extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
    let mut r = Rusage {
        utime: [0; 2],
        stime: [0; 2],
        fields: [0; 14],
    };
    // SAFETY: `getrusage(RUSAGE_SELF, p)` writes one `struct rusage` through `p`; `Rusage` is
    // that struct's layout on 64-bit Linux (two timevals, then fourteen longs).
    let ok = unsafe { getrusage(0, &mut r) } == 0;
    if !ok {
        return Usage::default();
    }
    let us = |t: [i64; 2]| (t[0].max(0) as u64) * 1_000_000 + t[1].max(0) as u64;
    Usage {
        maxrss_kb: r.fields[0].max(0) as u64,
        minflt: r.fields[4].max(0) as u64,
        majflt: r.fields[5].max(0) as u64,
        cpu_us: us(r.utime) + us(r.stime),
    }
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
pub fn usage() -> Usage {
    Usage::default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_process_has_faults_and_cpu_on_linux() {
        if cfg!(all(target_os = "linux", target_pointer_width = "64")) {
            let a = super::usage();
            let v: Vec<u8> = vec![1; 8 << 20];
            std::hint::black_box(&v);
            let b = super::usage();
            assert!(a.minflt > 0 && a.maxrss_kb > 0);
            assert!(b.minflt >= a.minflt);
        }
    }
}
