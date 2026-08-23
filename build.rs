//! The web UI is embedded into the binary, so it has to exist before rustc
//! runs. Without this, a fresh clone fails inside `rust-embed` with a message
//! that says nothing about what to do.

fn main() {
    println!("cargo:rerun-if-changed=web/dist");
    if !std::path::Path::new("web/dist/index.html").exists() {
        println!("cargo:warning=web/dist is missing: build the UI first");
        panic!(
            "\n\nweb/dist is missing, so there is no UI to embed.\n\
             Build it once, then build again:\n\n    \
             cd web && pnpm install && pnpm build\n\n"
        );
    }
}
