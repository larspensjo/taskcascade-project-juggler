use taskcascade_backend::app::summarize_database;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("Usage: database_summary <path>");
        std::process::exit(1);
    }
    match summarize_database(std::path::Path::new(&args[0])).await {
        Ok(summary) => println!("{}", serde_json::to_string(&summary).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
