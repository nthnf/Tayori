use anyhow::Result;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ActiveModelTrait, Set};
use std::path::PathBuf;
use uuid::Uuid;

use tayori::backend::db::{connect, init_vector_indexes};
use tayori::backend::entities::projects;
use tayori::backend::pages::project::ProjectPageModel;
use tayori::backend::search::smart_hybrid_search;
use tayori::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Connecting to in-memory SQLite...");
    let db = connect("sqlite::memory:").await?;

    println!("Running migrations...");
    Migrator::up(&db, None).await?;

    println!("Initializing vector indexes...");
    init_vector_indexes(&db).await?;

    let state = AppState::new(db.clone());
    let page_model = ProjectPageModel::new(state.clone());

    let project_id = Uuid::new_v4().to_string();
    projects::ActiveModel {
        id: Set(project_id.clone()),
        name: Set("Hybrid Search Test Project".to_string()),
        ..Default::default()
    }
    .insert(&db)
    .await?;

    let temp_dir = PathBuf::from("./temp");
    let mut files = vec![];
    if temp_dir.exists() {
        for entry in std::fs::read_dir(&temp_dir)? {
            let entry = entry?;
            if entry.path().is_file() {
                files.push(entry.path());
            }
        }
    }

    println!("\nFound {} files to index...", files.len());

    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let ext = file
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let bytes = std::fs::read(&file)?;

        println!("Ingesting {}...", name);
        let doc_id = page_model
            .create_ingesting_doc(project_id.clone(), name.clone())
            .await?;

        match page_model
            .process_and_ready_doc(doc_id.clone(), &bytes, &ext)
            .await
        {
            Ok(_) => println!("  -> Success!"),
            Err(e) => println!("  -> Failed: {}", e),
        }
    }

    let query = "How will artificial intelligence impact developers and their code?";
    println!("\n=== RUNNING HYBRID SEARCH ===");
    println!("Query: \"{}\"", query);

    let embedder = state
        .embedder
        .get_or_try_init(|| async { tayori::backend::models::embed::Embedder::new() })
        .await?;
    let query_vector = embedder.embed(vec![query.to_string()])?.pop().unwrap();

    let (max_cosine, max_bm25, results) =
        smart_hybrid_search(&db, query, query_vector, Some(5)).await?;

    println!("Max Cosine Similarity: {:.4}", max_cosine);
    println!("Max BM25 Score: {:.4}", max_bm25);

    if results.is_empty() {
        println!("No results found!");
    }

    for (i, res) in results.into_iter().enumerate() {
        println!("\n[Rank {}]", i + 1);
        println!("Source ID: {} ({})", res.id, res.source_type);
        println!("Content Snippet: {}", res.content);
    }

    Ok(())
}
