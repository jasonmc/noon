use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=GEONAMES_CITIES_PATH");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let mut out = BufWriter::new(File::create(out_dir.join("cities.tsv")).unwrap());

    if let Some(path) = env::var_os("GEONAMES_CITIES_PATH") {
        let path = PathBuf::from(path);
        println!("cargo::rerun-if-changed={}", path.display());
        for line in fs::read_to_string(path).unwrap().lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() >= 12 && !line.starts_with('#') {
                writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    cols[1], cols[8], cols[10], cols[11], cols[4], cols[5]
                )
                .unwrap();
            }
        }
    } else {
        for c in cities_json::CITIES.iter() {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                c.name, c.country, c.admin1, c.admin2, c.lat, c.lng
            )
            .unwrap();
        }
    }
}
