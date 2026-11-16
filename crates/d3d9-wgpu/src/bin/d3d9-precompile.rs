//! `d3d9-precompile [--shader-cache DIR] [--pipelines DIR] ROOT...`: converts and compiles every
//! shader of a load order into the caches (docs/fork/SHADER-CACHE.md). Each ROOT is a Data folder
//! or a shader package file, lowest priority first: a package of the same name in a later root
//! replaces the earlier one, as a mod's loose file does in the game.
fn main() {
    let mut roots = Vec::new();
    let mut shader = None;
    let mut pipelines = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--shader-cache" => shader = args.next().map(std::path::PathBuf::from),
            "--pipelines" => pipelines = args.next().map(std::path::PathBuf::from),
            "-h" | "--help" => {
                eprintln!("d3d9-precompile [--shader-cache DIR] [--pipelines DIR] ROOT...");
                return;
            }
            other => roots.push(std::path::PathBuf::from(other)),
        }
    }
    if roots.is_empty() {
        eprintln!("d3d9-precompile: no Data folder or shader package named");
        std::process::exit(2);
    }
    if let Some(d) = &shader {
        std::env::set_var("NV_SHADER_CACHE", d);
    }
    let (settings, default_pipelines) = d3d9_wgpu::shader_cache::plan(pipelines, true);
    let options = d3d9_wgpu::precompile::Options {
        roots,
        shader_cache: settings,
        pipelines: default_pipelines,
    };
    match d3d9_wgpu::precompile::run(&options) {
        Ok(r) => eprintln!(
            "precompiled: {} packages ({} replaced by a later root), {} shaders, {} distinct, {} unreadable, {} waited for, {:.2} s",
            r.packages, r.overridden, r.shaders, r.unique, r.unreadable, r.waited_for, r.elapsed.as_secs_f64()
        ),
        Err(e) => {
            eprintln!("d3d9-precompile: {e}");
            std::process::exit(1);
        }
    }
}
