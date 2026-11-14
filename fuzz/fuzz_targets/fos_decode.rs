#![no_main]

use libfuzzer_sys::fuzz_target;

// A save that splits into parts is then read part by part: every change form through the decoder of its type,
// every global data entry through the decoder of its kind. Errors are valid outcomes.
fuzz_target!(|data: &[u8]| {
    let Ok(save) = fos::Save::parse(data) else {
        return;
    };
    for cf in save.change_forms.iter().take(256) {
        let _ = fos::decode::quest(cf);
        let _ = fos::decode::cell(cf, None);
        let _ = fos::decode::cell_data(cf, Some(true));
        let _ = fos::decode::actor_base(cf);
        let _ = fos::decode::reference_start(cf);
    }
    for g in save.global_data_1.iter().chain(&save.global_data_2).take(256) {
        let _ = fos::decode::globals(g);
        let _ = fos::decode::misc_stats(g);
        let _ = fos::decode::sky(g);
        let _ = fos::decode::radio(g);
        let _ = fos::decode::location(g);
    }
});
