use std::time::{Duration, Instant};

use eam_retrieval::{cosine_similarity_bps, embed_text};

const FIXTURE: &str = include_str!("fixtures/g07-retrieval-benchmark.tsv");
type FixtureRows = Vec<(&'static str, &'static str)>;

#[test]
fn fixed_vector_benchmark_recalls_every_expected_document_in_top_three() {
    let (documents, queries) = load_fixture();
    let embedded = documents
        .iter()
        .map(|(id, text)| (*id, embed_text(text)))
        .collect::<Vec<_>>();

    let mut covered = 0;
    for (expected, text) in queries {
        let query = embed_text(text);
        let mut ranked = embedded
            .iter()
            .map(|(id, vector)| (*id, cosine_similarity_bps(&query, vector)))
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(id, score)| (std::cmp::Reverse(*score), *id));
        if ranked.iter().take(3).any(|(id, _)| *id == expected) {
            covered += 1;
        }
    }
    assert_eq!(covered, 3, "G07 vector recall coverage must remain 3/3");
}

#[test]
fn exact_scan_stays_within_the_frozen_debug_ceiling() {
    let query = embed_text("coordinating Aurora launch review");
    let corpus = (0..4_096)
        .map(|ordinal| {
            embed_text(&format!(
                "synthetic document {ordinal} stable benchmark text"
            ))
        })
        .collect::<Vec<_>>();
    let started = Instant::now();
    let best = corpus
        .iter()
        .map(|candidate| cosine_similarity_bps(&query, candidate))
        .max();

    assert!(best.is_some());
    assert!(
        started.elapsed() <= Duration::from_secs(5),
        "4,096-vector exact scan exceeded the G07 debug ceiling"
    );
}

fn load_fixture() -> (FixtureRows, FixtureRows) {
    let mut documents = Vec::new();
    let mut queries = Vec::new();
    for line in FIXTURE.lines().filter(|line| !line.starts_with('#')) {
        let mut fields = line.splitn(3, '\t');
        let kind = fields.next().unwrap();
        let id = fields.next().unwrap();
        let text = fields.next().unwrap();
        match kind {
            "document" => documents.push((id, text)),
            "query" => queries.push((id, text)),
            _ => panic!("unknown G07 fixture row kind: {kind}"),
        }
    }
    (documents, queries)
}
