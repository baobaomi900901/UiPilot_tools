use std::process::ExitCode;

use systemindex_spike::parse_args;

fn main() -> ExitCode {
    match parse_args(std::env::args_os()) {
        Ok(command) => {
            println!("{}", serde_json::to_string(&command).unwrap());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", serde_json::to_string(&error.evidence()).unwrap());
            ExitCode::from(error.exit_code() as u8)
        }
    }
}
