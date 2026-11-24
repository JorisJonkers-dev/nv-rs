// SPDX-License-Identifier: GPL-3.0-only
//! The whole tool on a small made-up repository and made-up pinned sources: every count, the
//! same bytes on a second run, the pictures drawn again from the summary alone, and the drop check.

use progress::{baseline_counts, counts, drops, measure, render, render_summary, Inputs, OUTPUTS};
use std::path::{Path, PathBuf};

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

const LEDGER: &str = "address\tsize\tname\tsubsystem\tunit\tstatus\trust\ttyped
00401000\t1\t\t\t\ttranslated\tcrates/engine-a/src/a.rs:3\tyes
00401010\t1\t\t\t\ttranslated\tcrates/engine-a/src/a.rs:9\t
00401020\t1\t\t\t\ttranslated\tcrates/engine-a/src/a.rs:12\t
00401030\t1\t\t\t\tstatic init (table)\tcrates/engine-core/data/static_initializers.tsv:7\t
00401040\t1\t\t\t\tstatic init (table)\tcrates/engine-core/data/atexit_destructors.tsv:9\t
00401050\t1\t\t\t\tplatform\t\t
";

/// A tree with everything the tool reads; `jip_tested` decides whether JIP's one test exists.
fn tree(base: &Path, jip_tested: bool) -> (PathBuf, PathBuf) {
    let root = base.join("repo");
    let src = base.join("sources");
    let _ = std::fs::remove_dir_all(base);
    put(
        &root,
        ".fork/capabilities.json",
        r#"{"extenders": [
  {"id": "xnvse", "provided_by_source": "crates/nvse", "commands": {"implemented": 2, "total": 3, "implemented_names": ["GetNVSEVersion", "con_TFC"]}},
  {"id": "jip-ln", "provided_by_source": "crates/jip", "commands": {"implemented": 2, "total": 2, "implemented_names": ["GetLNVersion", "FileExists"]}},
  {"id": "johnnyguitar", "provided_by_source": "crates/jg", "commands": {"implemented": 1, "total": 2, "implemented_names": ["GetQuestFailed"]}},
  {"id": "showoff", "provided_by_source": "crates/showoff", "commands": {"implemented": 0, "total": 1, "implemented_names": []}},
  {"id": "stewie-tweaks", "provided_by_source": "crates/stewie", "commands": {"implemented": 0, "total": 8, "implemented_names": []}},
  {"id": "ttw-nvse", "provided_by_source": "crates/ttw-nvse", "commands": {"implemented": 1, "total": 1, "implemented_names": ["TTWThing"]}}
]}"#,
    );
    put(
        &root,
        "crates/nvse/src/commands/misc_tests.rs",
        "#[test]\nfn version() { assert_eq!(call(0x1400), 6.0); }\n",
    );
    if jip_tested {
        put(
            &root,
            "crates/jip/src/commands/files.rs",
            "fn file_exists() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { run(\"FileExists\"); }\n}\n",
        );
    }
    put(
        &root,
        "crates/jg/src/commands_tests.rs",
        "#[test]\nfn quest() { call(0x3101); }\n",
    );
    put(
        &root,
        "crates/ttw-nvse/src/lib.rs",
        "#[cfg(test)]\nmod tests;\n",
    );
    put(
        &root,
        "crates/ttw-nvse/src/tests.rs",
        "#[test]\nfn t() { \"TTWThing\"; }\n",
    );
    put(
        &root,
        "crates/engine-typed/src/a.rs",
        "pub static A: Family = Family {\n    name: \"a\",\n    stage2: true,\n    funcs,\n};\npub static B: Family = Family {\n    name: \"b\",\n    stage2: false,\n    funcs,\n};\n",
    );
    put(
        &root,
        "docs/fork/graphics-features.tsv",
        "# plan\nid\tgroup\tname\nao\tpost\tAmbient occlusion\naniso\tmaterials\tTexture filtering\nsky\tworld\tSky\n",
    );
    put(
        &root,
        "crates/fork-gfx/src/atlas.rs",
        "pub static ATLAS: &[(&str, &[&str])] = &[\n    (\"ao.enabled\", &[\"ao\"]),\n    (\"textures.anisotropy\", &[\"aniso\"]),\n];\n",
    );
    put(
        &root,
        "crates/host/src/gfx_tests.rs",
        "#[test]\nfn off() { env(\"NV_GFX\", \"ao.enabled=false\"); }\n",
    );
    put(
        &root,
        "crates/engine-platform/src/d3d9device.rs",
        "fn register_code(e: &mut Engine) {\n    let device: [(u32, crate::AbiFn); 2] = [\n        (0, crate::entry!(0, qi(Ptr) -> u32).1),\n        (2, WRITER.state.0),\n    ];\n    for (slot, f) in device {\n        e.register(DEVICE_CODE + 4 * slot, f);\n    }\n}\n",
    );
    put(
        &root,
        "crates/d3d9-stream/src/cmd.rs",
        "commands! {\n    /// `SetRenderState(state, value)`.\n    SetRenderState { device: Handle, state: u32, value: u32 },\n}\n",
    );
    put(
        &root,
        "crates/d3d9-wgpu/src/lib.rs",
        "#[cfg(test)]\nmod fingerprint;\nfn execute(c: Cmd) { match c { Cmd::SetRenderState { .. } => {} _ => {} } }\n",
    );
    put(
        &root,
        "crates/d3d9-wgpu/src/state.rs",
        "pub mod rs {\n    pub const ZENABLE: usize = 7;\n    pub const FOGENABLE: usize = 28;\n}\nfn z(s: &[u32]) -> bool { s[rs::ZENABLE] != 0 }\nfn f(v: u32) {\n    match v {\n        rs::FOGENABLE => warn(\"fog is not provided\"),\n    }\n}\n",
    );

    put(
        &src,
        "xnvse/nvse/nvse/CommandTable.cpp",
        "void CommandTable::AddCommandsV1()\n{\n\tADD_CMD(GetNVSEVersion);\n\tADD_CMD(GetBaseObject);\n\tImportConsoleCommand(\"TFC\");\n}\n",
    );
    put(
        &src,
        "jip-ln/jip_nvse.cpp",
        "\t/*2200*/REG_CMD(GetLNVersion);\n\t/*2201*/REG_EMPTY;\n\t/*2202*/REG_CMD(FileExists);\n",
    );
    put(
        &src,
        "johnnyguitar/JG/internal/JG/JohnnyCommands.cpp",
        "\t\tREG_CMD(Other);\n\t\tREG_CMD(GetQuestFailed);\n",
    );
    put(
        &src,
        "johnnyguitar/JG/internal/JG/JohnnyPluginData.hpp",
        "\tconstexpr uint32_t JG_OPCODE_BASE = 0x3100;\n",
    );
    put(
        &src,
        "showoff/SHOWOFF-NVSE/ShowOffNVSE.cpp",
        "\t\t/*3C93*/ REG_CMD(ModNumericGameSetting)\n",
    );
    put(
        &src,
        "knvse/nvse_plugin_example/main.cpp",
        "\tREG_CMD(ForcePlayIdle);\n",
    );
    put(
        &src,
        "knvse/nvse_plugin_example/commands_animation.cpp",
        "\tbuilder.Create(\"GetAnim\", 1);\n",
    );
    put(
        &src,
        "d3d9-headers/mingw-w64-headers/include/d3d9.h",
        "DECLARE_INTERFACE_IID_(IDirect3DDevice9,IUnknown,\"x\")\n{\n    STDMETHOD(QueryInterface)(THIS) PURE;\n    STDMETHOD_(ULONG,AddRef)(THIS) PURE;\n    STDMETHOD(SetRenderState)(THIS_ DWORD a, DWORD b) PURE;\n    STDMETHOD(GetRenderState)(THIS) PURE;\n};\n",
    );
    put(
        &src,
        "d3d9-headers/mingw-w64-headers/include/d3d9types.h",
        "typedef enum _D3DRENDERSTATETYPE {\n    D3DRS_ZENABLE = 7,\n    D3DRS_FOGENABLE = 28,\n    D3DRS_DITHERENABLE = 26,\n    D3DRS_FORCE_DWORD = 0x7fffffff\n} D3DRENDERSTATETYPE;\n",
    );
    (root, src)
}

fn base(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn every_count_of_the_synthetic_tree() {
    let (root, src) = tree(&base("counts"), true);
    let p = measure(&Inputs {
        root: &root,
        sources: &src,
        ledger_tsv: LEDGER,
    })
    .unwrap();
    let c: Vec<(String, usize)> = counts(&p);
    let get = |k: &str| c.iter().find(|(n, _)| n == k).unwrap().1;
    assert_eq!(get("engine.by_hand"), 3);
    assert_eq!(get("engine.tables"), 2);
    assert_eq!(get("native.typed"), 1);
    assert_eq!(get("native.families"), 2);
    assert_eq!(get("native.stage2_families"), 1);
    assert_eq!(get("d3d9.methods_carried"), 1, "SetRenderState");
    assert_eq!(
        get("d3d9.methods_answered"),
        2,
        "SetRenderState and QueryInterface (slot 0)"
    );
    assert_eq!(
        get("d3d9.states_mapped"),
        1,
        "ZENABLE; FOGENABLE only warns"
    );
    assert_eq!(get("features.done"), 1, "ao; aniso has no test");
    assert_eq!(get("features.declared"), 2);
    // xNVSE: GetNVSEVersion tested by opcode 0x1400, con_TFC built without a test, GetBaseObject
    // missing.
    assert_eq!(get("extenders.xnvse.tested"), 1);
    assert_eq!(get("extenders.xnvse.built"), 2);
    // JIP: the empty slot is no command; FileExists tested by name in an inline test module.
    let jip = p.extenders.iter().find(|e| e.id == "jip-ln").unwrap();
    assert_eq!((jip.total, jip.tested.len(), jip.untested.len()), (2, 1, 1));
    // JohnnyGuitar: GetQuestFailed is the second slot, 0x3101, which the test calls.
    assert_eq!(get("extenders.johnnyguitar.tested"), 1);
    let jg = p.extenders.iter().find(|e| e.id == "johnnyguitar").unwrap();
    assert_eq!(jg.missing, ["Other"]);
    // kNVSE has no crate: both of its commands are missing.
    let k = p.extenders.iter().find(|e| e.id == "knvse").unwrap();
    assert_eq!(k.missing, ["ForcePlayIdle", "GetAnim"]);
    // Tweaks has no public source: its eight slots from the table's total, none built.
    let s = p
        .extenders
        .iter()
        .find(|e| e.id == "stewie-tweaks")
        .unwrap();
    assert_eq!((s.total, s.missing.len()), (8, 8));
    assert_eq!(get("extenders.ttw-nvse.tested"), 1);
}

#[test]
fn the_same_tree_writes_the_same_bytes_and_the_summary_draws_the_pictures() {
    let (root, src) = tree(&base("bytes"), true);
    let inp = Inputs {
        root: &root,
        sources: &src,
        ledger_tsv: LEDGER,
    };
    let a = render(&measure(&inp).unwrap());
    let b = render(&measure(&inp).unwrap());
    assert_eq!(a, b);
    assert_eq!(
        a.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        OUTPUTS.to_vec()
    );
    for (path, text) in &a[1..] {
        assert!(
            text.starts_with("<svg ") && text.ends_with("</svg>\n"),
            "{path}"
        );
        assert!(!text.contains("NaN"));
    }
    let again = render_summary(&a[0].1).unwrap();
    assert_eq!(again.as_slice(), &a[1..]);
}

#[test]
fn a_lost_test_is_a_drop() {
    let (root, src) = tree(&base("drop-before"), true);
    let before = render(
        &measure(&Inputs {
            root: &root,
            sources: &src,
            ledger_tsv: LEDGER,
        })
        .unwrap(),
    );
    let (root, src) = tree(&base("drop-after"), false);
    let after = measure(&Inputs {
        root: &root,
        sources: &src,
        ledger_tsv: LEDGER,
    })
    .unwrap();
    let d = drops(&baseline_counts(&before[0].1).unwrap(), &counts(&after));
    assert_eq!(d, [("extenders.jip-ln.tested".to_string(), 1, 0)]);
}
