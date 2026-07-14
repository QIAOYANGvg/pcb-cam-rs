use std::env;
use std::path::{Path, PathBuf};

use gerber_parse::export::golden::export_golden_json;
use gerber_parse::readgerb::load_gerber_file;

fn main() {
    let mut args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().and_then(|arg| arg.to_str()) == Some("--json") && args.len() == 3 {
        let input = PathBuf::from(args.remove(1));
        let output = PathBuf::from(args.remove(1));
        export_one_json(&input, &output);
        return;
    }

    if args.first().and_then(|arg| arg.to_str()) == Some("--json-all") && args.len() == 3 {
        let input = PathBuf::from(args.remove(1));
        let output = PathBuf::from(args.remove(1));
        export_all_json(&input, &output);
        return;
    }

    let mut args = args.into_iter();
    let Some(input) = args.next() else {
        eprintln!("usage: gerber-parse <gerber-file-or-directory>");
        eprintln!("       gerber-parse --json <input.gbr> <output.json>");
        eprintln!("       gerber-parse --json-all <input-dir> <output-dir>");
        std::process::exit(2);
    };

    let input = PathBuf::from(input);
    let files = collect_inputs(&input);

    if files.is_empty() {
        eprintln!("no Gerber files found in {}", input.display());
        std::process::exit(1);
    }

    let mut failed = false;

    for file in files {
        match load_gerber_file(&file.to_string_lossy()) {
            Ok(image) => {
                println!(
                    "{}: drawings={} apertures={} messages={} missing_dcode={}",
                    file.display(),
                    image.drawings.len(),
                    image.aperture_list.len(),
                    image.messages.len(),
                    image.has_missing_dcode
                );

                for message in image.messages.iter().take(5) {
                    println!("  message: {}", message);
                }
            }
            Err(messages) => {
                failed = true;
                println!("{}: failed", file.display());

                for message in messages {
                    println!("  error: {}", message);
                }
            }
        }
    }

    if failed {
        std::process::exit(1);
    }
}

fn export_one_json(input: &Path, output: &Path) {
    let image = match load_gerber_file(&input.to_string_lossy()) {
        Ok(image) => image,
        Err(messages) => {
            for message in messages {
                eprintln!("{}", message);
            }
            std::process::exit(1);
        }
    };

    if let Some(parent) = output.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            eprintln!("cannot create {}: {}", parent.display(), err);
            std::process::exit(1);
        }
    }

    let json = export_golden_json(&image);
    let bytes = match serde_json::to_vec_pretty(&json) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("cannot serialize {}: {}", input.display(), err);
            std::process::exit(1);
        }
    };

    if let Err(err) = std::fs::write(output, bytes) {
        eprintln!("cannot write {}: {}", output.display(), err);
        std::process::exit(1);
    }
}

fn export_all_json(input: &Path, output: &Path) {
    let files = collect_inputs(input);

    if let Err(err) = std::fs::create_dir_all(output) {
        eprintln!("cannot create {}: {}", output.display(), err);
        std::process::exit(1);
    }

    for file in files {
        let Some(stem) = file.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        let output_file = output.join(format!("{stem}.json"));
        export_one_json(&file, &output_file);
        println!("OK: {} -> {}", file.display(), output_file.display());
    }
}

fn collect_inputs(input: &Path) -> Vec<PathBuf> {
    if input.is_file() {
        return vec![input.to_path_buf()];
    }

    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(input) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() && is_gerber_path(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

fn is_gerber_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "gbr"
            | "gtl"
            | "gbl"
            | "gto"
            | "gbo"
            | "gts"
            | "gbs"
            | "gtp"
            | "gbp"
            | "gko"
            | "gml"
            | "g1"
            | "g2"
            | "g3"
            | "g4"
            | "gdl"
            | "gdd"
            | "gta"
    )
}
