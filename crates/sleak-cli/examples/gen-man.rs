//! Generate man pages for the sleak CLI into `man/` at the workspace root.
//!
//! Run with `cargo run -p sleak-cli --example gen-man`. One page is
//! written per command path: `man/sleak.1` for the top-level command and
//! `man/sleak-<subcommand>.1` for each subcommand, recursing into nested
//! subcommands (`sleak baseline create` becomes `man/sleak-baseline-create.1`).

use clap::CommandFactory;
use clap_mangen::Man;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../src/cli.rs"]
mod cli;

fn write_page(cmd: &clap::Command, out_dir: &Path, page: &str) -> std::io::Result<()> {
    let man = Man::new(cmd.clone());
    let mut buf = Vec::new();
    man.render(&mut buf)?;
    fs::create_dir_all(out_dir)?;
    fs::write(out_dir.join(format!("{page}.1")), buf)
}

fn generate(cmd: &clap::Command, out_dir: &Path, page: &str) -> std::io::Result<()> {
    write_page(cmd, out_dir, page)?;
    for sub in cmd.get_subcommands() {
        generate(sub, out_dir, &format!("{page}-{}", sub.get_name()))?;
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join("man");
    generate(&cli::Cli::command(), &out_dir, "sleak")
}
