use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    file: String,
}

fn main() {
    let args = Args::parse();

    println!("Loading {}!", args.file);
}
