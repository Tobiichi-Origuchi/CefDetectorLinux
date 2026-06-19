use crate::models::CliAppInfo;
use crate::search::core_search;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq)]
enum OutputFormat {
    Toml,
    Json,
    Csv,
}

pub fn handle_cli() {
    let args: Vec<String> = std::env::args().collect();
    let mut show_help = false;
    let mut show_version = false;
    let mut output_format: Option<OutputFormat> = None;
    let mut output_path = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => show_help = true,
            "--version" | "-V" => show_version = true,
            "--toml" | "-T" => output_format = Some(OutputFormat::Toml),
            "--json" | "-J" => output_format = Some(OutputFormat::Json),
            "--csv" | "-C" => output_format = Some(OutputFormat::Csv),
            "--output" | "-O" => {
                if i + 1 < args.len() {
                    output_path = Some(args[i + 1].clone());
                    i += 1;
                } else {
                    eprintln!("Error: --output requires a file path");
                    std::process::exit(1);
                }
            }
            _ => {
                // Ignore other args that might be passed by OS/Tauri
            }
        }
        i += 1;
    }

    if show_version {
        println!("cefdetector {}", VERSION);
        std::process::exit(0);
    }

    if show_help {
        println!("Linux CEF Detector {}", VERSION);
        println!();
        println!("Usage: cefdetector [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -h, --help       Print help information");
        println!("  -V, --version    Print version information");
        println!("  -T, --toml       Output results in TOML format");
        println!("  -J, --json       Output results in JSON format");
        println!("  -C, --csv        Output results in CSV format");
        println!("  -O, --output     Output results to the specified file path instead of stdout");
        std::process::exit(0);
    }

    if let Some(fmt) = output_format {
        let mut results = Vec::new();
        core_search(|info| {
            results.push(info);
        });

        let output_str = match fmt {
            OutputFormat::Json => {
                let cli_results: Vec<CliAppInfo> = results
                    .iter()
                    .map(|r| CliAppInfo {
                        file: &r.file,
                        app_type: &r.app_type,
                        size: r.size,
                        is_running: r.is_running,
                        is_dir: r.is_dir,
                    })
                    .collect();
                serde_json::to_string_pretty(&cli_results).unwrap_or_default()
            }
            OutputFormat::Toml => {
                let mut s = String::new();
                for r in &results {
                    s.push_str("[[app]]\n");
                    s.push_str(&format!(
                        "file = \"{}\"\n",
                        r.file.replace("\\", "\\\\").replace("\"", "\\\"")
                    ));
                    s.push_str(&format!("app_type = \"{}\"\n", r.app_type));
                    s.push_str(&format!("size = {}\n", r.size));
                    s.push_str(&format!("is_running = {}\n", r.is_running));
                    s.push_str(&format!("is_dir = {}\n\n", r.is_dir));
                }
                s
            }
            OutputFormat::Csv => {
                let mut s = String::from("file,app_type,size,is_running,is_dir\n");
                for r in &results {
                    let escaped_file = r.file.replace("\"", "\"\"");
                    s.push_str(&format!(
                        "\"{}\",\"{}\",{},{},{}\n",
                        escaped_file, r.app_type, r.size, r.is_running, r.is_dir
                    ));
                }
                s
            }
        };

        if let Some(path) = output_path {
            if let Err(e) = std::fs::write(&path, output_str) {
                eprintln!("Error writing to {}: {}", path, e);
                std::process::exit(1);
            }
        } else {
            println!("{}", output_str);
        }
        std::process::exit(0);
    }
}
