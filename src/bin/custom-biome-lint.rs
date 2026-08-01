use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    custom_biome_lint::run(std::env::args().skip(1), &cwd)
}
