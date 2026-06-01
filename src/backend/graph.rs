use petgraph::graph::{NodeIndex, UnGraph};
use rust_stemmers::{Algorithm, Stemmer};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct NodeWeight {
    pub name: String,
    pub category: String,
    pub chunk_ids: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeWeight {
    pub weight: u32,
    pub chunk_ids: Vec<uuid::Uuid>,
}

#[derive(Clone, Debug)]
pub struct GraphMatch {
    pub fact: String,
    pub chunk_ids: Vec<uuid::Uuid>,
}

pub struct SessionGraph {
    pub graph: UnGraph<NodeWeight, EdgeWeight>,
    pub node_map: HashMap<String, NodeIndex>,
}

impl Default for SessionGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionGraph {
    pub fn new() -> Self {
        Self {
            graph: UnGraph::new_undirected(),
            node_map: HashMap::new(),
        }
    }

    /// Clears all nodes and edges from the graph.
    pub fn clear(&mut self) {
        self.graph.clear();
        self.node_map.clear();
    }

    /// Adds a node if it does not exist, and returns its index. Updates chunk_ids metadata.
    pub fn add_node(&mut self, name: &str, category: &str, chunk_id: uuid::Uuid) -> NodeIndex {
        let clean_name = name.trim();
        let key = clean_name.to_lowercase();

        if let Some(&idx) = self.node_map.get(&key) {
            let node = &mut self.graph[idx];
            if !node.chunk_ids.contains(&chunk_id) {
                node.chunk_ids.push(chunk_id);
            }
            return idx;
        }

        let idx = self.graph.add_node(NodeWeight {
            name: clean_name.to_string(),
            category: category.to_string(),
            chunk_ids: vec![chunk_id],
        });
        self.node_map.insert(key, idx);
        idx
    }

    /// Adds an undirected co-occurrence edge between two nodes.
    pub fn add_edge(
        &mut self,
        source_name: &str,
        source_cat: &str,
        target_name: &str,
        target_cat: &str,
        chunk_id: uuid::Uuid,
    ) {
        let source_idx = self.add_node(source_name, source_cat, chunk_id);
        let target_idx = self.add_node(target_name, target_cat, chunk_id);

        if source_idx == target_idx {
            return; // No self loops
        }

        if let Some(edge_idx) = self.graph.find_edge(source_idx, target_idx) {
            let edge = &mut self.graph[edge_idx];
            edge.weight += 1;
            if !edge.chunk_ids.contains(&chunk_id) {
                edge.chunk_ids.push(chunk_id);
            }
        } else {
            self.graph.add_edge(
                source_idx,
                target_idx,
                EdgeWeight {
                    weight: 1,
                    chunk_ids: vec![chunk_id],
                },
            );
        }
    }

    /// Searches the graph using query terms and their stemmed versions (lemmatization),
    /// returning neighboring co-occurrence facts sorted by association strength.
    pub fn find_matches(&self, query: &str) -> Vec<GraphMatch> {
        let stemmer = Stemmer::create(Algorithm::English);

        let query_terms: HashSet<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| c.is_ascii_punctuation())
                    .to_string()
            })
            .filter(|w| !w.is_empty())
            .collect();

        let query_stems: HashSet<String> = query_terms
            .iter()
            .map(|w| stemmer.stem(w).to_string())
            .collect();

        let mut matched_indices = HashSet::new();

        for (node_name, &idx) in &self.node_map {
            let node = &self.graph[idx];
            let mut node_terms: Vec<String> = node_name
                .split_whitespace()
                .map(|w| {
                    w.trim_matches(|c: char| c.is_ascii_punctuation())
                        .to_string()
                })
                .filter(|w| !w.is_empty())
                .collect();

            // Include category as matching term
            node_terms.push(node.category.to_lowercase());

            for term in &node_terms {
                let term_stem = stemmer.stem(term).to_string();
                if query_terms.contains(term) || query_stems.contains(&term_stem) {
                    matched_indices.insert(idx);
                    break;
                }
            }
        }

        let mut unique_facts = Vec::new();
        let mut seen = HashSet::new();

        // 1. Direct Node Metadata Matches (The Inverted Index Approach)
        // If we matched multiple nodes, try to find intersection of their chunks
        if matched_indices.len() > 1 {
            let mut all_chunks = Vec::new();
            let mut matched_names = Vec::new();

            for &idx in &matched_indices {
                let node = &self.graph[idx];
                all_chunks.push(node.chunk_ids.clone());
                matched_names.push(node.name.clone());
            }

            // Find intersection
            if let Some(first_chunks) = all_chunks.first() {
                let mut intersection = first_chunks.clone();
                for chunks in all_chunks.iter().skip(1) {
                    intersection.retain(|c| chunks.contains(c));
                }

                if !intersection.is_empty() {
                    let fact = format!(
                        "Entities {} were mentioned together in these exact moments",
                        matched_names.join(", ")
                    );
                    if seen.insert(fact.clone()) {
                        unique_facts.push(GraphMatch {
                            fact,
                            chunk_ids: intersection,
                        });
                    }
                }
            }
        }

        // 2. Add individual node metadata (Fallback to individual mentions)
        for &idx in &matched_indices {
            let node = &self.graph[idx];
            let fact = format!("Entity [{}] ({}) was mentioned", node.name, node.category);
            if seen.insert(fact.clone()) {
                unique_facts.push(GraphMatch {
                    fact,
                    chunk_ids: node.chunk_ids.clone(),
                });
            }
        }

        // 3. Keep the Edge logic as deeper structural fallback
        let mut raw_facts = Vec::new();
        for &idx in &matched_indices {
            let mut neighbors = self.graph.neighbors(idx).detach();
            while let Some((edge_idx, neighbor_idx)) = neighbors.next(&self.graph) {
                let source = &self.graph[idx];
                let neighbor = &self.graph[neighbor_idx];
                let edge = &self.graph[edge_idx];

                raw_facts.push((
                    edge.weight,
                    format!(
                        "[{}] and [{}] were discussed together (association strength: {})",
                        source.name, neighbor.name, edge.weight
                    ),
                    edge.chunk_ids.clone(),
                ));
            }
        }

        raw_facts.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        for (_, fact, chunk_ids) in raw_facts {
            if seen.insert(fact.clone()) {
                unique_facts.push(GraphMatch { fact, chunk_ids });
            }
        }

        unique_facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_construction_and_retrieval() {
        let mut sg = SessionGraph::new();
        let chunk_id = uuid::Uuid::new_v4();
        sg.add_edge("John", "PER", "Pipewire config", "TASK", chunk_id);
        sg.add_edge("John", "PER", "Pipewire config", "TASK", chunk_id); // Should increment weight to 2
        sg.add_edge(
            "Pipewire config",
            "TASK",
            "Seattle release",
            "MILESTONE",
            chunk_id,
        );

        // Test basic matching
        let matches = sg.find_matches("What is John doing?");
        assert_eq!(matches.len(), 2); // 1 node match, 1 edge match
        assert!(
            matches[1]
                .fact
                .contains("[John] and [Pipewire config] were discussed together")
        );
        assert_eq!(matches[0].chunk_ids[0], chunk_id);

        // Test lemmatization / stemming matching
        let matches_stemmed = sg.find_matches("Show me the Pipewire task.");
        assert_eq!(matches_stemmed.len(), 3);
    }
}
