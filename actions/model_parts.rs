use anyhow::{Result, anyhow};
use image::{DynamicImage, RgbImage};
use wasi_nn::{self, ExecutionTarget, GraphBuilder, GraphEncoding};
use base64;
use std::time::Instant;
use std::collections::HashMap;

pub fn func(json: serde_json::Value, model_bytes: &[u8]) -> Result<serde_json::Value, anyhow::Error> {
    let total_start_time = Instant::now();
    let mut metrics: HashMap<String, f64> = HashMap::new();

    let start_time = Instant::now();
    let graph = GraphBuilder::new(GraphEncoding::Pytorch, ExecutionTarget::CPU)
        .build_from_bytes(&[&model_bytes])?;
    let mut context = graph.init_execution_context()?;
    metrics.insert("graph_buildinit_time".to_string(), start_time.elapsed().as_secs_f64());

    let results: Vec<String> = Vec::new();

    Ok(serde_json::json!({
        "results": results,
        "metrics": metrics
    }))
}


#[derive(Debug, PartialEq)]
struct InferenceResult(usize, f32);
