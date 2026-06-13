use std::fs;
use std::path::Path;

/// Stages the console UI bundle for embedding. The build embeds from
/// OUT_DIR so `cargo publish` works without the bundle: a crate built
/// without `just console-build` gets an empty asset set and `rototo
/// console` serves an instructional page instead.
fn main() {
    println!("cargo:rerun-if-changed=apps/console/dist");

    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let staged = Path::new(&out_dir).join("console-dist");
    if staged.exists() {
        fs::remove_dir_all(&staged).expect("clear staged console assets");
    }
    fs::create_dir_all(&staged).expect("create staged console asset directory");

    let dist = Path::new("apps/console/dist");
    if dist.is_dir() {
        copy_dir(dist, &staged);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).expect("read console dist directory") {
        let entry = entry.expect("read console dist entry");
        let source = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy() == ".gitkeep" {
            continue;
        }
        let target = to.join(&name);
        if source.is_dir() {
            fs::create_dir_all(&target).expect("create staged console subdirectory");
            copy_dir(&source, &target);
        } else {
            fs::copy(&source, &target).expect("copy console asset");
        }
    }
}
