fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = player_app::run_program(&args) {
        eprintln!("player: {error}");
        std::process::exit(1);
    }
}
