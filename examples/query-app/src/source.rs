//! Where query results come from. `MemorySource` is a stand-in until a real
//! backend (Neo4j / NebulaGraph / ...) implements [`GraphSource`].

use std::collections::{HashMap, HashSet, VecDeque};

use gpui_graph::{Edge, GraphData, Node};

pub trait GraphSource {
    /// Run a query and return the matching subgraph.
    fn query(&self, query: &str) -> Result<GraphData, String>;
    /// One-hop neighborhood of a node, used when the user expands it.
    fn expand(&self, id: &str) -> GraphData;
}

/// An in-memory property graph with a tiny query syntax:
///
/// - `*` — everything
/// - `<keyword> [depth N]` — nodes whose label contains `keyword`, plus N hops (default 1)
/// - `random N` — a random scale-free graph with N nodes, for stress testing
pub struct MemorySource {
    data: GraphData,
    adjacency: HashMap<String, Vec<usize>>,
}

impl MemorySource {
    pub fn new(data: GraphData) -> Self {
        let mut adjacency: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in data.edges.iter().enumerate() {
            adjacency.entry(e.source.to_string()).or_default().push(i);
            adjacency.entry(e.target.to_string()).or_default().push(i);
        }
        Self { data, adjacency }
    }

    fn subgraph(&self, seeds: Vec<String>, depth: usize) -> GraphData {
        let mut seen: HashSet<String> = seeds.iter().cloned().collect();
        let mut edges: HashSet<usize> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = seeds.into_iter().map(|s| (s, 0)).collect();
        while let Some((id, d)) = queue.pop_front() {
            if d >= depth {
                continue;
            }
            for &ei in self.adjacency.get(&id).into_iter().flatten() {
                edges.insert(ei);
                let e = &self.data.edges[ei];
                let other = if e.source.as_ref() == id { &e.target } else { &e.source };
                if seen.insert(other.to_string()) {
                    queue.push_back((other.to_string(), d + 1));
                }
            }
        }
        GraphData {
            nodes: self.data.nodes.iter().filter(|n| seen.contains(n.id.as_ref())).cloned().collect(),
            edges: self
                .data
                .edges
                .iter()
                .enumerate()
                .filter(|(i, e)| {
                    edges.contains(i) || (seen.contains(e.source.as_ref()) && seen.contains(e.target.as_ref()))
                })
                .map(|(_, e)| e.clone())
                .collect(),
        }
    }
}

impl GraphSource for MemorySource {
    fn query(&self, query: &str) -> Result<GraphData, String> {
        let query = query.trim();
        if query.is_empty() || query == "*" {
            return Ok(self.data.clone());
        }
        let mut words: Vec<&str> = query.split_whitespace().collect();
        if words.first() == Some(&"random") {
            let n = words.get(1).and_then(|n| n.parse().ok()).unwrap_or(500);
            return Ok(random_graph(n.clamp(2, 20_000)));
        }
        let mut depth = 1;
        if let [.., "depth", n] = words.as_slice() {
            depth = n.parse().map_err(|_| format!("invalid depth: {n}"))?;
            words.truncate(words.len() - 2);
        }
        let keyword = words.join(" ").to_lowercase();
        let seeds: Vec<String> = self
            .data
            .nodes
            .iter()
            .filter(|n| n.label.to_lowercase().contains(&keyword) || n.id.to_lowercase() == keyword)
            .map(|n| n.id.to_string())
            .collect();
        if seeds.is_empty() {
            return Err(format!("no node matches \"{keyword}\""));
        }
        Ok(self.subgraph(seeds, depth))
    }

    fn expand(&self, id: &str) -> GraphData {
        self.subgraph(vec![id.to_string()], 1)
    }
}

/// A small movie graph in the spirit of Neo4j's `:play movies`.
///
/// `community` stands in for a value the database computed (e.g. Louvain via
/// Neo4j GDS `gds.louvain.write`); the UI only reads it.
pub fn sample_movies() -> GraphData {
    let community_of_movie = |title: &str| match title {
        t if t.starts_with("The Matrix") || t == "Cloud Atlas" || t == "V for Vendetta" => "Wachowski",
        "Inception" | "The Dark Knight" | "Interstellar" => "Nolan",
        "John Wick" | "Speed" | "The Devil's Advocate" => "Keanu",
        _ => "Hanks & Ryan",
    };
    let community_of_genre = |genre: &str| match genre {
        "Sci-Fi" => "Wachowski",
        "Action" => "Keanu",
        "Thriller" => "Nolan",
        _ => "Hanks & Ryan",
    };
    let movies: &[(&str, &str, &[&str])] = &[
        ("The Matrix", "1999", &["Sci-Fi", "Action"]),
        ("The Matrix Reloaded", "2003", &["Sci-Fi", "Action"]),
        ("The Matrix Revolutions", "2003", &["Sci-Fi", "Action"]),
        ("John Wick", "2014", &["Action", "Thriller"]),
        ("Speed", "1994", &["Action", "Thriller"]),
        ("Cloud Atlas", "2012", &["Sci-Fi", "Drama"]),
        ("V for Vendetta", "2005", &["Action", "Drama"]),
        ("Inception", "2010", &["Sci-Fi", "Thriller"]),
        ("The Dark Knight", "2008", &["Action", "Drama"]),
        ("Interstellar", "2014", &["Sci-Fi", "Drama"]),
        ("Forrest Gump", "1994", &["Drama", "Romance"]),
        ("Cast Away", "2000", &["Drama"]),
        ("Sleepless in Seattle", "1993", &["Romance", "Comedy"]),
        ("You've Got Mail", "1998", &["Romance", "Comedy"]),
        ("Apollo 13", "1995", &["Drama"]),
        ("The Devil's Advocate", "1997", &["Thriller", "Drama"]),
    ];
    let acted: &[(&str, &[&str])] = &[
        ("Keanu Reeves", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions", "John Wick", "Speed", "The Devil's Advocate"]),
        ("Carrie-Anne Moss", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions"]),
        ("Laurence Fishburne", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions", "John Wick"]),
        ("Hugo Weaving", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions", "Cloud Atlas", "V for Vendetta"]),
        ("Natalie Portman", &["V for Vendetta"]),
        ("Tom Hanks", &["Cloud Atlas", "Forrest Gump", "Cast Away", "Sleepless in Seattle", "You've Got Mail", "Apollo 13"]),
        ("Halle Berry", &["Cloud Atlas"]),
        ("Meg Ryan", &["Sleepless in Seattle", "You've Got Mail"]),
        ("Sandra Bullock", &["Speed"]),
        ("Leonardo DiCaprio", &["Inception"]),
        ("Joseph Gordon-Levitt", &["Inception"]),
        ("Tom Hardy", &["Inception", "The Dark Knight"]),
        ("Michael Caine", &["Inception", "The Dark Knight", "Interstellar"]),
        ("Christian Bale", &["The Dark Knight"]),
        ("Heath Ledger", &["The Dark Knight"]),
        ("Matthew McConaughey", &["Interstellar"]),
        ("Anne Hathaway", &["Interstellar"]),
        ("Kevin Bacon", &["Apollo 13"]),
        ("Al Pacino", &["The Devil's Advocate"]),
        ("Charlize Theron", &["The Devil's Advocate"]),
    ];
    let directed: &[(&str, &[&str])] = &[
        ("Lana Wachowski", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions", "Cloud Atlas"]),
        ("Lilly Wachowski", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions", "Cloud Atlas"]),
        ("Chad Stahelski", &["John Wick"]),
        ("James McTeigue", &["V for Vendetta"]),
        ("Christopher Nolan", &["Inception", "The Dark Knight", "Interstellar"]),
        ("Robert Zemeckis", &["Forrest Gump", "Cast Away"]),
        ("Nora Ephron", &["Sleepless in Seattle", "You've Got Mail"]),
        ("Ron Howard", &["Apollo 13"]),
        ("Taylor Hackford", &["The Devil's Advocate"]),
    ];
    let produced: &[(&str, &[&str])] = &[
        ("Joel Silver", &["The Matrix", "The Matrix Reloaded", "The Matrix Revolutions", "V for Vendetta"]),
        ("Emma Thomas", &["Inception", "The Dark Knight", "Interstellar"]),
    ];

    let mut data = GraphData::default();
    let id = |kind: &str, name: &str| format!("{kind}:{name}");
    let mut genres = Vec::new();
    for (title, year, gs) in movies {
        let community = community_of_movie(title);
        data.nodes.push(
            Node::new(id("movie", title), *title, "Movie")
                .property("released", *year)
                .property("community", community)
                .group(community),
        );
        for g in *gs {
            if !genres.contains(g) {
                genres.push(*g);
                let community = community_of_genre(g);
                data.nodes.push(
                    Node::new(id("genre", g), *g, "Genre").property("community", community).group(community),
                );
            }
            data.edges.push(Edge::new(id("movie", title), id("genre", g), "IN_GENRE"));
        }
    }
    let mut people: Vec<&str> = Vec::new();
    for (rel, list) in [("ACTED_IN", acted), ("DIRECTED", directed), ("PRODUCED", produced)] {
        for (person, titles) in list {
            if !people.contains(person) {
                people.push(person);
                let community = community_of_movie(titles[0]);
                data.nodes.push(
                    Node::new(id("person", person), *person, "Person")
                        .property("name", *person)
                        .property("community", community)
                        .group(community),
                );
            }
            for t in *titles {
                data.edges.push(Edge::new(id("person", person), id("movie", t), rel));
            }
        }
    }
    // A couple of person-to-person edges, including a parallel pair.
    data.edges.push(Edge::new(id("person", "Lana Wachowski"), id("person", "Lilly Wachowski"), "SIBLING"));
    data.edges.push(Edge::new(id("person", "Lilly Wachowski"), id("person", "Lana Wachowski"), "SIBLING"));
    data.edges.push(Edge::new(id("person", "Keanu Reeves"), id("person", "Laurence Fishburne"), "FOLLOWS"));
    data
}

/// Preferential attachment inside planted communities, with a few bridges,
/// so community hulls and collapsing have something to show.
fn random_graph(n: usize) -> GraphData {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let kinds = ["Account", "Device", "IP", "Merchant", "Card"];
    let communities = (n / 120).clamp(3, 40);
    let mut data = GraphData::default();
    let mut targets: Vec<Vec<usize>> = vec![Vec::new(); communities];
    let mut all: Vec<usize> = Vec::new();
    for i in 0..n {
        let kind = kinds[(rand() % kinds.len() as u64) as usize];
        let c = (rand() % communities as u64) as usize;
        let community = format!("C{c}");
        data.nodes.push(
            Node::new(format!("n{i}"), format!("{kind} {i}"), kind)
                .property("community", community.clone())
                .group(community),
        );
        let links = 1 + (rand() % 2) as usize;
        for _ in 0..links {
            // Mostly inside the community; ~5% bridge to anywhere.
            let pool = if rand() % 20 == 0 || targets[c].is_empty() { &all } else { &targets[c] };
            if pool.is_empty() {
                break;
            }
            let t = pool[(rand() % pool.len() as u64) as usize];
            if t != i {
                data.edges.push(Edge::new(format!("n{i}"), format!("n{t}"), "LINKS"));
                targets[c].push(t);
            }
        }
        targets[c].push(i);
        all.push(i);
    }
    data
}
