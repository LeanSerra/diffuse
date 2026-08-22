use diffuse::{cli, git, server};

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let inv = match cli::parse(argv) {
        cli::Parsed::Help => {
            print!("{}", cli::HELP);
            return ExitCode::SUCCESS;
        }
        cli::Parsed::Version => {
            println!("diffuse {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        cli::Parsed::Run(inv) => inv,
    };

    // Dropped flags are reported here, in the terminal, because that is where
    // the person who typed the command is looking.
    if !inv.ignored.is_empty() {
        eprintln!(
            "diffuse: ignoring {} (diffuse always renders the full patch)",
            inv.ignored.join(", ")
        );
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("diffuse: {e}");
            return ExitCode::FAILURE;
        }
    };

    let repo = match git::discover(&cwd) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("diffuse: {e}");
            return ExitCode::FAILURE;
        }
    };

    let runner = git::Runner::new(repo, inv);

    // Behave exactly like git on a bad command: report it here and exit,
    // rather than opening a browser tab to say "bad revision".
    if let Err(e) = runner.validate() {
        eprintln!("{e}");
        return ExitCode::from(1);
    }

    let dev = std::env::var("DIFFUSE_DEV").is_ok();
    let open_browser = runner.inv.open_browser;
    let serving = match server::serve(runner, dev).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("diffuse: could not start server: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The URL is this command's output and goes to stdout, so it can be piped;
    // everything else diffuse has to say is a diagnostic on stderr.
    println!("{}", serving.url);
    if dev {
        eprintln!("diffuse: run `pnpm dev` in web/, then open http://localhost:5173/?t=dev");
    } else if open_browser {
        if let Err(e) = opener::open(&serving.url) {
            eprintln!("diffuse: could not open a browser ({e}); open the URL above");
        }
    }

    let _ = tokio::signal::ctrl_c().await;
    ExitCode::SUCCESS
}
