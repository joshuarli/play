use std::process::exit;

fn main() {
    let mut width = None;
    let mut height = None;
    let mut translated = vec!["--gui".to_owned()];
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--width" => {
                let Some(value) = args.next() else {
                    eprintln!("player-gui: --width needs a value");
                    exit(1);
                };
                width = Some(value);
            }
            "--height" => {
                let Some(value) = args.next() else {
                    eprintln!("player-gui: --height needs a value");
                    exit(1);
                };
                height = Some(value);
            }
            other => translated.push(other.to_owned()),
        }
    }
    if width.is_some() || height.is_some() {
        translated.push(format!(
            "--geometry={}x{}",
            width.unwrap_or_else(|| "1280".to_owned()),
            height.unwrap_or_else(|| "720".to_owned())
        ));
    }
    if let Err(error) = player_app::run(&translated) {
        eprintln!("player-gui: {error}");
        exit(1);
    }
}
