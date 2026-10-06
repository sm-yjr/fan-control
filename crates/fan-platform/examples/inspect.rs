//! Read-only hardware acceptance probe. Does not install a service or write SMC.
fn main() {
    if std::env::args().any(|arg| arg == "--fan-registers") {
        let mut smc = fan_platform::Smc::open().unwrap();
        for key in ["FNum", "F0md", "F0Md", "F1md", "F1Md", "Ftst"] {
            println!("{key}: {:?}", smc.read_key(key));
        }
        return;
    }
    let result = fan_platform::Smc::open().and_then(|mut smc| smc.discover_snapshot());
    match result {
        Ok(snapshot) => println!("{}", serde_json::to_string_pretty(&snapshot).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
