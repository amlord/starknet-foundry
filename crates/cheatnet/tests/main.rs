mod cheatcodes;
pub(crate) mod common;

// Build testing contracts before executing the tests
#[cfg(test)]
#[ctor::ctor]
fn init() {
    use camino::Utf8PathBuf;
    let contracts_path = Utf8PathBuf::from("tests").join("contracts");

    std::process::Command::new("scarb")
        .current_dir(contracts_path)
        .arg("build")
        .output()
        .unwrap();
}
