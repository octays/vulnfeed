use std::{collections::BTreeSet, env, fs, io, path::Path};

use build_data::{format_timestamp, get_source_time};
use sha2::{Digest, Sha256};
use shadow_rs::{CARGO_METADATA, CARGO_TREE, ShadowBuilder};

fn configure_rerun_if_head_commit_changed() {
    let mut current = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();

    // skip if no valid-looking git repository could be found
    while let Ok((dir, _)) = gix_discover::upwards(current.as_path()) {
        match dir {
            gix_discover::repository::Path::Repository(git_dir) => {
                unreachable!(
                    "build.rs should never be placed in a git bare repository: {}",
                    git_dir.display()
                )
            }
            gix_discover::repository::Path::WorkTree(work_dir) => {
                let git_refs_heads = work_dir.join(".git/refs/heads");
                println!("cargo::rerun-if-changed={}", git_refs_heads.display());
                break;
            }
            gix_discover::repository::Path::LinkedWorkTree { work_dir, .. } => {
                current = work_dir
                    .parent()
                    .expect("submodule's work_dir must have parent")
                    .to_path_buf();
                continue;
            }
        };
    }
}

const FRONTEND_DIR: &str = "assets";
const FRONTEND_OUTPUT: &str = "assets/public";
const EMBED_DIR: &str = "public";

/// Embed the frontend SPA into the binary: rust-embed serves files from
/// `public/`, so `cargo build` must stage the frontend build output there.
///
/// If `assets/public` has not been built yet and `bun` is available, the SPA is
/// built on the fly; otherwise `public/` stays empty (only `.gitkeep`) and the
/// binary serves no frontend. Set `VULNFEED_SKIP_FRONTEND_BUILD=1` to never
/// invoke bun from cargo.
fn embed_frontend(workspace: &Path) {
    build_frontend_if_missing(workspace);

    let embed_dir = workspace.join(EMBED_DIR);
    let output_dir = workspace.join(FRONTEND_OUTPUT);

    fs::create_dir_all(&embed_dir).expect("failed to create embed dir");
    clear_generated_files(&embed_dir).expect("failed to clear embed dir");
    if output_dir.join("index.html").exists() {
        copy_dir_recursive(&output_dir, &embed_dir).expect("failed to stage frontend assets");
    } else {
        println!(
            "cargo:warning=no frontend build output in {FRONTEND_OUTPUT}; the binary will not embed a frontend"
        );
    }

    // rust-embed reads the folder at compile time (debug-embed), but restaging
    // alone does not recompile the crate. Emitting a content hash as an env
    // var makes any asset change invalidate the build and re-embed.
    let mut hasher = Sha256::new();
    hash_dir(&embed_dir, &mut hasher).expect("failed to hash embed dir");
    println!(
        "cargo::rustc-env=VULNFEED_EMBEDDED_ASSETS_HASH={:x}",
        hasher.finalize()
    );

    // Watch the frontend sources and build output (never `public/` itself:
    // this script writes there, which would cause a rebuild loop).
    for path in [
        format!("{FRONTEND_DIR}/src"),
        FRONTEND_OUTPUT.to_string(),
        format!("{FRONTEND_DIR}/package.json"),
        format!("{FRONTEND_DIR}/bun.lock"),
        format!("{FRONTEND_DIR}/index.html"),
        format!("{FRONTEND_DIR}/vite.config.js"),
        format!("{FRONTEND_DIR}/tailwind.config.js"),
        format!("{FRONTEND_DIR}/postcss.config.js"),
    ] {
        println!("cargo::rerun-if-changed={}", workspace.join(path).display());
    }
    println!("cargo::rerun-if-env-changed=VULNFEED_SKIP_FRONTEND_BUILD");
}

fn build_frontend_if_missing(workspace: &Path) {
    if workspace.join(FRONTEND_OUTPUT).join("index.html").exists() {
        return;
    }
    if env::var_os("VULNFEED_SKIP_FRONTEND_BUILD").is_some() {
        println!(
            "cargo:warning={FRONTEND_OUTPUT} missing, skipping frontend build (VULNFEED_SKIP_FRONTEND_BUILD is set)"
        );
        return;
    }
    for args in [["install", "--frozen-lockfile"], ["run", "build"]] {
        let status = std::process::Command::new("bun")
            .args(args)
            .current_dir(workspace.join(FRONTEND_DIR))
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                println!(
                    "cargo:warning=`bun {}` failed with {status}; no frontend will be embedded",
                    args.join(" ")
                );
                return;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                println!(
                    "cargo:warning=`bun` not found and {FRONTEND_OUTPUT} is missing; run `bun install && bun run build` in {FRONTEND_DIR}/ to embed a frontend"
                );
                return;
            }
            Err(e) => {
                println!(
                    "cargo:warning=failed to run `bun {}`: {e}; no frontend will be embedded",
                    args.join(" ")
                );
                return;
            }
        }
    }
}

fn clear_generated_files(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".gitkeep" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            fs::copy(path, target)?;
        }
    }
    Ok(())
}

fn hash_dir(dir: &Path, hasher: &mut Sha256) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            hash_dir(&path, hasher)?;
        } else {
            hasher.update(path.file_name().unwrap().as_encoded_bytes());
            hasher.update(fs::read(&path)?);
        }
    }
    Ok(())
}

fn main() -> shadow_rs::SdResult<()> {
    configure_rerun_if_head_commit_changed();
    println!(
        "cargo::rustc-env=SOURCE_TIMESTAMP={}",
        if let Ok(t) = get_source_time() {
            format_timestamp(t)?
        } else {
            "".to_string()
        }
    );
    build_data::set_BUILD_TIMESTAMP();

    // The "CARGO_WORKSPACE_DIR" is set manually (not by Rust itself) in Cargo config file, to
    // solve the problem where the "CARGO_MANIFEST_DIR" is not what we want when this repo is
    // made as a submodule in another repo.
    let src_path = env::var("CARGO_WORKSPACE_DIR").or_else(|_| env::var("CARGO_MANIFEST_DIR"))?;
    let out_path = env::var("OUT_DIR")?;

    embed_frontend(Path::new(&src_path));

    let _ = ShadowBuilder::builder()
        .src_path(src_path)
        .out_path(out_path)
        // exclude these two large constants that we don't need
        .deny_const(BTreeSet::from([CARGO_METADATA, CARGO_TREE]))
        .build()?;
    Ok(())
}
