// SPDX-License-Identifier: GPL-3.0-only
//! The animation-path overrides (`SetActorAnimationPath`, `provenance/01-store-and-seam.md` and
//! `provenance/02-set-actor-animation-path.md`): for an actor or every actor, first or third
//! person, and an animation group, a stack of overrides. The entry on top whose condition holds
//! answers; disabling one brings back the one below.
//!
//! The store holds no engine state of its own: an entry names its `KFModel` and its condition
//! script, and what it holds on them (a manual reference on the model, the lambda's saved
//! variables) is taken and given back by the caller, from what [`Store::enable`],
//! [`Store::disable_path`] and [`Store::clear`] hand back.

use std::collections::BTreeMap;

/// Whom an override is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// One actor, by its reference's form id.
    Actor(u32),
    /// Every actor (the command called without a reference).
    Every,
}

/// One stack's place in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub target: Target,
    /// The player's first-person animation, else every third-person one.
    pub first_person: bool,
    /// The animation group id the file replaces (`TESAnimGroup` id: group and type bytes).
    pub group: u16,
}

/// One mod's override on a stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// Who set it: the mod index of the calling script's form.
    pub owner: u32,
    /// The `.kf` file the mod gave, as [`normal_path`] keeps it (lower case, `\` separators).
    pub path: Vec<u8>,
    /// The `KFModel` loaded from `path` (`ModelLoader::LoadKF`), with the manual reference that
    /// load took; 0 for none, which answers nothing.
    pub kf: u32,
    /// The condition function (a lambda or user function script), 0 for none: the override
    /// answers only while it returns true for the actor.
    pub condition: u32,
    /// The condition is asked again while the animation plays (`pollConditionFunction`).
    pub poll: bool,
}

/// Every stack, by key.
#[derive(Debug, Default)]
pub struct Store {
    stacks: BTreeMap<Key, Vec<Override>>,
}

/// `path` as the store keeps it: ASCII lower case, `/` as `\`.
pub fn normal_path(path: &[u8]) -> Vec<u8> {
    path.iter()
        .map(|&b| {
            if b == b'/' {
                b'\\'
            } else {
                b.to_ascii_lowercase()
            }
        })
        .collect()
}

impl Store {
    /// No override anywhere.
    pub fn is_empty(&self) -> bool {
        self.stacks.is_empty()
    }

    /// How many overrides, over every stack.
    pub fn len(&self) -> usize {
        self.stacks.values().map(Vec::len).sum()
    }

    /// Puts `entry` on top of `key`'s stack. The same owner's entry for the same path, if there is
    /// one, leaves its old place: enabling again moves it to the top with its new model and
    /// condition. The entry it replaced, if any, is handed back.
    pub fn enable(&mut self, key: Key, mut entry: Override) -> Option<Override> {
        entry.path = normal_path(&entry.path);
        let stack = self.stacks.entry(key).or_default();
        let old = stack
            .iter()
            .position(|o| o.owner == entry.owner && o.path == entry.path)
            .map(|i| stack.remove(i));
        stack.push(entry);
        old
    }

    /// Takes `owner`'s entry for `path` off `key`'s stack, wherever it is on it; the others stay
    /// in their order. The entry taken, if there was one.
    pub fn disable(&mut self, key: Key, owner: u32, path: &[u8]) -> Option<Override> {
        let path = normal_path(path);
        let stack = self.stacks.get_mut(&key)?;
        let removed = stack
            .iter()
            .position(|o| o.owner == owner && o.path == path)
            .map(|i| stack.remove(i));
        if stack.is_empty() {
            self.stacks.remove(&key);
        }
        removed
    }

    /// Takes `owner`'s entries for `path` off every stack of `target` and person, whatever group
    /// they replace (disabling by path needs no file loaded). The entries taken.
    pub fn disable_path(
        &mut self,
        target: Target,
        first_person: bool,
        owner: u32,
        path: &[u8],
    ) -> Vec<Override> {
        let keys: Vec<Key> = self
            .stacks
            .keys()
            .filter(|k| k.target == target && k.first_person == first_person)
            .copied()
            .collect();
        keys.into_iter()
            .filter_map(|k| self.disable(k, owner, path))
            .collect()
    }

    /// The entry on top of `key`'s stack.
    pub fn top(&self, key: Key) -> Option<&Override> {
        self.stacks.get(&key).and_then(|s| s.last())
    }

    /// The override that answers for the actor with form id `actor`, the person and `group` when
    /// no condition is asked: the top of the actor's own stack, else of every actor's.
    pub fn lookup(&self, actor: u32, first_person: bool, group: u16) -> Option<&Override> {
        self.candidates(actor, first_person, group).next()
    }

    /// Every override that may answer for the actor, the person and `group`, in the order they
    /// are asked: the actor's own stack from the top, then every actor's from the top. The seam
    /// takes the first whose condition holds.
    pub fn candidates(
        &self,
        actor: u32,
        first_person: bool,
        group: u16,
    ) -> impl Iterator<Item = &Override> {
        let stack = |target| {
            self.stacks
                .get(&Key {
                    target,
                    first_person,
                    group,
                })
                .into_iter()
                .flat_map(|s| s.iter().rev())
        };
        stack(Target::Actor(actor)).chain(stack(Target::Every))
    }

    /// Empties every stack; the entries taken.
    pub fn clear(&mut self) -> Vec<Override> {
        std::mem::take(&mut self.stacks)
            .into_values()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
