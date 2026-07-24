mod apply;
mod backends;
mod cli;
mod config;
mod planning;
mod process;
mod recovery;
mod state;
mod system;
mod workspaces;

fn main() {
    std::process::exit(cli::run(std::env::args().collect()));
}
