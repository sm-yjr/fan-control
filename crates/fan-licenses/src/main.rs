use std::path::PathBuf;

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let mut metadata = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--metadata") if metadata.is_none() => {
                metadata = Some(PathBuf::from(
                    args.next().ok_or("missing metadata argument")?,
                ));
            }
            Some("--output") if output.is_none() => {
                output = Some(PathBuf::from(args.next().ok_or("missing output argument")?));
            }
            Some("--help") => {
                println!(
                    "fan-licenses --metadata <cargo-metadata.json> --output <empty-directory>"
                );
                return Ok(());
            }
            _ => return Err("unknown or duplicate argument".into()),
        }
    }
    let metadata = metadata.ok_or("--metadata is required")?;
    let output = output.ok_or("--output is required")?;
    let bytes = std::fs::read(metadata).map_err(|_| "cannot read Cargo metadata")?;
    let count = fan_licenses::collect(&bytes, &output)?;
    println!("Collected original license materials for {count} registry packages.");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        // Errors intentionally omit local paths and metadata excerpts.
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
