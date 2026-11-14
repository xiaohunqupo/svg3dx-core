use std::env;
use std::fs;
use std::process;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: eval_file <file.svg>");
        process::exit(1);
    });
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("read {path}: {e}");
        process::exit(1);
    });
    match svg3dx_core::parse_and_evaluate(&text) {
        Ok(result) => {
            for w in &result.warnings {
                eprintln!("warning: {w}");
            }
            match svg3dx_core::to_json(&result) {
                Ok(json) => println!("{json}"),
                Err(e) => {
                    eprintln!("{e}");
                    process::exit(2);
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}
