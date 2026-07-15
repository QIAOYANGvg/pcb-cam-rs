use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use gerber_parse::export::golden::export_golden_json;
use gerber_parse::readgerb::load_gerber_file;
use gerber_render_plan::RenderPlan;
use gerber_render_wgpu::{OffscreenRenderer, RenderedImage, RendererConfig};

type CliResult<T = ()> = Result<T, CliError>;

fn main() {
    if let Err(error) = run(env::args_os().skip(1)) {
        error.report();
        std::process::exit(error.exit_code);
    }
}

fn run<I, S>(args: I) -> CliResult
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    dispatch(parse_args(args)?)
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Inspect {
        input: PathBuf,
    },
    ExportJson {
        input: PathBuf,
        output: PathBuf,
    },
    ExportAllJson {
        input: PathBuf,
        output: PathBuf,
    },
    Render {
        input: PathBuf,
        output: PathBuf,
        dimensions: Option<(u32, u32)>,
    },
}

fn parse_args<I, S>(args: I) -> CliResult<Command>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into).collect::<Vec<_>>();

    if args.first().and_then(|arg| arg.to_str()) == Some("--json") && args.len() == 3 {
        return Ok(Command::ExportJson {
            input: PathBuf::from(args.remove(1)),
            output: PathBuf::from(args.remove(1)),
        });
    }

    if args.first().and_then(|arg| arg.to_str()) == Some("--json-all") && args.len() == 3 {
        return Ok(Command::ExportAllJson {
            input: PathBuf::from(args.remove(1)),
            output: PathBuf::from(args.remove(1)),
        });
    }

    if args.first().and_then(|arg| arg.to_str()) == Some("--render")
        && (args.len() == 3 || args.len() == 5)
    {
        let input = PathBuf::from(args.remove(1));
        let output = PathBuf::from(args.remove(1));
        let dimensions = if args.len() == 3 {
            Some((
                parse_dimension(&args[1], "width")?,
                parse_dimension(&args[2], "height")?,
            ))
        } else {
            None
        };

        return Ok(Command::Render {
            input,
            output,
            dimensions,
        });
    }

    let Some(input) = args.into_iter().next() else {
        return Err(CliError::usage());
    };

    Ok(Command::Inspect {
        input: PathBuf::from(input),
    })
}

fn dispatch(command: Command) -> CliResult {
    match command {
        Command::Inspect { input } => inspect(&input),
        Command::ExportJson { input, output } => export_one_json(&input, &output),
        Command::ExportAllJson { input, output } => export_all_json(&input, &output),
        Command::Render {
            input,
            output,
            dimensions,
        } => render_one_png(&input, &output, dimensions),
    }
}

fn inspect(input: &Path) -> CliResult {
    let files = collect_inputs(input);

    if files.is_empty() {
        return Err(CliError::failure(format!(
            "no Gerber files found in {}",
            input.display()
        )));
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
        Err(CliError::silent_failure())
    } else {
        Ok(())
    }
}

fn export_one_json(input: &Path, output: &Path) -> CliResult {
    let image = load_gerber_file(&input.to_string_lossy()).map_err(CliError::messages)?;

    create_parent_dirs(output)?;

    let json = export_golden_json(&image);
    let bytes = serde_json::to_vec_pretty(&json).map_err(|err| {
        CliError::failure(format!("cannot serialize {}: {}", input.display(), err))
    })?;

    std::fs::write(output, bytes)
        .map_err(|err| CliError::failure(format!("cannot write {}: {}", output.display(), err)))
}

fn export_all_json(input: &Path, output: &Path) -> CliResult {
    let files = collect_inputs(input);

    std::fs::create_dir_all(output)
        .map_err(|err| CliError::failure(format!("cannot create {}: {}", output.display(), err)))?;

    for file in files {
        let Some(stem) = file.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        let output_file = output.join(format!("{stem}.json"));
        export_one_json(&file, &output_file)?;
        println!("OK: {} -> {}", file.display(), output_file.display());
    }

    Ok(())
}

fn render_one_png(input: &Path, output: &Path, dimensions: Option<(u32, u32)>) -> CliResult {
    let total_started = Instant::now();
    let stage_started = Instant::now();
    let image = load_gerber_file(&input.to_string_lossy()).map_err(CliError::messages)?;
    let parse_elapsed = stage_started.elapsed();
    let stage_started = Instant::now();
    let plan = RenderPlan::from_image(&image).map_err(|err| {
        CliError::failure(format!(
            "cannot build render plan for {}: {}",
            input.display(),
            err
        ))
    })?;
    let plan_elapsed = stage_started.elapsed();
    let mut config = RendererConfig::default();

    if let Some((width, height)) = dimensions {
        config.width = width;
        config.height = height;
    }

    let stage_started = Instant::now();
    let renderer = OffscreenRenderer::new_blocking(config)
        .map_err(|err| CliError::failure(format!("cannot initialize wgpu renderer: {err}")))?;
    let init_elapsed = stage_started.elapsed();
    let stage_started = Instant::now();
    let rendered = renderer
        .render_blocking(&plan)
        .map_err(|err| CliError::failure(format!("cannot render {}: {err}", input.display())))?;
    let render_elapsed = stage_started.elapsed();

    create_parent_dirs(output)?;
    let stage_started = Instant::now();
    write_png(output, &rendered).map_err(CliError::failure)?;
    let png_elapsed = stage_started.elapsed();

    println!(
        "OK: {} -> {} ({}x{}, adapter={}, parse={:.3}s, plan={:.3}s, gpu-init={:.3}s, \
         render+readback={:.3}s, png={:.3}s, total={:.3}s)",
        input.display(),
        output.display(),
        rendered.width,
        rendered.height,
        renderer.adapter_info().name,
        parse_elapsed.as_secs_f64(),
        plan_elapsed.as_secs_f64(),
        init_elapsed.as_secs_f64(),
        render_elapsed.as_secs_f64(),
        png_elapsed.as_secs_f64(),
        total_started.elapsed().as_secs_f64()
    );

    Ok(())
}

fn parse_dimension(value: &OsStr, name: &str) -> CliResult<u32> {
    value
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| CliError::invalid_arguments(format!("{name} must be a positive integer")))
}

fn create_parent_dirs(output: &Path) -> CliResult {
    let Some(parent) = output.parent() else {
        return Ok(());
    };

    std::fs::create_dir_all(parent)
        .map_err(|err| CliError::failure(format!("cannot create {}: {}", parent.display(), err)))
}

fn write_png(output: &Path, image: &RenderedImage) -> Result<(), String> {
    let file =
        File::create(output).map_err(|err| format!("cannot create {}: {err}", output.display()))?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fastest);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("cannot initialize {}: {err}", output.display()))?;
    writer
        .write_image_data(&image.rgba)
        .map_err(|err| format!("cannot write {}: {err}", output.display()))
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

#[derive(Debug)]
struct CliError {
    exit_code: i32,
    lines: Vec<String>,
}

impl CliError {
    fn usage() -> Self {
        Self {
            exit_code: 2,
            lines: vec![
                "usage: gerber-parse <gerber-file-or-directory>".to_owned(),
                "       gerber-parse --json <input.gbr> <output.json>".to_owned(),
                "       gerber-parse --json-all <input-dir> <output-dir>".to_owned(),
                "       gerber-parse --render <input.gbr> <output.png> [width height]".to_owned(),
            ],
        }
    }

    fn invalid_arguments(message: String) -> Self {
        Self {
            exit_code: 2,
            lines: vec![message],
        }
    }

    fn failure(message: String) -> Self {
        Self {
            exit_code: 1,
            lines: vec![message],
        }
    }

    fn messages(messages: Vec<String>) -> Self {
        Self {
            exit_code: 1,
            lines: messages,
        }
    }

    fn silent_failure() -> Self {
        Self {
            exit_code: 1,
            lines: Vec::new(),
        }
    }

    fn report(&self) {
        for line in &self.lines {
            eprintln!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_explicit_mode() {
        assert_eq!(
            parse_args(["--json", "input.gbr", "output.json"]).unwrap(),
            Command::ExportJson {
                input: PathBuf::from("input.gbr"),
                output: PathBuf::from("output.json"),
            }
        );
        assert_eq!(
            parse_args(["--json-all", "inputs", "outputs"]).unwrap(),
            Command::ExportAllJson {
                input: PathBuf::from("inputs"),
                output: PathBuf::from("outputs"),
            }
        );
        assert_eq!(
            parse_args(["--render", "input.gbr", "output.png"]).unwrap(),
            Command::Render {
                input: PathBuf::from("input.gbr"),
                output: PathBuf::from("output.png"),
                dimensions: None,
            }
        );
    }

    #[test]
    fn parses_render_dimensions() {
        assert_eq!(
            parse_args(["--render", "input.gbr", "output.png", "800", "600"]).unwrap(),
            Command::Render {
                input: PathBuf::from("input.gbr"),
                output: PathBuf::from("output.png"),
                dimensions: Some((800, 600)),
            }
        );
    }

    #[test]
    fn invalid_render_dimension_is_an_argument_error() {
        let error = parse_args(["--render", "input.gbr", "output.png", "0", "600"]).unwrap_err();

        assert_eq!(error.exit_code, 2);
        assert_eq!(error.lines, ["width must be a positive integer"]);
    }

    #[test]
    fn unrecognized_shapes_fall_back_to_inspection() {
        assert_eq!(
            parse_args(["--json", "only-one-path"]).unwrap(),
            Command::Inspect {
                input: PathBuf::from("--json"),
            }
        );
        assert_eq!(
            parse_args(["board.gbr", "ignored-extra-argument"]).unwrap(),
            Command::Inspect {
                input: PathBuf::from("board.gbr"),
            }
        );
    }

    #[test]
    fn empty_arguments_return_usage() {
        let error = parse_args(std::iter::empty::<OsString>()).unwrap_err();

        assert_eq!(error.exit_code, 2);
        assert_eq!(error.lines.len(), 4);
    }
}
