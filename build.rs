use std::path::PathBuf;

const VERSION_TEMPLATE: &str = r#"
pub const VERSION: &str = "{version}";
pub const HW_VER: &str = "{hw}";
pub const FIRMWARE: &str = "{firmware}";
"#;

fn main() {
    println!("cargo:rerun-if-changed=*.env*");
    if let Ok(mut iter) = dotenvy::dotenv_iter() {
        while let Some(Ok((key, value))) = iter.next() {
            println!("cargo:rustc-env={key}={value}");
        }
    }

    linker_be_nice();
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    println!("cargo:rustc-cfg=feature=\"gen_version\"");

    let version_str = if let Ok(rel) = std::env::var("RELEASE_BUILD") {
        println!("cargo:rustc-cfg=feature=\"release_build\"");
        rel
    } else {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        format!("D{epoch}")
    };

    let hw = if cfg!(feature = "esp32c3") {
        "v2"
    } else {
        "unknown"
    };

    let gen = VERSION_TEMPLATE
        .replace("{version}", &version_str)
        .replace("{hw}", hw)
        .replace("{firmware}", "STAFF_ATTENDANCE");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("version.rs"), gen.trim()).unwrap();
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!("💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`");
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
