// SPDX-License-Identifier: GPL-3.0-only
//! The pictures, drawn from the summary alone (`docs/media/fork-progress.json`): whatever counts
//! the summary, the pictures follow it, and another counter that writes the same summary gets
//! the same pictures.

use crate::json::Value;
use crate::svg::{self, pct, seg, thousands, Row};

fn n(v: &Value, path: &[&str]) -> Result<usize, String> {
    let mut cur = v;
    for k in path {
        cur = cur
            .get(k)
            .ok_or_else(|| format!("summary: no {}", path.join(".")))?;
    }
    match cur {
        Value::Arr(a) => Ok(a.len()),
        other => other
            .as_u64()
            .map(|x| x as usize)
            .ok_or_else(|| format!("summary: {} is not a count", path.join("."))),
    }
}

fn engine(s: &Value) -> Result<String, String> {
    let e = |k: &str| n(s, &["engine_port", k]);
    let (hand, start, stop, games) = (
        e("by_hand")?,
        e("startup_table")?,
        e("shutdown_table")?,
        e("game_functions")?,
    );
    let covered = hand + start + stop;
    let row = Row {
        label: "ENGINE PORT · FalloutNV.exe 1.4.0.525".into(),
        right: format!(
            "{} · {} game functions",
            if covered == games {
                "100%".into()
            } else {
                pct(covered, games)
            },
            thousands(games)
        ),
        segs: vec![
            seg(hand, "by hand"),
            seg(start, "start-up table"),
            seg(stop, "shut-down table"),
        ],
        total: games,
        rest: "open".into(),
    };
    Ok(svg::single(
        "Engine port progress",
        &format!(
            "Engine port progress: {} by hand, {} start-up table, {} shut-down table, {} of the game's code covered in all",
            pct(hand, games),
            pct(start, games),
            pct(stop, games),
            pct(covered, games)
        ),
        &row,
    ))
}

fn native(s: &Value) -> Result<String, String> {
    let typed = n(s, &["native_64bit", "typed"])?;
    let hand = n(s, &["native_64bit", "translated_by_hand"])?;
    let fams = n(s, &["native_64bit", "families"])?;
    let stage2 = n(s, &["native_64bit", "stage2_families"])?;
    let rows = [
        Row {
            label: "Typed native code (ADR-0014 stage 1)".into(),
            right: format!(
                "{} of {} hand translations",
                thousands(typed),
                thousands(hand)
            ),
            segs: vec![seg(typed, "functions typed")],
            total: hand,
            rest: "still address-level translations".into(),
        },
        Row {
            label: "Families with a stage-2 store (objects can move to host memory)".into(),
            right: format!("{stage2} of {fams} typed families"),
            segs: vec![seg(stage2, "with a stage-2 store")],
            total: fams,
            rest: "stage 1 only".into(),
        },
    ];
    Ok(svg::panel(
        "NATIVE 64-BIT CODE",
        &format!(
            "Native 64-bit code: {typed} of {hand} hand-translated functions typed; {stage2} of {fams} typed families with a stage-2 store"
        ),
        &rows,
    ))
}

fn d3d9(s: &Value) -> Result<String, String> {
    let d = |k: &str| n(s, &["d3d9_on_wgpu", k]);
    let (methods, carried, answered) = (d("device_methods")?, d("carried")?, d("answered_only")?);
    let (states, mapped, warned) = (d("render_states")?, d("mapped")?, d("warned")?);
    let rows = [
        Row {
            label: "IDirect3DDevice9 methods".into(),
            right: format!("{carried} of {methods} carried to wgpu"),
            segs: vec![
                seg(carried, "carried to wgpu"),
                seg(answered, "answered by the device model"),
            ],
            total: methods,
            rest: "not answered".into(),
        },
        Row {
            label: "Render states (D3DRENDERSTATETYPE)".into(),
            right: format!("{mapped} of {states} mapped"),
            segs: vec![seg(mapped, "mapped"), seg(warned, "warned, not provided")],
            total: states,
            rest: "no code names it".into(),
        },
    ];
    Ok(svg::panel(
        "DIRECT3D 9 ON WGPU (VULKAN / METAL)",
        &format!(
            "Direct3D 9 on wgpu: {carried} of {methods} device methods carried to wgpu, {answered} more answered by the device model; {mapped} of {states} render states mapped, {warned} warned"
        ),
        &rows,
    ))
}

fn extenders(s: &Value) -> Result<String, String> {
    let mut rows = Vec::new();
    let mut aria = Vec::new();
    for e in s.get("extenders").map(Value::as_arr).unwrap_or(&[]) {
        let name = e.get("name").and_then(Value::as_str).unwrap_or("?");
        let (total, tested, untested) =
            (n(e, &["total"])?, n(e, &["tested"])?, n(e, &["untested"])?);
        let built = tested + untested;
        rows.push(Row {
            label: name.to_string(),
            right: format!("{} of {} built", thousands(built), thousands(total)),
            segs: vec![seg(tested, "tested"), seg(untested, "built, no test")],
            total,
            rest: "not built".into(),
        });
        aria.push(format!(
            "{name} {built} of {total} commands built, {tested} tested"
        ));
    }
    if rows.is_empty() {
        return Err("summary: no extenders".into());
    }
    Ok(svg::panel(
        "BUILT-IN SCRIPT EXTENDERS · commands",
        &format!("Script extender commands: {}", aria.join("; ")),
        &rows,
    ))
}

fn features(s: &Value) -> Result<String, String> {
    let f = |k: &str| n(s, &["engine_features", k]);
    let (total, done, declared) = (f("capabilities")?, f("done")?, f("declared_untested")?);
    let row = Row {
        label: "Graphics plan capabilities (docs/fork/graphics-features.tsv)".into(),
        right: format!("{done} of {total} done"),
        segs: vec![
            seg(done, "done (setting and test)"),
            seg(declared, "setting, no test"),
        ],
        total,
        rest: "open".into(),
    };
    Ok(svg::panel(
        "ENGINE FEATURES",
        &format!(
            "Engine features: {done} of {total} graphics plan capabilities done, {declared} more declared by a setting without a test"
        ),
        &[row],
    ))
}

/// The five pictures, in the order of `crate::OUTPUTS` after the summary.
pub fn all(summary: &Value) -> Result<Vec<String>, String> {
    Ok(vec![
        engine(summary)?,
        native(summary)?,
        d3d9(summary)?,
        extenders(summary)?,
        features(summary)?,
    ])
}
