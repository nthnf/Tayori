use petgraph::graph::{NodeIndex, UnGraph};
use rust_stemmers::{Algorithm, Stemmer};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct NodeWeight {
    pub name: String,
    pub category: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeWeight {
    pub weight: u32,
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

    /// Adds a node if it does not exist, and returns its index.
    pub fn add_node(&mut self, name: &str, category: &str) -> NodeIndex {
        let clean_name = name.trim();
        let key = clean_name.to_lowercase();
        if let Some(&idx) = self.node_map.get(&key) {
            return idx;
        }
        let idx = self.graph.add_node(NodeWeight {
            name: clean_name.to_string(),
            category: category.to_string(),
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
        let source_idx = self.add_node(source_name, source_cat);
        let target_idx = self.add_node(target_name, target_cat);

        if source_idx == target_idx {
            return; // No self loops
        }

        if let Some(edge_idx) = self.graph.find_edge(source_idx, target_idx) {
            let edge = &mut self.graph[edge_idx];
            edge.weight += 1;
            if !edge.chunk_ids.contains(&chunk_id) {
                edge.chunk_ids.push(chunk_id);
                if edge.chunk_ids.len() > 5 {
                    edge.chunk_ids.remove(0); // keep bounded to last 5 chunks
                }
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
    pub fn find_matches(&self, query: &str) -> Vec<String> {
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

        let mut raw_facts = Vec::new();
        for &idx in &matched_indices {
            // Find all neighbors for the matched node
            let mut neighbors = self.graph.neighbors(idx).detach();
            while let Some((edge_idx, neighbor_idx)) = neighbors.next(&self.graph) {
                let source = &self.graph[idx];
                let neighbor = &self.graph[neighbor_idx];
                let edge = &self.graph[edge_idx];

                // Store weight for sorting later
                raw_facts.push((
                    edge.weight,
                    format!(
                        "[{}] and [{}] were discussed together (association strength: {}, chunks: {:?})",
                        source.name, neighbor.name, edge.weight, edge.chunk_ids
                    ),
                ));
            }
        }

        // Sort by weight descending, then by string alphabetically
        raw_facts.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        raw_facts.dedup();

        raw_facts.into_iter().map(|(_, fact)| fact).collect()
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
        assert_eq!(matches.len(), 1);
        assert!(matches[0].contains("[John] and [Pipewire config] were discussed together"));
        assert!(matches[0].contains("strength: 2"));

        // Test lemmatization / stemming matching
        let matches_stemmed = sg.find_matches("Show me the Pipewire task.");
        assert_eq!(matches_stemmed.len(), 2);
    }
}
