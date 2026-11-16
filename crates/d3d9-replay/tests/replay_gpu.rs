//! The synthetic scene replayed into the wgpu backend on a device (lavapipe in CI, the card
//! locally). A machine without an adapter skips with a message, as the other GPU tests do.

use d3d9_replay::replay::{run, Options, Replay, WgpuTarget};
use d3d9_replay::synthetic::{self, Scene};

/// One device at a time in this process.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_synthetic_scene_replays_into_wgpu() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let scene = Scene::small();
    let bytes = synthetic::stream(&scene);
    let replay = Replay::parse(&bytes, synthetic::setup_cmds(&scene)).unwrap();
    let mut target = WgpuTarget::new(Some(6));
    let result = match run(
        &mut target,
        &replay,
        Options {
            warmup: 1,
            passes: 2,
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GPU adapter: {e}: the replay test is skipped");
            return;
        }
    };
    assert_eq!(result.frames.len() as u32, 2 * scene.frames);
    assert!(result.frames.iter().all(|f| f.draws == scene.draws));
    assert!(result.frames.iter().all(|f| f.cmd_ns > 0));
    // The GPU time is there when the device has timestamp queries.
    if let Some(g) = result.gpu_ns {
        assert_eq!(g.len(), result.frames.len());
    }
}

#[test]
fn a_larger_target_replays_too() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let scene = Scene::small();
    let bytes = synthetic::stream(&scene);
    let all: Vec<_> = d3d9_stream::wire::commands(&bytes)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let big = d3d9_replay::rescale::rescale(
        &all,
        &d3d9_replay::rescale::Scale::new((64, 64), (192, 128)),
    );
    let replay = Replay::parse(&big, synthetic::setup_cmds(&scene)).unwrap();
    let mut target = WgpuTarget::new(None);
    match run(
        &mut target,
        &replay,
        Options {
            warmup: 0,
            passes: 1,
        },
    ) {
        Ok(r) => assert_eq!(r.frames.len() as u32, scene.frames),
        Err(e) => eprintln!("no GPU adapter: {e}: the replay test is skipped"),
    }
}

#[test]
fn the_synthetic_scene_replays_into_the_vulkan_prototype() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let scene = Scene::small();
    let bytes = synthetic::stream(&scene);
    let replay = Replay::parse(&bytes, synthetic::setup_cmds(&scene)).unwrap();
    let mut target = match d3d9_replay::vulkan::VulkanTarget::new(Some(6)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("no GPU adapter: {e}: the Vulkan prototype test is skipped");
            return;
        }
    };
    let result = run(
        &mut target,
        &replay,
        Options {
            warmup: 1,
            passes: 2,
        },
    )
    .unwrap();
    assert_eq!(result.frames.len() as u32, 2 * scene.frames);
    // Every draw was carried out: none skipped for want of a shader, declaration or target.
    assert_eq!(
        target.counts.skipped,
        0,
        "{}",
        d3d9_replay::replay::Target::notes(&target)
    );
    assert_eq!(
        target.counts.draws,
        u64::from(3 * scene.frames * scene.draws)
    );
}
