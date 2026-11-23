// Номер сборки = unix-время сборки: приложение ставит ядро только если оно новее
// установленного, а `zhord start` перезапускает работающий демон старшей сборки.
fn main() {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=ZHOR_BUILD={t}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
