use flatc_rust::run;

use mktemp::Temp;
use std::{fs, fs::File, io, io::Write, path::Path};

fn generate_chess_flatbuff() -> Result<(), std::io::Error> {
    run(flatc_rust::Args {
        inputs: &[Path::new("chess_flat_buffer/chess.fbs")],
        out_dir: Path::new("target/flatbuffers/"),
        ..Default::default()
    })
    .expect("flatc");

    let data = "// @generated\r\n\r\n// Force clippy and checks to ignore this file\n#![allow(clippy::all)]\n#![allow(unknown_lints)]\n#![allow(unused_imports)]\n#![allow(clippy::cognitive_complexity)]\n#![allow(clippy::pedantic)]\n\n";

    let file_path = Path::new("target/flatbuffers/chess_generated.rs");
    prepend_file(data.as_bytes(), &file_path)?;

    Ok(())
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=./build.rs");
    println!("cargo:rerun-if-changed=./Cargo.lock");

    generate_chess_flatbuff()?;

    Ok(())
}

// Modified from https://stackoverflow.com/questions/43441166/prepend-line-to-beginning-of-file
fn prepend_file<P: AsRef<Path>>(data: &[u8], file_path: &P) -> io::Result<()> {
    // Create a temporary file
    let tmp_path = Temp::new_file()?;
    // Open temp file for writing
    let mut tmp = File::create(&tmp_path)?;
    // Open source file for reading
    let mut src = File::open(&file_path)?;
    // Write the data to prepend
    tmp.write_all(data)?;
    // Copy the rest of the source file
    io::copy(&mut src, &mut tmp)?;
    fs::remove_file(&file_path)?;
    fs::copy(&tmp_path, &file_path)?;
    Ok(())
}
