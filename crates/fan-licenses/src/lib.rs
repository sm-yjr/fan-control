//! Offline collection of original Cargo registry license materials.
//!
//! The reviewed identifier set is deliberately small. Every identifier in an
//! expression must be reviewed, even in an OR branch. AND binds more tightly
//! than OR; parentheses are parsed rather than discarded. WITH, LicenseRef,
//! deprecated slash syntax and additional identifiers require an explicit review.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
const VENDORED_SOURCES: &str = include_str!("../licenses/sources.json");

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
    manifest_path: PathBuf,
    license: Option<String>,
    license_file: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Sources {
    format_version: u32,
    packages: Vec<PinnedPackage>,
}

#[derive(Deserialize)]
struct PinnedPackage {
    crate_name: String,
    version: String,
    license: String,
    reviewed_expression: Option<String>,
    revision: String,
    files: Vec<PinnedFile>,
}

#[derive(Deserialize)]
struct PinnedFile {
    vendored_file: PathBuf,
    file_name: PathBuf,
    source_url: String,
    sha256: String,
}

#[derive(Deserialize)]
struct VcsInfo {
    git: GitInfo,
}

#[derive(Deserialize)]
struct GitInfo {
    sha1: String,
}

#[derive(Serialize)]
struct Manifest {
    format_version: u32,
    packages: Vec<ManifestPackage>,
}

#[derive(Serialize)]
struct ManifestPackage {
    crate_name: String,
    version: String,
    license: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reviewed_expression: Option<String>,
    reviewed_identifiers: Vec<String>,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    /// Relative to the output directory; never an absolute developer path.
    file: String,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_url: Option<String>,
}

struct Material {
    relative: PathBuf,
    bytes: Vec<u8>,
    source_url: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Expression {
    License(String),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
}

struct Parser<'a> {
    tokens: Vec<&'a str>,
    position: usize,
}

impl<'a> Parser<'a> {
    fn expression(&mut self, depth: usize) -> Result<Expression, String> {
        let mut result = self.conjunction(depth)?;
        while self.consume("OR") {
            result = Expression::Or(Box::new(result), Box::new(self.conjunction(depth)?));
        }
        Ok(result)
    }

    fn conjunction(&mut self, depth: usize) -> Result<Expression, String> {
        let mut result = self.primary(depth)?;
        while self.consume("AND") {
            result = Expression::And(Box::new(result), Box::new(self.primary(depth)?));
        }
        Ok(result)
    }

    fn primary(&mut self, depth: usize) -> Result<Expression, String> {
        if depth > 32 {
            return Err("SPDX expression nesting is too deep".into());
        }
        if self.consume("(") {
            let inner = self.expression(depth + 1)?;
            if !self.consume(")") {
                return Err("unclosed SPDX expression parenthesis".into());
            }
            return Ok(inner);
        }
        let identifier = self
            .tokens
            .get(self.position)
            .ok_or("incomplete SPDX expression")?;
        if !matches!(
            *identifier,
            "MIT"
                | "Apache-2.0"
                | "Zlib"
                | "BSD-2-Clause"
                | "BSD-3-Clause"
                | "ISC"
                | "Unlicense"
                | "Unicode-3.0"
        ) {
            return Err("unsupported or malformed SPDX license identifier".into());
        }
        self.position += 1;
        Ok(Expression::License((*identifier).to_owned()))
    }

    fn consume(&mut self, token: &str) -> bool {
        if self.tokens.get(self.position) == Some(&token) {
            self.position += 1;
            true
        } else {
            false
        }
    }
}

fn parse_expression(input: &str) -> Result<Expression, String> {
    if input.len() > 4096 || !input.is_ascii() {
        return Err("invalid SPDX expression length or encoding".into());
    }
    let mut tokens = Vec::new();
    let mut start = None;
    for (offset, byte) in input.bytes().enumerate() {
        if byte.is_ascii_whitespace() || matches!(byte, b'(' | b')') {
            if let Some(from) = start.take() {
                tokens.push(&input[from..offset]);
            }
            if matches!(byte, b'(' | b')') {
                tokens.push(&input[offset..offset + 1]);
            }
        } else {
            start.get_or_insert(offset);
        }
    }
    if let Some(from) = start {
        tokens.push(&input[from..]);
    }
    let mut parser = Parser {
        tokens,
        position: 0,
    };
    let expression = parser.expression(0)?;
    if parser.position != parser.tokens.len() {
        return Err("unparsed or unsupported SPDX expression suffix".into());
    }
    Ok(expression)
}

fn identifiers(expression: &Expression, result: &mut BTreeSet<String>) {
    match expression {
        Expression::License(identifier) => {
            result.insert(identifier.clone());
        }
        Expression::And(left, right) | Expression::Or(left, right) => {
            identifiers(left, result);
            identifiers(right, result);
        }
    }
}

fn safe_component(input: &str) -> bool {
    !input.is_empty()
        && input != "."
        && input != ".."
        && input
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'+'))
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|part| match part {
            Component::Normal(value) => value.to_str().is_some_and(|text| {
                !text.contains(['\\', ':']) && !text.chars().any(char::is_control)
            }),
            _ => false,
        })
}

/// Match legal text files without treating copying.rs or license.rs as notices.
fn legal_filename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let upper = name.to_ascii_uppercase();
    let stem = if let Some((stem, extension)) = upper.rsplit_once('.') {
        if !matches!(
            extension,
            "TXT"
                | "MD"
                | "MARKDOWN"
                | "RST"
                | "HTML"
                | "HTM"
                | "ADOC"
                | "MIT"
                | "APACHE"
                | "BSD"
                | "ISC"
                | "ZLIB"
        ) {
            return false;
        }
        stem
    } else {
        &upper
    };
    [
        "LICENSE",
        "LICENCE",
        "UNLICENSE",
        "COPYING",
        "COPYRIGHT",
        "NOTICE",
    ]
    .iter()
    .any(|prefix| {
        stem == *prefix
            || stem
                .strip_prefix(prefix)
                .is_some_and(|tail| tail.starts_with(['-', '_', ' ']))
    })
}

fn find_materials(
    root: &Path,
    directory: &Path,
    result: &mut BTreeSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("registry directory nesting is too deep".into());
    }
    for entry in fs::read_dir(directory).map_err(|_| "cannot inspect registry package")? {
        let entry = entry.map_err(|_| "cannot inspect registry entry")?;
        let kind = entry
            .file_type()
            .map_err(|_| "cannot inspect registry entry type")?;
        // Never follow package symlinks, including legal files pointing outside.
        if kind.is_symlink() {
            return Err("registry package contains a symlink; review required".into());
        }
        let path = entry.path();
        if kind.is_dir() {
            find_materials(root, &path, result, depth + 1)?;
        } else if kind.is_file() && legal_filename(&path) {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "registry path escaped package")?;
            if !safe_relative(relative) {
                return Err("unsafe registry legal file path".into());
            }
            result.insert(relative.to_owned());
        }
    }
    Ok(())
}

fn read_text(root: &Path, relative: &Path) -> Result<Vec<u8>, String> {
    if !safe_relative(relative) {
        return Err("unsafe license file path".into());
    }
    // Inspect every component so an explicitly declared license_file cannot
    // escape through a symlink even if no named legal file was discovered.
    let mut current = root.to_owned();
    for component in relative.components() {
        current.push(component);
        if fs::symlink_metadata(&current)
            .map_err(|_| "missing original license file")?
            .file_type()
            .is_symlink()
        {
            return Err("license file contains a symlink".into());
        }
    }
    let info = fs::metadata(&current).map_err(|_| "cannot inspect original license file")?;
    if !info.is_file() || info.len() == 0 || info.len() > MAX_TEXT_BYTES {
        return Err("original license file must be nonempty text within size limit".into());
    }
    let bytes = fs::read(current).map_err(|_| "cannot read original license file")?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "original license file is not UTF-8 text")?;
    if text.trim().is_empty() || text.contains('\0') {
        return Err("original license text is empty or binary".into());
    }
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn verify_revision(root: &Path, pin: &PinnedPackage) -> Result<(), String> {
    let vcs: VcsInfo = serde_json::from_slice(&read_text(root, Path::new(".cargo_vcs_info.json"))?)
        .map_err(|_| "invalid registry revision metadata")?;
    if vcs.git.sha1 != pin.revision
        || pin.revision.len() != 40
        || !pin.revision.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("upstream source revision does not match registry release".into());
    }
    Ok(())
}

fn pinned_materials(
    package: &Package,
    root: &Path,
    sources: &Sources,
    vendor: &Path,
) -> Result<Vec<Material>, String> {
    let pin = sources
        .packages
        .iter()
        .find(|pin| pin.crate_name == package.name && pin.version == package.version)
        .ok_or("original license files missing; no exact reviewed upstream source")?;
    if package.license.as_deref() != Some(&pin.license) || pin.files.is_empty() {
        return Err("upstream source license declaration does not match".into());
    }
    verify_revision(root, pin)?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for file in &pin.files {
        if !safe_relative(&file.file_name) || !seen.insert(file.file_name.clone()) {
            return Err("unsafe or duplicate pinned legal file name".into());
        }
        if !file.source_url.starts_with("https://") || file.source_url.chars().any(char::is_control)
        {
            return Err("invalid pinned upstream source URL".into());
        }
        let bytes = read_text(vendor, &file.vendored_file)?;
        if digest(&bytes) != file.sha256 {
            return Err("pinned upstream original SHA-256 mismatch".into());
        }
        result.push(Material {
            relative: file.file_name.clone(),
            bytes,
            source_url: Some(file.source_url.clone()),
        });
    }
    Ok(result)
}

fn package_materials(
    package: &Package,
    sources: &Sources,
    vendor: &Path,
) -> Result<Vec<Material>, String> {
    let root = package
        .manifest_path
        .parent()
        .ok_or("invalid registry manifest location")?;
    let mut files = BTreeSet::new();
    find_materials(root, root, &mut files, 0)?;
    if let Some(declared) = &package.license_file {
        // cargo metadata supplies license_file relative to the manifest.
        if !safe_relative(declared) {
            return Err("unsafe declared license_file path".into());
        }
        // A missing declared file must fail, even when another LICENSE exists.
        read_text(root, declared)?;
        files.insert(declared.clone());
    }
    if files.is_empty() {
        if sources.packages.iter().any(|pin| {
            pin.crate_name == package.name
                && pin.version == package.version
                && pin.reviewed_expression.is_some()
        }) {
            return Err("original license files missing for reviewed legacy expression".into());
        }
        return pinned_materials(package, root, sources, vendor);
    }
    let mut materials: Vec<Material> = files
        .into_iter()
        .map(|relative| {
            let bytes = read_text(root, &relative)?;
            Ok(Material {
                relative,
                bytes,
                source_url: None,
            })
        })
        .collect::<Result<_, String>>()?;
    // An exact reviewed legacy-expression entry retains its upstream evidence
    // in addition to every original license found in the registry archive.
    if sources.packages.iter().any(|pin| {
        pin.crate_name == package.name
            && pin.version == package.version
            && pin.reviewed_expression.is_some()
    }) {
        materials.extend(pinned_materials(package, root, sources, vendor)?);
    }
    Ok(materials)
}

/// Collect all registry packages in a full `cargo metadata --locked` document.
/// Existing nonempty output directories are rejected to avoid replacing files
/// unrelated to this collector. Collection and validation precede all writes.
pub fn collect(metadata: &[u8], output: &Path) -> Result<usize, String> {
    let sources: Sources =
        serde_json::from_str(VENDORED_SOURCES).map_err(|_| "invalid vendored source index")?;
    if sources.format_version != 1 {
        return Err("unsupported vendored source index version".into());
    }
    collect_with_sources(
        metadata,
        output,
        &sources,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("licenses"),
    )
}

fn collect_with_sources(
    metadata: &[u8],
    output: &Path,
    sources: &Sources,
    vendor: &Path,
) -> Result<usize, String> {
    let metadata: Metadata =
        serde_json::from_slice(metadata).map_err(|_| "invalid Cargo metadata document")?;
    let mut packages = BTreeMap::new();
    for package in metadata.packages {
        let Some(source) = &package.source else {
            continue;
        };
        if !source.starts_with("registry+") {
            return Err(
                "non-registry external dependency requires a license collection review".into(),
            );
        }
        if !safe_component(&package.name) || !safe_component(&package.version) {
            return Err("unsafe registry package name or version".into());
        }
        if packages
            .insert((package.name.clone(), package.version.clone()), package)
            .is_some()
        {
            return Err("duplicate registry package name/version; source review required".into());
        }
    }
    if packages.is_empty() {
        return Err("Cargo metadata contains no registry dependencies".into());
    }
    let mut manifest = Manifest {
        format_version: 1,
        packages: Vec::new(),
    };
    let mut writes = Vec::new();
    for ((name, version), package) in packages {
        let checked = (|| -> Result<ManifestPackage, String> {
            let license = package
                .license
                .as_deref()
                .ok_or("registry package has no SPDX license declaration")?;
            let reviewed_expression = sources
                .packages
                .iter()
                .find(|pin| {
                    pin.crate_name == package.name
                        && pin.version == package.version
                        && pin.license == license
                        && pin.reviewed_expression.is_some()
                })
                .map(|pin| {
                    let root = package
                        .manifest_path
                        .parent()
                        .ok_or("invalid registry manifest location")?;
                    verify_revision(root, pin)?;
                    Ok::<_, String>(pin.reviewed_expression.clone())
                })
                .transpose()?
                .flatten();
            let expression = parse_expression(reviewed_expression.as_deref().unwrap_or(license))?;
            let mut reviewed = BTreeSet::new();
            identifiers(&expression, &mut reviewed);
            let mut files = Vec::new();
            for material in package_materials(&package, sources, vendor)? {
                let relative = Path::new(&name).join(&version).join(material.relative);
                let file = relative
                    .to_str()
                    .ok_or("non-UTF-8 legal file path")?
                    .replace('\\', "/");
                files.push(ManifestFile {
                    file,
                    sha256: digest(&material.bytes),
                    source_url: material.source_url,
                });
                writes.push((relative, material.bytes));
            }
            files.sort_by(|a, b| a.file.cmp(&b.file));
            Ok(ManifestPackage {
                crate_name: name.clone(),
                version: version.clone(),
                license: license.to_owned(),
                reviewed_expression,
                reviewed_identifiers: reviewed.into_iter().collect(),
                files,
            })
        })()
        .map_err(|error| format!("{name} {version}: {error}"))?;
        manifest.packages.push(checked);
    }
    if output.exists() {
        let info = fs::symlink_metadata(output).map_err(|_| "cannot inspect output directory")?;
        if info.file_type().is_symlink()
            || !info.is_dir()
            || fs::read_dir(output)
                .map_err(|_| "cannot inspect output directory")?
                .next()
                .is_some()
        {
            return Err("output must be an absent or empty directory".into());
        }
    }
    fs::create_dir_all(output).map_err(|_| "cannot create output directory")?;
    for (relative, bytes) in writes {
        let destination = output.join(relative);
        fs::create_dir_all(destination.parent().ok_or("invalid output file location")?)
            .map_err(|_| "cannot create package license directory")?;
        fs::write(destination, bytes).map_err(|_| "cannot write original license material")?;
    }
    let mut encoded =
        serde_json::to_vec_pretty(&manifest).map_err(|_| "cannot encode license manifest")?;
    encoded.push(b'\n');
    fs::write(output.join("manifest.json"), encoded)
        .map_err(|_| "cannot write license manifest")?;
    Ok(manifest.packages.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "fan-licenses-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn write(&self, relative: &str, bytes: &[u8]) {
            let file = self.0.join(relative);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }

        fn metadata(&self, license: Option<&str>, declared: Option<&str>) -> Vec<u8> {
            serde_json::to_vec(&json!({"packages":[{
                "name":"test-crate", "version":"1.2.3", "license":license,
                "source":"registry+https://github.com/rust-lang/crates.io-index",
                "manifest_path":self.0.join("package/Cargo.toml"), "license_file":declared
            }]}))
            .unwrap()
        }

        fn empty_sources() -> Sources {
            Sources {
                format_version: 1,
                packages: Vec::new(),
            }
        }

        fn collect(&self, license: Option<&str>, declared: Option<&str>) -> Result<usize, String> {
            collect_with_sources(
                &self.metadata(license, declared),
                &self.0.join("output"),
                &Self::empty_sources(),
                &self.0.join("vendor"),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn full_expression_preserves_and_obligations_and_precedence() {
        let expression = parse_expression("(MIT OR Apache-2.0) AND Unicode-3.0").unwrap();
        assert_eq!(
            expression,
            Expression::And(
                Box::new(Expression::Or(
                    Box::new(Expression::License("MIT".into())),
                    Box::new(Expression::License("Apache-2.0".into()))
                )),
                Box::new(Expression::License("Unicode-3.0".into()))
            )
        );
        let mut licenses = BTreeSet::new();
        identifiers(&expression, &mut licenses);
        assert_eq!(
            licenses.into_iter().collect::<Vec<_>>(),
            ["Apache-2.0", "MIT", "Unicode-3.0"]
        );
        assert_eq!(
            parse_expression("MIT OR Apache-2.0 AND Unicode-3.0").unwrap(),
            Expression::Or(
                Box::new(Expression::License("MIT".into())),
                Box::new(Expression::And(
                    Box::new(Expression::License("Apache-2.0".into())),
                    Box::new(Expression::License("Unicode-3.0".into()))
                ))
            )
        );
        assert!(parse_expression("Unlicense OR MIT").is_ok());
    }

    #[test]
    fn unknown_licenses_and_partial_expressions_are_rejected() {
        for expression in [
            "",
            "Proprietary",
            "MIT OR Proprietary",
            "(MIT OR Apache-2.0) AND Unknown-1.0",
            "MIT/Apache-2.0",
            "MIT WITH LLVM-exception",
            "MIT OR",
            "AND MIT",
            "MIT Apache-2.0",
            "(MIT",
            "MIT)",
            "()",
            "MIT+",
            "LicenseRef-Custom OR MIT",
            "MIT or Apache-2.0",
        ] {
            assert!(
                parse_expression(expression).is_err(),
                "accepted {expression}"
            );
        }
    }

    #[test]
    fn legal_filename_rules_do_not_copy_source_code() {
        for name in [
            "LICENSE",
            "LiCeNsE.md",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "LICENSE.BSD",
            "COPYING",
            "UNLICENSE",
            "COPYRIGHT.txt",
            "NOTICE-DEPENDENCIES.md",
            "nested/NOTICE",
        ] {
            assert!(legal_filename(Path::new(name)), "missed {name}");
        }
        for name in [
            "copying.rs",
            "license.rs",
            "README.md",
            "license.json",
            "LICENSE.png",
            "COPYING_HELPER.swift",
            "LICENSED.md",
        ] {
            assert!(!legal_filename(Path::new(name)), "copied {name}");
        }
    }

    #[test]
    fn paths_cannot_escape_or_use_platform_separators() {
        for path in [
            "",
            "/LICENSE",
            "../LICENSE",
            "licenses/../../LICENSE",
            "C:\\LICENSE",
            "licenses\\NOTICE",
            "licenses/NOTICE\n",
        ] {
            assert!(!safe_relative(Path::new(path)), "accepted {path:?}");
        }
        for value in [
            "",
            ".",
            "..",
            "../crate",
            "crate/name",
            "crate\\name",
            "user:crate",
        ] {
            assert!(!safe_component(value));
        }
        assert!(safe_relative(Path::new("licenses/COPYRIGHT.txt")));
        assert!(safe_component("1.2.3+build.4"));
    }

    #[test]
    fn every_original_is_copied_byte_for_byte_and_manifest_is_portable() {
        let fixture = Fixture::new();
        let originals: &[(&str, &[u8])] = &[
            ("LICENSE-MIT", b"MIT original\r\nCopyright A\r\n"),
            ("LICENSE-APACHE", b"Apache original\n"),
            ("NOTICE", b"Original notice\n"),
            ("nested/COPYRIGHT.txt", b"Copyright B\n"),
            (
                "nested/Unicode-permission.txt",
                b"Declared original permission\n",
            ),
        ];
        for (path, bytes) in originals {
            fixture.write(&format!("package/{path}"), bytes);
        }
        fixture.write("package/src/copying.rs", b"this is source code\n");
        assert_eq!(
            fixture
                .collect(
                    Some("(MIT OR Apache-2.0) AND Unicode-3.0"),
                    Some("nested/Unicode-permission.txt")
                )
                .unwrap(),
            1
        );
        for (path, original) in originals {
            assert_eq!(
                fs::read(fixture.0.join("output/test-crate/1.2.3").join(path)).unwrap(),
                *original
            );
        }
        assert!(!fixture
            .0
            .join("output/test-crate/1.2.3/src/copying.rs")
            .exists());
        let manifest = fs::read_to_string(fixture.0.join("output/manifest.json")).unwrap();
        assert!(!manifest.contains(fixture.0.to_str().unwrap()));
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(
            manifest["packages"][0]["files"].as_array().unwrap().len(),
            originals.len()
        );
        for file in manifest["packages"][0]["files"].as_array().unwrap() {
            assert!(!Path::new(file["file"].as_str().unwrap()).is_absolute());
            assert_eq!(file["sha256"].as_str().unwrap().len(), 64);
        }
    }

    #[test]
    fn absent_declaration_original_or_declared_file_fails_before_writes() {
        let fixture = Fixture::new();
        fixture.write("package/Cargo.toml", b"[package]\n");
        assert!(fixture.collect(Some("MIT"), None).is_err());
        fixture.write("package/LICENSE", b"Original license\n");
        assert!(fixture.collect(None, None).is_err());
        assert!(fixture.collect(Some("MIT OR Proprietary"), None).is_err());
        assert!(fixture.collect(Some("MIT"), Some("absent.txt")).is_err());
        assert!(fixture.collect(Some("MIT"), Some("../LICENSE")).is_err());
        assert!(!fixture.0.join("output").exists());
    }

    #[test]
    fn exact_upstream_pin_checks_version_revision_and_content() {
        let fixture = Fixture::new();
        let revision = "0123456789abcdef0123456789abcdef01234567";
        fixture.write(
            "package/.cargo_vcs_info.json",
            format!("{{\"git\":{{\"sha1\":\"{revision}\"}}}}").as_bytes(),
        );
        let original = b"Upstream original license\n";
        fixture.write("vendor/pinned-LICENSE.md", original);
        let mut sources = Sources {
            format_version: 1,
            packages: vec![PinnedPackage {
                crate_name: "test-crate".into(),
                version: "1.2.3".into(),
                license: "MIT".into(),
                reviewed_expression: None,
                revision: revision.into(),
                files: vec![PinnedFile {
                    vendored_file: "pinned-LICENSE.md".into(),
                    file_name: "LICENSE.md".into(),
                    source_url: format!("https://example.org/{revision}/LICENSE.md"),
                    sha256: digest(original),
                }],
            }],
        };
        let metadata = fixture.metadata(Some("MIT"), None);
        let output = fixture.0.join("output");
        let vendor = fixture.0.join("vendor");
        sources.packages[0].version = "1.2.4".into();
        assert!(collect_with_sources(&metadata, &output, &sources, &vendor).is_err());
        sources.packages[0].version = "1.2.3".into();
        sources.packages[0].revision = "ffffffffffffffffffffffffffffffffffffffff".into();
        assert!(collect_with_sources(&metadata, &output, &sources, &vendor).is_err());
        sources.packages[0].revision = revision.into();
        fixture.write("vendor/pinned-LICENSE.md", b"Tampered legal text\n");
        assert!(collect_with_sources(&metadata, &output, &sources, &vendor).is_err());
        fixture.write("vendor/pinned-LICENSE.md", original);
        assert_eq!(
            collect_with_sources(&metadata, &output, &sources, &vendor).unwrap(),
            1
        );
        assert_eq!(
            fs::read(output.join("test-crate/1.2.3/LICENSE.md")).unwrap(),
            original
        );
    }

    #[test]
    fn reviewed_legacy_expression_is_bound_to_exact_release_and_keeps_originals() {
        let fixture = Fixture::new();
        let revision = "0123456789abcdef0123456789abcdef01234567";
        fixture.write(
            "package/.cargo_vcs_info.json",
            format!("{{\"git\":{{\"sha1\":\"{revision}\"}}}}").as_bytes(),
        );
        let evidence = b"Original upstream declaration: at your option\n";
        fixture.write("vendor/evidence.md", evidence);
        let sources = Sources {
            format_version: 1,
            packages: vec![PinnedPackage {
                crate_name: "test-crate".into(),
                version: "1.2.3".into(),
                license: "MIT/Apache-2.0".into(),
                reviewed_expression: Some("MIT OR Apache-2.0".into()),
                revision: revision.into(),
                files: vec![PinnedFile {
                    vendored_file: "evidence.md".into(),
                    file_name: "UPSTREAM-DECLARATION.md".into(),
                    source_url: format!("https://example.org/{revision}/README.md"),
                    sha256: digest(evidence),
                }],
            }],
        };
        let output = fixture.0.join("output");
        let vendor = fixture.0.join("vendor");
        let metadata = fixture.metadata(Some("MIT/Apache-2.0"), None);
        // Evidence is not a replacement for this crate's actual license texts.
        assert!(collect_with_sources(&metadata, &output, &sources, &vendor).is_err());
        fixture.write("package/LICENSE-MIT", b"Original MIT\n");
        fixture.write("package/LICENSE-APACHE", b"Original Apache\n");
        assert!(collect_with_sources(
            &fixture.metadata(Some("MIT/Proprietary"), None),
            &output,
            &sources,
            &vendor
        )
        .is_err());
        assert_eq!(
            collect_with_sources(&metadata, &output, &sources, &vendor).unwrap(),
            1
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["packages"][0]["license"], "MIT/Apache-2.0");
        assert_eq!(
            manifest["packages"][0]["reviewed_expression"],
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            manifest["packages"][0]["files"].as_array().unwrap().len(),
            3
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_read_outside_the_registry_package() {
        let fixture = Fixture::new();
        fixture.write("outside.txt", b"Private material\n");
        fs::create_dir(fixture.0.join("package")).unwrap();
        std::os::unix::fs::symlink(
            fixture.0.join("outside.txt"),
            fixture.0.join("package/LICENSE"),
        )
        .unwrap();
        assert!(fixture.collect(Some("MIT"), None).is_err());
        assert!(!fixture.0.join("output").exists());
    }

    #[test]
    fn nonempty_output_is_preserved() {
        let fixture = Fixture::new();
        fixture.write("package/LICENSE", b"Original license\n");
        fixture.write("output/keep.txt", b"Existing user file\n");
        assert!(fixture.collect(Some("MIT"), None).is_err());
        assert_eq!(
            fs::read(fixture.0.join("output/keep.txt")).unwrap(),
            b"Existing user file\n"
        );
        assert!(!fixture.0.join("output/manifest.json").exists());
    }
}
