// SPDX-License-Identifier: GPL-3.0-only
//! The five one-word globals the shared engine parts hand out through `cdecl` getters. Each
//! translation loads one word from its exe address and returns it; there is no call tree, no float
//! maths and no write, so each typed function is the same load at the same address.

use engine_core::Engine;

/// The platform swap word (`011d8a80`).
const PLATFORM_SWAP_WORD: u32 = 0x011d_8a80;
/// The default texture setting (`011f4748`).
const DEFAULT_TEXTURE_SETTING: u32 = 0x011f_4748;
/// The memory manager's limit word (`011a95fc`).
const LIMIT_WORD: u32 = 0x011a_95fc;
/// The audio instance (`011f6d98`).
const AUDIO_INSTANCE: u32 = 0x011f_6d98;
/// The task queue object (`011df1a8`).
const TASK_QUEUE_OBJECT: u32 = 0x011d_f1a8;

// Typed from 004b7210
/// The word at `011d8a80`, read as the exe reads it (`cdecl`, no arguments).
pub fn platform_swap_word(e: &mut Engine) -> u32 {
    e.mem.u32(PLATFORM_SWAP_WORD)
}

// Typed from 0043c4b0
/// The default texture setting at `011f4748` (`cdecl`, no arguments).
pub fn default_texture_setting(e: &mut Engine) -> u32 {
    e.global::<u32>(DEFAULT_TEXTURE_SETTING)
}

// Typed from 008781f0
/// The global limit word at `011a95fc` (`cdecl`, no arguments).
pub fn limit_word(e: &mut Engine) -> u32 {
    e.global::<u32>(LIMIT_WORD)
}

// Typed from 00453a70
/// `BSAudio::QInstance`: the audio instance at `011f6d98` (`cdecl`, no arguments).
pub fn bs_audio_q_instance(e: &mut Engine) -> u32 {
    e.global::<u32>(AUDIO_INSTANCE)
}

// Typed from 004537b0
/// The task queue object at `011df1a8` (`cdecl`, no arguments).
pub fn task_queue_object(e: &mut Engine) -> u32 {
    e.global::<u32>(TASK_QUEUE_OBJECT)
}
