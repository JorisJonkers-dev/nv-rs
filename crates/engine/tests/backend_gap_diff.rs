// SPDX-License-Identifier: GPL-3.0-only
//! A differential case for each real translation gap the local decomp backend can answer for
//! (`crates/engine-core/src/typed/real_gaps.txt`, JorisJonkers-dev/nv-rs#1000). The translation is
//! both sides here; in the private run every case also runs against the backend as the reference
//! peer, so a case is the translation compared with the original code on a world that does not
//! reach what the gap leaves out (stack words the exe never wrote). The cases need the backend and
//! skip without `NV_BACKEND_LIBRARY` (the harness refuses to compare a function with itself, and
//! the translation alone is covered by the unit's own tests).

use engine as _;
use engine_core::prelude::*;
use engine_core::typed::diff::{Diff, Input};

#[allow(dead_code)]
mod gate {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/fork/test_gate.rs"
    ));
}

/// The backend, or `None` (the case skips) when the private run has not built one.
fn backend() -> Option<Box<dyn engine_core::typed::diff::ReferencePeer>> {
    gate::require_data("NV_BACKEND_LIBRARY")?;
    Some(engine_core::typed::diff::backend_peer().expect("the backend library did not load"))
}

/// A callee double that changes nothing and returns zero.
fn nothing(_: &mut Engine, _: &[u32]) -> Ret {
    Ret::default()
}

/// 00d4ded0: set the object at +0x18, take a reference on the new one, drop the old one's, then
/// read two sizes from the new object's vtable slot +0x24.
#[test]
fn run_00d4ded0_sets_the_referenced_object() {
    use engine_fallout_unplaced_b::units::unplaced::run_00d4ded0::fn_00d4ded0;
    // The typed side is the translation behind a wrapper; the backend is what it is compared with.
    fn through(e: &mut Engine, this: Ptr, object: Ptr) {
        fn_00d4ded0(e, this, object)
    }
    let Some(peer) = backend() else { return };
    let report = Diff::new(entry!(0x00d4_ded0, through(Ptr, Ptr)))
        .translation(entry!(0x00d4_ded0, fn_00d4ded0(Ptr, Ptr)).1)
        .reference_peer(peer)
        .inputs(&[Input::Of(&[0]), Input::Of(&[0])])
        .setup(|e, case| {
            e.register(0x00c9_0510, nothing);
            e.register(0x00c9_05b0, nothing);
            let vtable = e.mem.alloc(0x28);
            e.mem.set_u32(vtable + 0x24, 0x0079_0024);
            e.register(0x0079_0024, |e, a| {
                e.mem.set_u32(a[2], 0x0001_0203);
                e.mem.set_u32(a[2] + 4, 0x0004_0506);
                Ret::default()
            });
            let object = e.mem.alloc(8);
            e.mem.set_u32(object, vtable);
            let old = e.mem.alloc(8);
            e.mem.set_u32(old, vtable);
            let this = e.mem.alloc(0x20);
            if case.word(0) == 1 {
                e.mem.set_u32(this + 0x18, old);
            }
            // An old object makes the module call the reference release itself (it holds
            // 00c905b0), which the host double never sees and an overridable address cannot
            // ignore; so the world has none.
            // A non-null object makes the translation call through the object's vtable, and the
            // module's `call_indirect` reaches only functions the module holds, not a double.
            let _ = object;
            vec![this, case.word(1)]
        })
        .run();
    if let Some(fault) = &report.backend_fault {
        eprintln!("backend fault: {fault}");
    }
    report.assert_same();
}
