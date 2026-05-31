use anyhow::Result;
use futures::StreamExt;
use migration::{Migrator, MigratorTrait};
use sea_orm::{ActiveModelTrait, Set};
use std::io::{self, Write};
use std::path::PathBuf;
use uuid::Uuid;

use tayori::backend::db::{connect, init_vector_indexes};
use tayori::backend::entities::projects;
use tayori::backend::models::llm::{LlmModel, read_api_key};
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

    let question = "How will artificial intelligence impact developers and their code?";
    println!("\n=== RUNNING HYBRID SEARCH ===");
    println!("Question: \"{}\"", question);

    let embedder = state
        .embedder
        .get_or_try_init(|| async { tayori::backend::models::embed::Embedder::new() })
        .await?;
    let query_vector = embedder.embed(vec![question.to_string()])?.pop().unwrap();

    let (max_cosine, max_bm25, candidates) =
        smart_hybrid_search(&db, question, query_vector, Some(5)).await?;

    println!("Max Cosine Similarity: {:.4}", max_cosine);
    println!("Max BM25 Score: {:.4}", max_bm25);

    println!("Reading API key from keyring...");
    let api_key = match read_api_key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("Failed to read API key from keyring: {}", e);
            return Ok(());
        }
    };

    let model = LlmModel::new("gpt-4o-mini".to_string(), api_key);

    println!("\nSending request to the LLM backend (streaming)...");
    match model
        .ask_stream(question, candidates, max_cosine, max_bm25)
        .await
    {
        Ok(mut stream) => {
            println!("\n=== LLM Response ===");
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(text) => {
                        print!("{}", text);
                        io::stdout().flush().unwrap();
                    }
                    Err(e) => {
                        eprintln!("\n[Stream Error: {}]", e);
                        break;
                    }
                }
            }
            println!("\n====================");
        }
        Err(e) => {
            eprintln!("Failed to get stream from LLM: {}", e);
        }
    }

    Ok(())
}
