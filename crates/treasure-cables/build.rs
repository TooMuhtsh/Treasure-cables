//! - Identifiant de compilation (commit Git) affiché par l'outil, pour savoir
//!   sans ambiguïté quelle version a produit un rapport.
//! - Interface web (`web/dist`, produite par `npm run build`) embarquée dans
//!   l'exécutable ; à défaut, une page explique comment la compiler.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "inconnu".to_owned());
    println!("cargo:rustc-env=TREASURE_BUILD={commit}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");

    embed_web();
}

fn embed_web() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../web/dist");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut files = Vec::new();
    if dist.join("index.html").exists() {
        list(&dist, &dist, &mut files);
    } else {
        let placeholder = out.join("placeholder.html");
        std::fs::write(
            &placeholder,
            "<!doctype html><meta charset=utf-8><title>Treasure Cables</title>\
             <p>Interface web non compilée : lancez <code>npm ci &amp;&amp; npm run build</code> \
             dans le dossier <code>web/</code>, puis recompilez.</p>",
        )
        .unwrap();
        files.push(("index.html".to_owned(), placeholder));
    }

    let mut code = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for (name, path) in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let _ = writeln!(
            code,
            "    ({name:?}, include_bytes!({:?})),",
            path.display().to_string()
        );
    }
    code.push_str("];\n");
    std::fs::write(out.join("web_assets.rs"), code).unwrap();
}

fn list(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            list(root, &path, out);
        } else {
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((name, path.canonicalize().unwrap()));
        }
    }
}
