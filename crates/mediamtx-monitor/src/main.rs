// Windows ではコンソールウィンドウを出さない。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mediamtx_monitor::cli::Args;

fn main() {
    let args = Args::parse_from(std::env::args().skip(1).collect());
    if args.help {
        println!("{}", Args::USAGE);
        return;
    }
    if args.version {
        println!("mediamtx-monitor {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Err(err) = mediamtx_monitor::run(args.smoke_exit_after_ms) {
        eprintln!("起動に失敗しました: {err}");
        std::process::exit(1);
    }
}
