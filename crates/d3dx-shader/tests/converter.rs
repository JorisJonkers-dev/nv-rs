//! libvkd3d-shader's bytecode through the d3d9-wgpu reader and converter (nv-linux#200): synthetic
//! HLSL only, no game data. Skips loudly when the library is not installed (`NV_REQUIRE_VKD3D=1`
//! makes that a failure, as in CI).

use d3d9_wgpu::sm::{self, wgsl, Stage};
use d3dx_shader::vkd3d::Vkd3d;
use d3dx_shader::{Kind, NoIncludes, Request, ShaderCompiler};

const PS: &str = "float4 c : register(c0);\nsampler2D s : register(s0);\n\
    float4 main(float2 uv : TEXCOORD0) : COLOR { return tex2D(s, uv) * c; }\n";
const VS: &str = "float4x4 m : register(c0);\n\
    float4 main(float4 p : POSITION) : POSITION { return mul(p, m); }\n";

fn library(test: &str) -> Option<Vkd3d> {
    match Vkd3d::load() {
        Ok(v) => {
            println!("executed: {test} ({})", v.version());
            Some(v)
        }
        Err(e) => {
            assert!(
                !matches!(std::env::var("NV_REQUIRE_VKD3D").as_deref(), Ok("1")),
                "{test}: {e}"
            );
            println!("skipped: {test} (libvkd3d-shader not available)");
            None
        }
    }
}

fn through_the_converter(v: &Vkd3d, source: &str, target: &str, stage: Stage, link: &wgsl::Link) {
    let r = Request {
        kind: Kind::Compile,
        source: source.as_bytes(),
        entry: Some(b"main"),
        target: Some(target.as_bytes()),
        flags: 0,
        macros: &[],
    };
    let out = v
        .compile(&r, &mut NoIncludes)
        .unwrap_or_else(|e| panic!("{target}: {e}"));
    let program = sm::parse(&sm::tokens(&out.bytecode).unwrap())
        .unwrap_or_else(|e| panic!("{target}: reader: {e:?}"));
    assert_eq!(program.stage, stage);
    assert_eq!(program.profile(), target);
    let wgsl =
        wgsl::convert(&program, link).unwrap_or_else(|e| panic!("{target}: converter: {e:?}"));
    sm::validate(&wgsl).unwrap_or_else(|e| panic!("{target}: invalid WGSL: {e}\n{wgsl}"));
}

#[test]
fn vertex_shaders_pass_the_reader_and_converter() {
    let Some(v) = library("vertex_shaders_pass_the_reader_and_converter") else {
        return;
    };
    let link = wgsl::Link {
        inputs: vec![wgsl::Input::Float(4)],
        ..wgsl::Link::default()
    };
    for target in ["vs_2_0", "vs_3_0"] {
        through_the_converter(&v, VS, target, Stage::Vertex, &link);
    }
}

#[test]
fn pixel_shaders_pass_the_reader_and_converter() {
    let Some(v) = library("pixel_shaders_pass_the_reader_and_converter") else {
        return;
    };
    let link = wgsl::Link {
        varyings: vec![wgsl::Semantic {
            usage: sm::usage::TEXCOORD,
            index: 0,
        }],
        centroid: vec![true],
        ..wgsl::Link::default()
    };
    for target in ["ps_2_0", "ps_3_0"] {
        through_the_converter(&v, PS, target, Stage::Pixel, &link);
    }
}
