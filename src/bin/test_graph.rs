use tayori::backend::graph::SessionGraph;
use tayori::backend::models::install;
use tayori::backend::models::pos::PosModel;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let model_path = install::default_pos_model_path(None);
    let tokenizer_path = install::default_pos_tokenizer_path(None);

    println!("Checking POS model at {:?}", model_path);
    if !model_path.exists() {
        println!("Model not found! Downloading now...");
        install::install_pos(None).await?;
        println!("Download complete.");
    }

    let pos_model = PosModel::new(&model_path, &tokenizer_path)?;
    let mut graph = SessionGraph::new();

    let sentences = [
        "The new UI Design looks absolutely fantastic, but the React components are broken.",
        "Charlie is fixing the React components right now.",
        "I had a quick chat with Dave about the database migration.",
        "Dave said the database migration will affect the new UI Design.",
    ];

    println!("\n--- Processing Sentences ---");
    for sentence in sentences.iter() {
        println!("Input: \"{}\"", sentence);
        let chunk_id = uuid::Uuid::new_v4();

        match pos_model.extract_entities(sentence) {
            Ok(entities) => {
                if !entities.is_empty() {
                    if entities.len() == 1 {
                        let entity = &entities[0];
                        graph.add_node(&entity.text, &entity.category, chunk_id);
                    } else {
                        for i in 0..entities.len() - 1 {
                            let source = &entities[i];
                            let target = &entities[i + 1];
                            graph.add_edge(
                                &source.text,
                                &source.category,
                                &target.text,
                                &target.category,
                                chunk_id,
                            );
                        }
                    }
                }
            }
            Err(e) => eprintln!("Error processing sentence: {}", e),
        }
    }

    println!("\n--- Graph Storage State ---");
    println!("Nodes in Graph:");
    for &idx in graph.node_map.values() {
        let node = &graph.graph[idx];
        println!("  - [{}] (Type: {})", node.name, node.category);
    }

    println!("Edges (Co-occurrences):");
    for edge_idx in graph.graph.edge_indices() {
        if let Some((source_idx, target_idx)) = graph.graph.edge_endpoints(edge_idx) {
            let source = &graph.graph[source_idx];
            let target = &graph.graph[target_idx];
            let edge = &graph.graph[edge_idx];
            println!(
                "  - [{}] <--> [{}] (weight: {})",
                source.name, target.name, edge.weight
            );
        }
    }

    println!("\n--- Querying Graph ---");
    let query1 = "Tell me about React components";
    println!("Query: \"{}\"", query1);
    let matches = graph.find_matches(query1);
    for m in matches {
        println!("  -> {}", m.fact);
    }

    println!(" ");
    let query2 = "What is Dave working on?";
    println!("Query: \"{}\"", query2);
    let matches = graph.find_matches(query2);
    for m in matches {
        println!("  -> {}", m.fact);
    }

    Ok(())
}
