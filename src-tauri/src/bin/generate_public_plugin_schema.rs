use std::{env, fs, path::PathBuf, process::ExitCode};

fn output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root missing")
        .join("docs/plugin-sdk/uipilot-plugin-v1.schema.json")
}

fn generated_schema() -> String {
    let mut output = serde_json::to_string_pretty(&uipilot_lib::public_plugin_manifest_schema())
        .expect("public plugin schema must serialize");
    output.push('\n');
    output
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let check = match arguments.as_slice() {
        [] => false,
        [argument] if argument == "--check" => true,
        _ => return Err("usage: generate_public_plugin_schema [--check]".into()),
    };
    let path = output_path();
    let generated = generated_schema();
    if check {
        let current = fs::read_to_string(&path)
            .map_err(|_| format!("schema is missing: {}", path.display()))?;
        if current != generated {
            return Err(format!("schema is stale: {}", path.display()));
        }
        return Ok(());
    }
    fs::create_dir_all(path.parent().expect("schema parent missing"))
        .map_err(|error| error.to_string())?;
    fs::write(&path, generated).map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
