// SPDX-License-Identifier: GPL-3.0-only
//! One verdict per plugin DLL, as the engine can say it (the owner's rules, ADR-0018): what the
//! host does with the file and why. The curated part says what each plugin is for and where the
//! engine stands; the observed part is what the host saw when it loaded the plugin (how far it
//! got, how many commands it registered, what it wrote into the game's code).
//!
//! Verdicts: `built into the engine` (the DLL is not loaded), `engine already has the fix`,
//! `not needed here`, `feature missing` (a ticket says what to rebuild natively), and for the
//! plugins the host itself must run: `serves commands`, `loads`, `not working`.

use crate::Engine;

/// What a plugin is, for the engine's purposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// The engine has it natively (all of it or most): the DLL is not loaded.
    BuiltIn,
    /// The engine has the plugin's effect as a row of its own.
    EngineHasFix,
    /// A patch of the original's machine code the native engine does not need.
    NotNeeded,
    /// A patch or a feature the engine does not have; a ticket says what to build.
    FeatureMissing,
    /// A plugin with no native build whose commands or menus the host must run.
    Host,
}

/// (file name lower case, class, mod, ticket on nv-rs, what and why).
pub const CURATED: &[(&str, Class, &str, u32, &str)] = &[
    ("jip_nvse.dll", Class::BuiltIn, "JIP LN NVSE Plugin", 554, "all 1,118 commands and 171 of 183 game patches are native"),
    ("johnnyguitar.dll", Class::BuiltIn, "JohnnyGuitar NVSE", 555, "native; the commands still missing are built natively (#555-#557)"),
    ("showoffnvse.dll", Class::BuiltIn, "ShowOff xNVSE", 558, "native; the commands still missing are built natively (#558-#560)"),
    ("nvse_stewie_tweaks.dll", Class::BuiltIn, "IStewieAI Tweaks", 561, "native engine-fix rows; its 8 commands are not built yet (#561, #562)"),
    ("ttw_nvse.dll", Class::BuiltIn, "TTW NVSE", 406, "native, 3 of 3 commands"),
    ("mcm.dll", Class::BuiltIn, "The Mod Configuration Menu", 0, "native (crates/mcm, row NV_MCM)"),
    ("ui_organizer.dll", Class::BuiltIn, "UIO - User Interface Organizer", 0, "native (row NV_UIO)"),
    ("engineoptimizations.dll", Class::BuiltIn, "IStewieAI Engine Optimizations", 522, "single rows native; the speed-ups are goals (#522)"),
    ("nvtf.dll", Class::BuiltIn, "NVTF", 541, "fixed-step part native; fMaxTime and spider hands decided in #541"),
    ("actorcausesavebloatfix.dll", Class::EngineHasFix, "ActorCause Save Bloat Fix", 510, "row NV_ACTOR_CAUSE"),
    ("celloffsetgenerator.dll", Class::EngineHasFix, "Cell Offset Generator - NVSE", 514, "row NV_CELL_OFFSETS"),
    ("mlf.dll", Class::EngineHasFix, "Mod Limit Fix", 535, "row NV_DIRECT_FILES"),
    ("muzzleflashfix.dll", Class::EngineHasFix, "Muzzle Flash Light Fix", 538, "the muzzle flash light fade row"),
    ("vatslagfix.dll", Class::EngineHasFix, "VATS Lag Fix", 548, "row NV_VATS_LAG"),
    ("crashlogger.dll", Class::NotNeeded, "Yvile Crash Logger", 549, "diagnoses crashes of the original executable; the engine writes its own report (#549)"),
    ("zlibupdate.dll", Class::NotNeeded, "zlib Updated - NVSE", 552, "replaces the zlib inside the original executable; the engine has its own compression, speed is a goal (#552)"),
    ("nvac.dll", Class::FeatureMissing, "NVAC - New Vegas Anti Crash", 539, "the crashes it prevents become engine tests (#539)"),
    ("weapon_lag_fix.dll", Class::FeatureMissing, "Combat Lag Fix", 518, "weapon fire must not stall the frame (#518)"),
    ("audioregionfix.dll", Class::FeatureMissing, "Audio Region Fix - NVSE", 512, "audio regions as the plugin makes them (#512)"),
    ("cloudupgrade.dll", Class::FeatureMissing, "Cloud Upgrade", 517, "clouds as the plugin draws them (#517)"),
    ("depthresolve.dll", Class::FeatureMissing, "Depth Resolve - NVSE", 520, "a readable depth buffer for effects (#520)"),
    ("dof-fix.dll", Class::FeatureMissing, "Depth of Field Fix", 521, "the depth-of-field effect as fixed (#521)"),
    ("externalemittancefix.dll", Class::FeatureMissing, "External Emittance Fix", 523, "lights objects emit outside (#523)"),
    ("fallout shader loader.dll", Class::FeatureMissing, "Shader Loader - NVSE", 524, "replacement shaders from the mods' Shaders folders (#524)"),
    ("alpha fixes.dll", Class::FeatureMissing, "Fallout Alpha Rendering Tweaks", 525, "alpha blending as the plugin sets it (#525)"),
    ("fogculling.dll", Class::FeatureMissing, "Fog-based Object Culling", 526, "objects hidden in dense fog are not drawn (#526)"),
    ("highreslocalmaps.dll", Class::FeatureMissing, "High Res Local Maps", 527, "local map textures at higher resolution (#527)"),
    ("highresbloom.dll", Class::FeatureMissing, "High Resolution Bloom", 528, "bloom at a higher resolution (#528)"),
    ("hdpipboy.dll", Class::FeatureMissing, "High Resolution Screens", 529, "the Pip-Boy screen at a higher resolution (#529)"),
    ("highreswaterdepth.dll", Class::FeatureMissing, "High Resolution Water Fog", 530, "water depth fog at a higher resolution (#530)"),
    ("lod fixes.dll", Class::FeatureMissing, "LOD Fixes and Improvements - NVSE", 532, "the LOD behaviours the plugin changes (#532)"),
    ("lodflickerfix.dll", Class::FeatureMissing, "LOD Flicker Fix - NVSE", 533, "distant land does not flicker (#533)"),
    ("mobilepipboylight.dll", Class::FeatureMissing, "Mobile Pip-Boy Light - NVSE", 534, "the Pip-Boy light as the plugin makes it (#534)"),
    ("moonlightnvse.dll", Class::FeatureMissing, "MoonlightNVSE", 537, "the night lighting the plugin changes (#537)"),
    ("newvegasreloaded.dll", Class::FeatureMissing, "NVR", 540, "the renderer replacement and its effects (#540)"),
    ("dynamicreflections.dll", Class::FeatureMissing, "Real Time Reflections", 542, "reflections drawn live (#542)"),
    ("simplefogremover.dll", Class::FeatureMissing, "Simple Fog Remover", 544, "the plugin that removes fog (#544)"),
    ("vanillaplusao.dll", Class::FeatureMissing, "Vanilla Plus AO", 545, "ambient occlusion (#545)"),
    ("vanillaplusterrain.dll", Class::FeatureMissing, "Vanilla Plus Terrain", 546, "terrain shading (#546)"),
    ("viewmodel shading fix.dll", Class::FeatureMissing, "Viewmodel Shading Fix - NVSE", 547, "the first-person weapon's shading (#547)"),
    ("anhnvse.dll", Class::Host, "AnhNVSE", 511, "script commands the engine does not have"),
    ("bettercaravan.dll", Class::Host, "Better Caravan", 513, "the Caravan game's additions and menu"),
    ("climatecontrol.dll", Class::Host, "Climate Control NVSE", 516, "the weather commands Climate Control and its rain mods call"),
    ("race_menu_presets.dll", Class::Host, "Character Preset Menu", 515, "the preset menu of character creation"),
    ("nvse_console_clipboard.dll", Class::Host, "Console Paste", 519, "paste into the console"),
    ("improved_console.dll", Class::Host, "Improved Console", 531, "a better in-game console"),
    ("knvse.dll", Class::Host, "kNVSE Animation Plugin", 550, "animation commands and exports"),
    ("supnvse.dll", Class::Host, "SUP NVSE Plugin", 543, "script commands and UI elements"),
    ("yui.dll", Class::Host, "yUI", 551, "the interface framework and ySI sorting"),
    ("modernminimap.dll", Class::Host, "Modern Minimap", 536, "the minimap and its HUD"),
];

/// What the host saw of one plugin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub mapped: bool,
    pub queried: bool,
    pub loaded: bool,
    pub commands: usize,
    pub patch_writes: usize,
    pub patch_bytes: usize,
    /// Why it stopped, when it did.
    pub stopped: Option<String>,
    /// Imports the host does not provide.
    pub missing_imports: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub file: String,
    pub mod_name: String,
    pub class: Option<Class>,
    /// The words raul shows.
    pub verdict: &'static str,
    pub reason: String,
    pub ticket: u32,
    pub observed: Observed,
}

fn curated(file: &str) -> Option<&'static (&'static str, Class, &'static str, u32, &'static str)> {
    let f = file.to_ascii_lowercase();
    CURATED.iter().find(|c| c.0 == f)
}

/// The verdict for one file given what was observed (`None`: the host did not look, because the
/// engine builds the plugin in).
pub fn verdict_for(file: &str, seen: Option<Observed>) -> Verdict {
    let row = curated(file);
    let observed = seen.clone().unwrap_or_default();
    let (class, mod_name, ticket, why) = match row {
        Some(r) => (Some(r.1), r.2.to_string(), r.3, r.4.to_string()),
        None => (None, String::new(), 0, String::new()),
    };
    let ran = match &seen {
        None => String::new(),
        Some(o) if o.loaded => format!(
            "; the host loads it, {} command(s), {} write(s) into the game's code dropped",
            o.commands, o.patch_writes
        ),
        Some(o) => format!(
            "; the host stops it: {}",
            o.stopped.as_deref().unwrap_or("unknown reason")
        ),
    };
    let (verdict, reason) = match class {
        Some(Class::BuiltIn) => ("built into the engine", why),
        Some(Class::EngineHasFix) => ("engine already has the fix", why),
        Some(Class::NotNeeded) => ("not needed here", why),
        Some(Class::FeatureMissing) => ("feature missing", format!("{why}{ran}")),
        Some(Class::Host) | None => match &seen {
            Some(o) if o.loaded && o.commands > 0 => ("serves commands", format!("{why}{ran}")),
            Some(o) if o.loaded => ("loads", format!("{why}{ran}")),
            Some(_) => ("not working", format!("{why}{ran}")),
            None => ("not working", format!("{why}; not looked at")),
        },
    };
    Verdict {
        file: file.to_string(),
        mod_name,
        class,
        verdict,
        reason,
        ticket,
        observed,
    }
}

/// What the host saw of each loaded plugin, by file name.
pub fn observed(e: &mut Engine) -> Vec<(String, Observed)> {
    let sites = crate::patch_sites(e);
    crate::plugins_loaded(e)
        .into_iter()
        .filter(|p| !p.runtime)
        .map(|p| {
            let mine: Vec<_> = sites.iter().filter(|s| s.plugin == p.file).collect();
            let o = Observed {
                mapped: true,
                queried: p.queried,
                loaded: p.loaded,
                commands: p.commands.len(),
                patch_writes: mine.len(),
                patch_bytes: mine.iter().map(|s| s.bytes.len()).sum(),
                stopped: p.failed.clone(),
                missing_imports: p.unresolved.len(),
            };
            (p.file, o)
        })
        .collect()
}

/// The verdicts of `files` (the DLLs found): for a plugin the host loaded, with what it saw; for
/// one the engine builds in or has the fix for, without.
pub fn verdicts(e: &mut Engine, files: &[String]) -> Vec<Verdict> {
    let seen = observed(e);
    files
        .iter()
        .map(|f| {
            let o = seen
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(f))
                .map(|s| s.1.clone());
            verdict_for(f, o)
        })
        .collect()
}

fn esc(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// The verdicts as the machine-readable file raul reads (`mod-support.json`).
pub fn to_json(v: &[Verdict]) -> String {
    let mut o = String::from("{\n  \"plugins\": [\n");
    for (i, x) in v.iter().enumerate() {
        o.push_str(&format!(
            "    {{\"file\": \"{}\", \"mod\": \"{}\", \"verdict\": \"{}\", \"reason\": \"{}\", \"ticket\": {}, \"mapped\": {}, \"queried\": {}, \"loaded\": {}, \"commands\": {}, \"patch_writes\": {}, \"missing_imports\": {}}}{}\n",
            esc(&x.file),
            esc(&x.mod_name),
            x.verdict,
            esc(&x.reason),
            x.ticket,
            x.observed.mapped,
            x.observed.queried,
            x.observed.loaded,
            x.observed.commands,
            x.observed.patch_writes,
            x.observed.missing_imports,
            if i + 1 < v.len() { "," } else { "" }
        ));
    }
    o.push_str("  ]\n}\n");
    o
}

#[cfg(test)]
pub fn esc_for_test(s: &str) -> String {
    esc(s)
}
