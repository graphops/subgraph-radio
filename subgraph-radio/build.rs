use std::env;
use std::path::PathBuf;

fn main() {
    let workspace_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let index_html_path = workspace_root
        .join("../frontend/dist/index.html")
        .canonicalize()
        .unwrap();
    println!(
        "cargo:rustc-env=INDEX_HTML_PATH={}",
        index_html_path.display()
    );

    let js_path = workspace_root
        .join("../frontend/dist/frontend-d6e92d55b4e09ed6.js")
        .canonicalize()
        .unwrap();
    println!("cargo:rustc-env=JS_PATH={}", js_path.display());

    let wasm_path = workspace_root
        .join("../frontend/dist/frontend-d6e92d55b4e09ed6_bg.wasm")
        .canonicalize()
        .unwrap();
    println!("cargo:rustc-env=WASM_PATH={}", wasm_path.display());
}
