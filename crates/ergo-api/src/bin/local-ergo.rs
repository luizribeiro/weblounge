//! Runs Ergo for `just concierge dev`: `local-ergo <dir> <api address> <token file>`.

use std::{
    convert::Infallible, env, error::Error, fs, os::unix::process::CommandExt, process::ExitCode,
};

use ergo_api::local::LocalErgo;

fn main() -> ExitCode {
    let Err(error) = run();
    eprintln!("local-ergo: {error}");
    ExitCode::FAILURE
}

fn run() -> Result<Infallible, Box<dyn Error>> {
    let [dir, api, token_file]: [String; 3] = env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "usage: local-ergo <dir> <api address> <token file>")?;
    let token = fs::read_to_string(&token_file)?;
    fs::create_dir_all(&dir)?;
    let ergo = LocalErgo::from_env(dir, api.parse()?, token.trim())?;
    ergo.init()?;
    Err(ergo.command("run").exec().into())
}
