use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use colored::Colorize;

use git_valet::valet;

#[derive(Parser)]
#[command(
    name = "git-valet",
    version,
    about = "Transparently version private files in a separate private repo, synced via git hooks",
    long_about = "git-valet — transparently version private files (.env, secrets, notes, AI prompts)\nin a separate private repo, synced via git hooks. Zero workflow change."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a valet repo for this project
    Init {
        /// Remote of the valet repo (e.g. git@github.com:user/project-private.git)
        remote: String,
        /// Files/directories to track (optional — omit on fresh clone to read from .gitvalet)
        files: Vec<String>,
    },
    /// Show the valet repo status
    Status,
    /// Synchronize the valet repo (add + commit + push)
    Sync {
        #[arg(short, long, default_value = "chore: sync valet")]
        message: String,
    },
    /// Push the valet repo
    Push,
    /// Pull the valet repo
    Pull,
    /// Add files to the valet repo
    Add { files: Vec<String> },
    /// Remove git-valet from this project (hooks + config)
    Deinit,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

fn main() -> ExitCode {
    // Respect NO_COLOR (https://no-color.org/)
    if std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
    }

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { remote, files } => valet::init(&remote, &files),
        Commands::Status => valet::status(),
        Commands::Sync { message } => valet::sync(&message),
        Commands::Push => valet::push(),
        Commands::Pull => valet::pull(),
        Commands::Add { files } => valet::add_files(&files),
        Commands::Deinit => valet::deinit(),
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "git-valet",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{} {err:#}", "error:".red().bold());
            ExitCode::FAILURE
        }
    }
}
