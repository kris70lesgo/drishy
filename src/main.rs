use drishy::{decode, encode, index::DecodeIndex, parallel, restart_qoi};
use image::{ColorType, ImageReader};
use std::{env, error::Error, fs, io, path::Path};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().collect();
    match arguments.get(1).map(String::as_str) {
        Some("encode") if arguments.len() == 4 => encode_png(&arguments[2], &arguments[3]),
        Some("decode") if arguments.len() == 4 => decode_qoi(&arguments[2], &arguments[3]),
        Some("encode-restart") if arguments.len() == 4 => {
            encode_png_restart(&arguments[2], &arguments[3])
        }
        Some("decode-restart") if arguments.len() == 4 => {
            decode_restart(&arguments[2], &arguments[3])
        }
        Some("index") if arguments.len() == 4 => make_index(&arguments[2], &arguments[3]),
        Some("decode-indexed") if arguments.len() == 5 => {
            decode_indexed(&arguments[2], &arguments[3], &arguments[4])
        }
        _ => {
            print_usage();
            Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid command or arguments").into())
        }
    }
}

fn encode_png(input: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let image = ImageReader::open(input)?.with_guessed_format()?.decode()?;
    let qoi = if image.color().has_alpha() {
        let image = image.into_rgba8();
        encode::encode_rgb_or_rgba(image.width(), image.height(), 4, image.as_raw())?
    } else {
        let image = image.into_rgb8();
        encode::encode_rgb_or_rgba(image.width(), image.height(), 3, image.as_raw())?
    };
    fs::write(output, qoi)?;
    Ok(())
}

fn decode_qoi(input: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(input)?;
    let (header, pixels) = decode::decode(&bytes)?;
    let color = match header.channels {
        3 => ColorType::Rgb8,
        4 => ColorType::Rgba8,
        _ => unreachable!("validated QOI channel count"),
    };
    image::save_buffer(output, &pixels, header.width, header.height, color)?;
    Ok(())
}

fn encode_png_restart(input: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let image = ImageReader::open(input)?.with_guessed_format()?.decode()?;
    let restart = if image.color().has_alpha() {
        let image = image.into_rgba8();
        restart_qoi::encode_rgb_or_rgba(image.width(), image.height(), 4, image.as_raw())?
    } else {
        let image = image.into_rgb8();
        restart_qoi::encode_rgb_or_rgba(image.width(), image.height(), 3, image.as_raw())?
    };
    fs::write(output, restart)?;
    Ok(())
}

fn decode_restart(input: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(input)?;
    let (header, pixels) = restart_qoi::decode(&bytes)?;
    let color = if header.channels == 3 {
        ColorType::Rgb8
    } else {
        ColorType::Rgba8
    };
    image::save_buffer(output, &pixels, header.width, header.height, color)?;
    Ok(())
}

fn make_index(input: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(input)?;
    let workers = std::thread::available_parallelism()?.get();
    let index = DecodeIndex::build(&bytes, workers)?;
    let mut file = fs::File::create(output)?;
    index.write_to(&mut file)?;
    println!(
        "Wrote {} decode checkpoints for {}",
        index.checkpoint_count(),
        Path::new(input).display()
    );
    Ok(())
}

fn decode_indexed(input: &str, index_path: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(input)?;
    let index = DecodeIndex::read_from(fs::File::open(index_path)?)?;
    let (header, pixels) = parallel::decode_indexed(&bytes, &index)?;
    let color = if header.channels == 3 {
        ColorType::Rgb8
    } else {
        ColorType::Rgba8
    };
    image::save_buffer(output, &pixels, header.width, header.height, color)?;
    Ok(())
}

fn print_usage() {
    eprintln!(
        "Usage:\n  drishy encode <input.png> <output.qoi>\n  drishy decode <input.qoi> <output.png>\n  drishy encode-restart <input.png> <output.drq>\n  drishy decode-restart <input.drq> <output.png>\n  drishy index <input.qoi> <output.idx>\n  drishy decode-indexed <input.qoi> <input.idx> <output.png>"
    );
}
