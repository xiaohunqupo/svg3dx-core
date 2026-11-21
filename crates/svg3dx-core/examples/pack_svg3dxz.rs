//! Pack sample `.svg3dx` + sidecars into `.svg3dxz` using core package I/O.
//!
//! Usage:
//!   cargo run -p svg3dx-core --example pack_svg3dxz -- \
//!     --main path/to/doc.svg3dx --out path/to/out.svg3dxz [--asset rel=path ...]
//!
//! Or pack the two official glTF samples (paths relative to svg3dx samples dir):
//!   cargo run -p svg3dx-core --example pack_svg3dxz -- --samples-dir ../../svg3dx/samples

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use svg3dx_core::{write_svg3dxz, Svg3dxPackage};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--samples-dir" || a.starts_with("--samples-dir=")) {
        let dir = arg_value(&args, "--samples-dir").unwrap_or_else(|| {
            eprintln!("--samples-dir requires a path");
            std::process::exit(2);
        });
        pack_official_samples(Path::new(&dir));
        return;
    }

    let main_path = arg_value(&args, "--main").unwrap_or_else(|| {
        usage();
        std::process::exit(2);
    });
    let out_path = arg_value(&args, "--out").unwrap_or_else(|| {
        usage();
        std::process::exit(2);
    });

    let main_xml = fs::read_to_string(&main_path).unwrap_or_else(|e| {
        eprintln!("read main: {e}");
        std::process::exit(1);
    });
    let mut pkg = Svg3dxPackage::new(main_xml);
    for a in args.iter().filter(|a| a.starts_with("--asset=")) {
        let rest = &a["--asset=".len()..];
        let (rel, file) = rest.split_once('=').unwrap_or_else(|| {
            eprintln!("--asset expects rel=path, got {rest}");
            std::process::exit(2);
        });
        let bytes = fs::read(file).unwrap_or_else(|e| {
            eprintln!("read asset {file}: {e}");
            std::process::exit(1);
        });
        pkg.set_asset(rel, bytes).unwrap_or_else(|e| {
            eprintln!("set asset {rel}: {e}");
            std::process::exit(1);
        });
    }

    let bytes = write_svg3dxz(&pkg).unwrap_or_else(|e| {
        eprintln!("write package: {e}");
        std::process::exit(1);
    });
    if let Some(parent) = Path::new(&out_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&out_path, bytes).unwrap_or_else(|e| {
        eprintln!("write {out_path}: {e}");
        std::process::exit(1);
    });
    println!("wrote {out_path}");
}

fn pack_official_samples(samples: &Path) {
    pack_one(
        samples,
        "draft-0.1-gltf.svg3dx",
        "draft-0.1-gltf.svg3dxz",
        &["assets/bunny.glb"],
    );
    pack_one(
        samples,
        "draft-0.1-gltf-if.svg3dx",
        "draft-0.1-gltf-if.svg3dxz",
        &["assets/bunny.glb", "assets/bunny-lod0.glb"],
    );
}

fn pack_one(samples: &Path, main_name: &str, out_name: &str, assets: &[&str]) {
    let main_path = samples.join(main_name);
    let out_path = samples.join(out_name);
    let main_xml = fs::read_to_string(&main_path).unwrap_or_else(|e| {
        eprintln!("read {}: {e}", main_path.display());
        std::process::exit(1);
    });
    let mut pkg = Svg3dxPackage::new(main_xml);
    for rel in assets {
        let file: PathBuf = samples.join(rel);
        let bytes = fs::read(&file).unwrap_or_else(|e| {
            eprintln!("read {}: {e}", file.display());
            std::process::exit(1);
        });
        pkg.set_asset(*rel, bytes).unwrap_or_else(|e| {
            eprintln!("set asset {rel}: {e}");
            std::process::exit(1);
        });
    }
    let bytes = write_svg3dxz(&pkg).unwrap_or_else(|e| {
        eprintln!("write package: {e}");
        std::process::exit(1);
    });
    fs::write(&out_path, bytes).unwrap_or_else(|e| {
        eprintln!("write {}: {e}", out_path.display());
        std::process::exit(1);
    });
    println!("wrote {}", out_path.display());
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    for (i, a) in args.iter().enumerate() {
        if a == key {
            return args.get(i + 1).cloned();
        }
        if let Some(v) = a.strip_prefix(&prefix) {
            return Some(v.to_string());
        }
    }
    None
}

fn usage() {
    eprintln!(
        "Usage:\n  pack_svg3dxz --main DOC.svg3dx --out OUT.svg3dxz [--asset rel=path]...\n  pack_svg3dxz --samples-dir PATH"
    );
}
