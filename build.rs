fn main() {
    println!("cargo::rerun-if-changed=.env");
    if let Ok(iter) = dotenvy::dotenv_iter() {
        for item in iter.flatten() {
            println!("cargo::rustc-env={}={}", item.0, item.1);
        }
    }
}
