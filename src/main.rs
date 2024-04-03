#[macro_use]
extern crate lazy_static;

use clap::Parser;
use env_logger::Env;
use std::path::PathBuf;

mod card;

#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    /// Use a single file as input
    #[arg(long)]
    file: PathBuf,

    /// Walk a directory and use all files as input
    #[arg(long)]
    folder: PathBuf,
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let cards = card::parse_file(&args.file).await.unwrap();
    println!("{:?}", cards);
}
