#[path = "src/asset_policy.rs"]
mod asset_policy;

fn main() {
    println!("cargo:rerun-if-changed=../frontend/dist");
    println!("cargo:rerun-if-changed=src/asset_policy.rs");
    let index =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontend/dist/index.html");
    let issues = match std::fs::read_to_string(&index) {
        Ok(html) => asset_policy::violations(&html),
        Err(error) => vec![format!(
            "frontend not built — run npm run build in frontend/ ({}: {error})",
            index.display()
        )],
    };
    if !issues.is_empty() {
        let message = format!("desktop frontend policy: {}", issues.join("; "));
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!("{message}");
        }
        println!("cargo:warning={message}");
    }
    tauri_build::build();
}
