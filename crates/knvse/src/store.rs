// SPDX-License-Identifier: GPL-3.0-only
//! The animation-path overrides (`SetActorAnimationPath`, `provenance/01-store-and-seam.md`): for
//! an actor or every actor, first or third person, and an animation group, a stack of overrides.
//! The entry on top answers; disabling one brings back the one below.

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
    /// The `.kf` file or folder the mod gave, as [`normal_path`] keeps it (lower case, `\`
    /// separators).
    pub path: Vec<u8>,
    /// The sequence (`BSAnimGroupSequence`) started in place of the group's; 0 while none is
    /// loaded, which answers nothing.
    pub sequence: u32,
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
    /// one, leaves its old place: enabling again moves it to the top with its new sequence.
    pub fn enable(&mut self, key: Key, mut entry: Override) {
        entry.path = normal_path(&entry.path);
        let stack = self.stacks.entry(key).or_default();
        stack.retain(|o| !(o.owner == entry.owner && o.path == entry.path));
        stack.push(entry);
    }

    /// Takes `owner`'s entry for `path` off `key`'s stack, wherever it is on it; the others stay
    /// in their order. Whether there was one.
    pub fn disable(&mut self, key: Key, owner: u32, path: &[u8]) -> bool {
        let path = normal_path(path);
        let Some(stack) = self.stacks.get_mut(&key) else {
            return false;
        };
        let before = stack.len();
        stack.retain(|o| !(o.owner == owner && o.path == path));
        let removed = stack.len() != before;
        if stack.is_empty() {
            self.stacks.remove(&key);
        }
        removed
    }

    /// The entry on top of `key`'s stack.
    pub fn top(&self, key: Key) -> Option<&Override> {
        self.stacks.get(&key).and_then(|s| s.last())
    }

    /// The override that answers for the actor with form id `actor`, the person and `group`: the
    /// actor's own stack first, then every actor's.
    pub fn lookup(&self, actor: u32, first_person: bool, group: u16) -> Option<&Override> {
        let key = |target| Key {
            target,
            first_person,
            group,
        };
        self.top(key(Target::Actor(actor)))
            .or_else(|| self.top(key(Target::Every)))
    }

    /// Empties every stack.
    pub fn clear(&mut self) {
        self.stacks.clear();
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
