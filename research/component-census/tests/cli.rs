//! The command's output is the test seam. Every input here is built from scratch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("component-census-cli-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("crates")).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn census(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_component-census"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn the_command_writes_a_generated_table_and_checks_it() {
    let dir = TempDir::new();
    dir.write(
        "components.tsv",
        "component\tlabel\tthread\tregion\tcrates\ttypes\tsource\n\
         scene\tGamebryo scene graph\tmain\tguest/scene\tengine-gamebryo\tNi\tdocs/adr/0010\n\
         havok\tHavok\thavok thread\tguest/havok\tengine-havok\tHk\tdocs/adr/0014\n\
         interface\tInterface\tmain\tguest/interface\tengine-fallout-interface\tMenu\tdocs/fork/CI.md\n",
    );
    dir.write(
        "crates/engine-fallout-interface/src/typed.rs",
        "fn read_node() {\n\
             let _ = e.get(node, NiNode::m_usSize);\n\
         }\n\
         fn write_node() {\n\
             e.set(node, NiNode::m_usSize);\n\
         }\n",
    );
    let root = dir.path().to_str().unwrap();
    let declarations = dir
        .path()
        .join("components.tsv")
        .to_str()
        .unwrap()
        .to_string();
    let out = dir.path().join("census.md").to_str().unwrap().to_string();

    let generated = census(&["--root", root, "--components", &declarations, "--out", &out]);
    assert!(generated.status.success(), "{generated:?}");
    assert!(
        String::from_utf8_lossy(&generated.stdout).contains("3 components, 1 access rows"),
        "{generated:?}"
    );
    let table = fs::read_to_string(&out).unwrap();
    assert!(
        table.contains("| Havok | havok thread | guest/havok |"),
        "{table}"
    );
    assert!(
        table.contains("| interface | scene | NiNode::m_usSize | 1 | 1 |"),
        "{table}"
    );
    assert!(table.contains("`read_node`"), "{table}");
    assert!(table.contains("`write_node`"), "{table}");

    let current = census(&[
        "--root",
        root,
        "--components",
        &declarations,
        "--out",
        &out,
        "--check",
    ]);
    assert!(current.status.success(), "{current:?}");

    fs::write(&out, "# stale\n").unwrap();
    let stale = census(&[
        "--root",
        root,
        "--components",
        &declarations,
        "--out",
        &out,
        "--check",
    ]);
    assert!(!stale.status.success(), "{stale:?}");
    assert!(
        String::from_utf8_lossy(&stale.stderr).contains("is stale"),
        "{stale:?}"
    );
}
