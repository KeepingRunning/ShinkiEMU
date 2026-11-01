use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Windowless ShinkiEMU C ABI and Manager UI runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(clap::Subcommand)]
enum Command {
    Run(shinki_emu_headless::Arguments),
    Compare(shinki_emu_headless::capture::CompareArguments),
    #[cfg(feature = "manager-ui")]
    Ui(shinki_emu_headless::ui::Arguments),
}
fn main() -> std::process::ExitCode {
    match Cli::parse().command {
        Command::Compare(arguments) => shinki_emu_headless::capture::compare_cli(arguments),
        Command::Run(arguments) => {
            let queries: &[(&str, shinki_emu_abi::Query)] = &[
                #[cfg(feature = "static-cmvs")]
                ("shinki.emu.cmvs", shinki_emu_cmvs::cmvs_query),
                #[cfg(feature = "static-siglus")]
                ("siglus_rs", shinki_emu_siglus_rs::query),
            ];
            shinki_emu_headless::run_cli(arguments, queries)
        }
        #[cfg(feature = "manager-ui")]
        Command::Ui(arguments) => shinki_emu_headless::ui::run_cli(arguments),
    }
}
