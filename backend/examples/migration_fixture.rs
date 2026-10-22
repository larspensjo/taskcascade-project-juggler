use taskcascade_backend::test_support::{build_fixture, FixtureKind};

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let kind = match args.get(1).and_then(|arg| arg.to_str()) {
        Some("plain") if args.len() == 2 => FixtureKind::Plain,
        Some("wal") if args.len() == 2 => FixtureKind::Wal,
        _ => {
            eprintln!("Usage: migration_fixture <empty dir> plain|wal");
            std::process::exit(1);
        }
    };
    let dir = std::path::Path::new(&args[0]);
    let (expected, _wal_connection) = build_fixture(dir, kind).await;
    std::fs::write(
        dir.join("expected.json"),
        serde_json::to_vec(&expected).unwrap(),
    )
    .unwrap();
    // Exit while the connection is held so SQLite cannot checkpoint the WAL.
    std::process::exit(0);
}
