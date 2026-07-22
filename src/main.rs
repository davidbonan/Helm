use helm::cli;

fn main() -> eframe::Result<()> {
    match cli::parse(std::env::args_os().skip(1)) {
        cli::Args::Gui { open_url } => helm::app::run(open_url),
        other => std::process::exit(cli::execute(other)),
    }
}
