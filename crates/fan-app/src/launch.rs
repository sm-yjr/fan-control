#[derive(Debug, PartialEq, Eq)]
pub enum LaunchMode {
    Gui { demo: bool },
    Helper,
    ReadSensors,
    UpdaterCheck,
    UiSmoke,
    PackageInfo,
    Help,
}
impl LaunchMode {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let args: Vec<_> = args.into_iter().skip(1).collect();
        match args.as_slice() {
            [] => Ok(Self::Gui { demo: false }),
            [arg] => match arg.as_str() {
                "--helper" => Ok(Self::Helper),
                "--demo" => Ok(Self::Gui { demo: true }),
                "--read-sensors" => Ok(Self::ReadSensors),
                "--check-updater-runtime" => Ok(Self::UpdaterCheck),
                "--ui-smoke" => Ok(Self::UiSmoke),
                "--package-info" => Ok(Self::PackageInfo),
                "--help" | "-h" => Ok(Self::Help),
                _ => Err(format!("unknown argument: {arg}")),
            },
            _ => Err("only one launch mode may be selected".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<LaunchMode, String> {
        LaunchMode::parse(args.iter().map(|s| s.to_string()))
    }
    #[test]
    fn helper_cannot_be_combined_with_gui_diagnostics() {
        assert!(parse(&["app", "--helper", "--demo"]).is_err());
    }
    #[test]
    fn diagnostics_are_explicit() {
        assert_eq!(
            parse(&["app", "--read-sensors"]).unwrap(),
            LaunchMode::ReadSensors
        );
        assert!(parse(&["app", "--unknown"]).is_err());
    }
}
