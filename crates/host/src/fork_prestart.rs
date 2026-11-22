// SPDX-License-Identifier: GPL-3.0-only
//! Functions to run on the engine before the game's code starts (the fork's seam): `Host::start_with` runs the game up to
//! main's first frame, and what the game does on the way (building the main menu, for one) can only be replaced by a
//! registration made before that. A caller (the scene run, docs/fork/SCENE.md) adds its functions with [`add`] before the
//! start; the host runs them once, in order, at the start ([`run`]).

use engine::Engine;
use std::sync::Mutex;

type Hook = Box<dyn FnOnce(&mut Engine) + Send>;

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());

/// Adds a function to run before the game starts.
pub fn add(hook: impl FnOnce(&mut Engine) + Send + 'static) {
    HOOKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(Box::new(hook));
}

/// Runs and forgets every added function (the host, before the game's first frame).
pub fn run(engine: &mut Engine) {
    let hooks = std::mem::take(&mut *HOOKS.lock().unwrap_or_else(|p| p.into_inner()));
    for hook in hooks {
        hook(engine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_functions_run_once_in_order() {
        let log = std::sync::Arc::new(Mutex::new(Vec::new()));
        for n in 1..=2 {
            let log = log.clone();
            add(move |_| log.lock().unwrap().push(n));
        }
        let mut e = Engine::new();
        run(&mut e);
        run(&mut e);
        assert_eq!(*log.lock().unwrap(), vec![1, 2]);
    }
}
