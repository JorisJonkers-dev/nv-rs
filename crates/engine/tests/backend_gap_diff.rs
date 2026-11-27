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

use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard};

/// The loaded module is one guest reservation for the whole process, so two cases cannot run
/// against it at once: each case holds this until its test thread ends.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

thread_local! {
    static HELD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
}

/// The backend, or `None` (the case skips) when the private run has not built one.
fn backend() -> Option<Box<dyn engine_core::typed::diff::ReferencePeer>> {
    gate::require_data("NV_BACKEND_LIBRARY")?;
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    HELD.with(|h| *h.borrow_mut() = Some(guard));
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

/// A particle shader with no target (the 16-bit target count at +0x126 is zero): both spawn
/// functions store the zero point as the position and the velocity of particle `index`, the
/// timer between them. The gap (stack words stored for a null first target) is not reached.
fn particles_without_targets(e: &mut Engine, case: &engine_core::typed::diff::Case) -> Vec<u32> {
    let this = e.mem.alloc(0x130);
    let data = e.mem.alloc(0x30 * 4);
    e.mem.set_u32(this + 0x60, data);
    e.mem.set_u32(this + 0x104, 0x3f80_0000);
    e.mem.set_u16(this + 0x126, 0);
    // The zero point (`011f426c`) holds a vector the world picks, so a copy is visible.
    e.map(0x011f_426c, 0x10);
    e.mem.set_u32(0x011f_426c, 0x3f00_0000);
    e.mem.set_u32(0x011f_426c + 4, 0x4000_0000);
    e.mem.set_u32(0x011f_426c + 8, 0x4040_0000);
    vec![this, case.word(0)]
}

#[test]
fn run_00ba71d0_spawns_at_the_zero_point() {
    use engine_bs::units::bsshader::particleshaderproperty::fn_00ba71d0;
    use engine_bs::units::bsshader::particleshaderproperty::ParticleShaderProperty as Particles;
    fn through(e: &mut Engine, this: Ptr<Particles>, index: u32) {
        fn_00ba71d0(e, this, index)
    }
    let Some(peer) = backend() else { return };
    Diff::new(entry!(0x00ba_71d0, through(Ptr<Particles>, u32)))
        .translation(entry!(0x00ba_71d0, fn_00ba71d0(Ptr<Particles>, u32)).1)
        .reference_peer(peer)
        .inputs(&[Input::Of(&[0, 1, 3])])
        .setup(particles_without_targets)
        .run()
        .assert_same();
}

#[test]
fn run_00ba74a0_spawns_at_the_zero_point() {
    use engine_bs::units::bsshader::particleshaderproperty::fn_00ba74a0;
    use engine_bs::units::bsshader::particleshaderproperty::ParticleShaderProperty as Particles;
    fn through(e: &mut Engine, this: Ptr<Particles>, index: u32) {
        fn_00ba74a0(e, this, index)
    }
    let Some(peer) = backend() else { return };
    Diff::new(entry!(0x00ba_74a0, through(Ptr<Particles>, u32)))
        .translation(entry!(0x00ba_74a0, fn_00ba74a0(Ptr<Particles>, u32)).1)
        .reference_peer(peer)
        .inputs(&[Input::Of(&[0, 1, 3])])
        .setup(particles_without_targets)
        .run()
        .assert_same();
}

/// 00c806b0: the nearest-hit wrapper of a ray test. A null node makes the ray test (`00c7fcc0`,
/// which the module holds) leave the best record alone, so the answer is its first word, zero.
#[test]
fn run_00c806b0_answers_the_best_hit_word() {
    use engine_havok::units::bshavok::bhkutilfunctions::fn_00c806b0;
    fn through(e: &mut Engine, node: Ptr, ray: u32, flag: u32) -> u32 {
        fn_00c806b0(e, node, ray, flag)
    }
    let Some(peer) = backend() else { return };
    Diff::new(entry!(0x00c8_06b0, through(Ptr, u32, u32) -> u32))
        .translation(entry!(0x00c8_06b0, fn_00c806b0(Ptr, u32, u32) -> u32).1)
        .reference_peer(peer)
        .inputs(&[Input::Of(&[0]), Input::Of(&[0]), Input::Of(&[0, 1])])
        .setup(|e, case| {
            let ray = e.mem.alloc(0x40);
            vec![0, ray, case.word(2)]
        })
        .run()
        .assert_same();
}
