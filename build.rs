use std::path::Path;
use std::process::Command;

fn main() {
    // Rerun if the stub template changes.
    println!("cargo:rerun-if-changed=src/bin/stub_template.c");

    let out_dir = "_build";
    std::fs::create_dir_all(out_dir).expect("failed to create _build/");

    // Compile a proof-of-concept stub with empty args to verify gcc works.
    // fuselage-pack recompiles from stub_template.c at pack time with real args.
    let placeholder_src = Path::new(out_dir).join("stub_placeholder.c");
    let stub_template =
        std::fs::read_to_string("src/bin/stub_template.c").expect("failed to read stub_template.c");

    // Replace the FUSELAGE_PACK_ARGS macro with an empty definition.
    let placeholder = stub_template.replace("    FUSELAGE_PACK_ARGS\n", "");
    std::fs::write(&placeholder_src, placeholder).expect("failed to write stub_placeholder.c");

    let stub_out = Path::new(out_dir).join("stub_placeholder");
    let status = Command::new("gcc")
        .args([
            "-static",
            "-O2",
            "-o",
            stub_out.to_str().unwrap(),
            placeholder_src.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run gcc — is gcc installed?");

    if !status.success() {
        panic!("gcc failed to compile stub_placeholder.c");
    }
}
