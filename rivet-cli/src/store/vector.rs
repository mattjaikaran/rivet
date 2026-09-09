//! Blueprint vector index (phase 2.2, pillar 04).
//!
//! Parsed blueprints become searchable chunks — one per route (`GET
//! /orders`, its DTO names, and story IDs) — embedded as fixed-size float
//! vectors in a local LanceDB table under `.rivet/lancedb`. A symptom
//! string embeds the same way and the nearest chunk tells `rivet explain`
//! which route to blame.
//!
//! LanceDB is async, so every public function here is `async`; command
//! modules run it inside a small tokio runtime. All access funnels through
//! this module so the SQLite store never depends on LanceDB's arrow stack.
//!
//! Embeddings are deterministic feature hashing over whitespace tokens:
//! no model, no network, stable across machines and runs.

use std::sync::Arc;

// arrow types arrive via lancedb's re-export (lancedb owns the arrow
// version), so this module adds no direct arrow dependency and can never
// drift out of sync with the version lancedb links.
use futures::TryStreamExt;
use lancedb::Connection;
use lancedb::arrow::arrow_array::types::Float32Type;
use lancedb::arrow::arrow_array::{FixedSizeListArray, Float32Array, RecordBatch, StringArray};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use lancedb::query::{ExecutableQuery, QueryBase};

/// Vector width of every embedded chunk.
const DIM: usize = 256;

/// How many nearest neighbors `search` returns.
const TOP_K: usize = 5;

/// One indexed blueprint chunk with its vector and metadata.
struct Chunk {
    /// Searchable text: method, path, handler, story IDs.
    text: String,
    /// Deterministic embedding of `text`.
    vector: Vec<f32>,
}

/// The parts of a route that make a useful searchable chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSummary {
    /// HTTP method, for example `GET`.
    pub method: String,
    /// Route path, for example `/orders`.
    pub path: String,
    /// Rust-safe handler name.
    pub handler: String,
    /// User-story IDs attached to the route.
    pub stories: Vec<String>,
}

/// Build a deterministic fixed-size embedding from character trigrams.
///
/// The text is lowercased, every three consecutive characters become a
/// feature (plus the whole words, so short tokens still count), and each
/// feature hashes into the vector with a signed weight. Shared substrings —
/// the way a symptom like "orders" overlaps "GET /orders" — land in the
/// same buckets, so cosine similarity between a chunk and a symptom is a
/// real measure of overlap. The vector is normalized after accumulation.
fn embed(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    let mut features: Vec<String> = Vec::new();
    for word in text.to_lowercase().split_whitespace() {
        features.push(word.to_string());
        let chars: Vec<char> = word.chars().collect();
        if chars.len() >= 3 {
            for window in chars.windows(3) {
                features.push(window.iter().collect());
            }
        }
    }
    for feature in &features {
        // Two independent hash lanes (Fowler-Noll-Vo style) pick the bucket
        // and the sign, keeping collisions from cancelling out.
        let mut bucket: u64 = 0xcbf2_9ce4_8422_2325;
        let mut sign: u64 = 0x8422_2325_cbf2_9ce4;
        for b in feature.bytes() {
            bucket ^= u64::from(b);
            bucket = bucket.wrapping_mul(0x0000_0100_0000_01b3);
            sign ^= u64::from(b);
            sign = sign.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let idx = (bucket % DIM as u64) as usize;
        let weight = if sign & 1 == 0 { 1.0 } else { -1.0 };
        v[idx] += weight;
    }
    let norm = v.iter().fold(0.0f32, |acc, x| acc + x * x).sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Render one route as its searchable chunk text.
fn route_text(route: &RouteSummary) -> String {
    format!(
        "{} {} handler {} stories {}",
        route.method,
        route.path,
        route.handler,
        route.stories.join(" ")
    )
}

/// Open the vector index under `.rivet/lancedb` in `project_dir`.
async fn open_db(project_dir: &std::path::Path) -> Result<Connection, String> {
    let dir = project_dir.join(".rivet");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create .rivet: {e}"))?;
    let uri = dir.join("lancedb");
    let uri = uri
        .to_str()
        .ok_or_else(|| "store path is not valid UTF-8".to_string())?;
    lancedb::connect(uri)
        .execute()
        .await
        .map_err(|e| format!("cannot open vector store: {e}"))
}

/// Make an app label safe as a LanceDB table name.
fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Index one blueprint's chunks, replacing any previous index of the same
/// app path. Re-indexing is idempotent: the table is dropped and rebuilt so
/// stale chunks never survive a rebuild.
pub async fn index_blueprint(
    project_dir: &std::path::Path,
    app_label: &str,
    routes: &[RouteSummary],
) -> Result<(), String> {
    let db = open_db(project_dir).await?;
    let table_name = format!("blueprint_{}", sanitize(app_label));

    // Drop any stale table, then rebuild from the given chunks.
    let _ = db.drop_table(&table_name, &[]).await;
    let chunks: Vec<Chunk> = routes
        .iter()
        .map(|r| {
            let text = route_text(r);
            let vector = embed(&text);
            Chunk { text, vector }
        })
        .collect();

    if chunks.is_empty() {
        return Ok(());
    }

    let batch = to_batch(&chunks).map_err(|e| format!("cannot build vector batch: {e}"))?;
    db.create_table(&table_name, batch)
        .execute()
        .await
        .map_err(|e| format!("cannot create vector table: {e}"))?;
    Ok(())
}

/// Search the indexed chunks for `symptom` and return the closest chunk
/// texts with their distances, best match first.
pub async fn search(
    project_dir: &std::path::Path,
    app_label: &str,
    symptom: &str,
) -> Result<Vec<(String, f32)>, String> {
    let db = open_db(project_dir).await?;
    let table_name = format!("blueprint_{}", sanitize(app_label));
    let table = match db.open_table(&table_name).execute().await {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()), // nothing indexed yet
    };

    let query_vec = embed(symptom);
    let batches: Vec<RecordBatch> = table
        .query()
        .limit(TOP_K)
        .nearest_to(query_vec.as_slice())
        .map_err(|e| format!("cannot build vector query: {e}"))?
        .execute()
        .await
        .map_err(|e| format!("vector query failed: {e}"))?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|e| format!("vector query stream failed: {e}"))?;

    let mut results = Vec::new();
    for batch in batches {
        let text = batch["text"].as_any().downcast_ref::<StringArray>();
        let distance = batch["_distance"].as_any().downcast_ref::<Float32Array>();
        if let (Some(text), Some(distance)) = (text, distance) {
            for (t, d) in text.iter().zip(distance.iter()) {
                if let Some(text) = t {
                    results.push((text.to_string(), d.unwrap_or(f32::MAX)));
                }
            }
        }
    }
    results.sort_by(|a, b| a.1.total_cmp(&b.1));
    Ok(results)
}

/// Convert chunks to an arrow RecordBatch of `(text, vector)`.
fn to_batch(chunks: &[Chunk]) -> Result<RecordBatch, lancedb::arrow::arrow_schema::ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                DIM as i32,
            ),
            true,
        ),
    ]));
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let vectors: Vec<Option<Vec<Option<f32>>>> = chunks
        .iter()
        .map(|c| Some(c.vector.iter().map(|x| Some(*x)).collect()))
        .collect();
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(texts)),
            Arc::new(
                FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(vectors, DIM as i32),
            ),
        ],
    )
}

/// Compact digest of one parsed module, for the per-commit fingerprint
/// table. Not cryptographic: enough to detect that a module changed.
pub fn digest(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeddings_are_fixed_width_and_normalized() {
        let v = embed("GET /orders handler list_orders stories US-100");
        assert_eq!(v.len(), DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "norm was {norm}");
    }

    #[test]
    fn overlapping_symptoms_land_near_their_route() {
        // Two routes share no vocabulary; a symptom about orders should sit
        // closer to the orders route than to the ping route.
        let orders = embed("GET /orders handler list_orders stories US-100 order");
        let ping = embed("GET /ping handler ping stories US-001 pong");
        let symptom = embed("orders endpoint broken");
        let dist_orders = orders
            .iter()
            .zip(&symptom)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>();
        let dist_ping = ping
            .iter()
            .zip(&symptom)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>();
        assert!(
            dist_orders < dist_ping,
            "orders {dist_orders} vs ping {dist_ping}"
        );
    }

    #[test]
    fn digest_changes_when_text_changes() {
        assert_ne!(digest("GET /orders"), digest("GET /payments"));
        assert_eq!(digest("same"), digest("same"));
    }

    #[tokio::test]
    async fn indexed_route_ranks_first_for_its_symptom() {
        // Index two routes into a real LanceDB table in a temp dir, then
        // search for a symptom that only the orders route can answer. The
        // orders chunk must come back ranked first.
        let dir = std::env::temp_dir().join(format!(
            "rivet-vector-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let routes = vec![
            RouteSummary {
                method: "GET".into(),
                path: "/ping".into(),
                handler: "ping".into(),
                stories: vec!["US-001".into()],
            },
            RouteSummary {
                method: "POST".into(),
                path: "/orders".into(),
                handler: "create_order".into(),
                stories: vec!["US-100".into()],
            },
        ];
        index_blueprint(&dir, "app", &routes)
            .await
            .expect("index lands");

        let results = search(&dir, "app", "orders create failing")
            .await
            .expect("search runs");
        assert!(!results.is_empty(), "search returned no chunks");
        let (best, _) = &results[0];
        assert!(
            best.contains("POST /orders"),
            "orders route should rank first, got {best:?} from {results:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
