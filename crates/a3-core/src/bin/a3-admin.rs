//! The admin migration tool (section 5). Users have no schema rights, so schema changes and the first
//! admin account go through this, connected as the schema owner.
//!
//!   a3-admin migrate
//!   a3-admin bootstrap <login> "<Display name>" <email>     (password from A3_ADMIN_PASSWORD)
//!
//! The owner connection comes from the DATABASE_URL environment variable.

use a3_core::scram;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let url = std::env::var("DATABASE_URL").map_err(|_| "Set DATABASE_URL to the schema owner connection.".to_string())?;
    let pool = PgPoolOptions::new().max_connections(1).connect(&url).await.map_err(|e| format!("Could not connect: {e}"))?;
    match args.get(1).map(String::as_str) {
        Some("migrate") => {
            sqlx::migrate!("./migrations").run(&pool).await.map_err(|e| format!("Migration failed: {e}"))?;
            println!("Schema is at version {}.", a3_core::SCHEMA_VERSION);
            Ok(())
        }
        Some("bootstrap") if args.len() >= 5 => {
            let password = std::env::var("A3_ADMIN_PASSWORD").map_err(|_| "Set A3_ADMIN_PASSWORD.".to_string())?;
            let verifier = scram::verifier(&password);
            let id = sqlx::query_scalar!("SELECT admin_create_user($1, $2, $3, $4, true)", args[2], args[3], args[4], verifier)
                .fetch_one(&pool)
                .await
                .map_err(|e| a3_core::CoreError::from(e).message)?;
            println!("Created admin {} ({}).", args[2], id.map(|i| i.to_string()).unwrap_or_default());
            Ok(())
        }
        _ => Err("Usage: a3-admin migrate | a3-admin bootstrap <login> \"<Display name>\" <email>".to_string()),
    }
}
