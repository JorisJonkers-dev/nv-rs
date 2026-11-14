//! `functions_jip/jip_fn_string.h` (JIP LN NVSE): `GetStringHash` without its file mode. The
//! three regular expression commands (`sv_RegexMatch`, `sv_RegexSearch`, `sv_RegexReplace`) run
//! on MSVC's `std::regex` (ECMAScript grammar), which no Rust regular expression crate matches
//! in every case; they are not built.

use super::Handlers;
use crate::containers::{str_hash_ci, str_hash_cs};
use engine_core::prelude::*;
use nvse::commands::Read;
use nvse::{handled, CommandArgs};

#[rustfmt::skip]
pub static COMMANDS: Handlers = &[
    (0x293f, get_string_hash),
];

// Translated from JIP LN NVSE 57.30 (5a30ac4), functions_jip/jip_fn_string.h:
// Cmd_GetStringHash_Execute.
/// The hash of a non-empty text as JIP's maps make it, without case unless the third argument
/// is set, as a signed number, echoed to an open console. The split mode (the second argument
/// set: one hash per `|` part, written to `GetStringHash.txt`) writes a file through the C
/// library and is not built: it leaves the result 0.
fn get_string_hash(e: &mut Engine, w: &[u32]) -> Ret {
    let a = CommandArgs::new(w);
    let read = Read::of(e, &a);
    if read.ok {
        let text = read.text(0);
        let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
        let (split, use_case) = (read.int(1, 0), read.int(2, 0));
        if !text.is_empty() && split == 0 {
            let hash = if use_case == 0 {
                str_hash_ci(text)
            } else {
                str_hash_cs(text)
            };
            a.set_result(e, f64::from(hash as i32));
            crate::console::print_result(e, &a);
        }
    }
    handled(true)
}

#[cfg(test)]
#[path = "jip_fn_string_tests.rs"]
mod tests;
