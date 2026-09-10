#[path = "../confirmation.rs"]
mod confirmation;
#[path = "../confirmation_window.rs"]
mod confirmation_window;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dark = match args.as_slice() {
        [show, theme] if show == "--show" && theme == "light" => false,
        [show, theme] if show == "--show" && theme == "dark" => true,
        _ => {
            eprintln!("Use --show light or --show dark to open the simulation window.");
            std::process::exit(2);
        }
    };
    match confirmation_window::show(dark) {
        Ok(outcome) => println!("CONFIRMATION_SPECIMEN_{outcome:?}"),
        Err(code) => {
            eprintln!("{code}");
            std::process::exit(1);
        }
    }
}
