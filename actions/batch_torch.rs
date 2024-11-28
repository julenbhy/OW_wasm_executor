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
    metrics.insert("graph_build_time".to_string(), start_time.elapsed().as_secs_f64());

    let start_time = Instant::now();
    let mut context = graph.init_execution_context()?;
    metrics.insert("context_init_time".to_string(), start_time.elapsed().as_secs_f64(),);

    // Get list of images from JSON
    let start_time = Instant::now();
    let images_base64 = json["image"].as_array().ok_or_else(|| {
        anyhow::anyhow!("From wasm: 'images' not found or not an array in JSON")
    })?;

    let class_labels = json["class_labels"].as_array().ok_or_else(|| {
        anyhow::anyhow!("From wasm: 'class_labels' not found or not an array in JSON")
    })?;

    let top_k = json["top_k"].as_u64().unwrap_or(5) as usize;
    let image_names = json["image_names"].as_array().ok_or_else(|| {
        anyhow::anyhow!("From wasm: 'image_names' not found or not an array in JSON")
    })?;
    metrics.insert("parameters_parse_time".to_string(),start_time.elapsed().as_secs_f64());

    let start_time = Instant::now();
    let input_data = preprocess(
        images_base64,
        224,
        224,
        &[0.485, 0.456, 0.406],
        &[0.229, 0.224, 0.225],
    )?;

    let batch_size = images_base64.len();
    let precision = wasi_nn::TensorType::F32;
    let shape = &[batch_size as usize, 3, 224, 224];
    metrics.insert("transform_time".to_string(), start_time.elapsed().as_secs_f64());

    let start_time = Instant::now();
    context.set_input(0, precision, shape, &input_data)?;
    metrics.insert("set_input_time".to_string(), start_time.elapsed().as_secs_f64());

    // Perform inference
    let start_time = Instant::now();
    context.compute()?;
    metrics.insert("inference_time".to_string(), start_time.elapsed().as_secs_f64());

    let start_time = Instant::now();
    let mut output_buffer = vec![0f32; 1000 * batch_size];
    context.get_output(0, &mut output_buffer)?;

    let mut batch_outputs = Vec::new();
    for i in 0..batch_size {
        let start = i * 1000;
        let end = (i + 1) * 1000;
        batch_outputs.push(output_buffer[start..end].to_vec());
    }

    let results = batch_outputs
        .iter()
        .zip(image_names.iter())
        .map(|(output, name)| {
            let probabilities = softmax(output.clone());
            let top_results = sort_results(&probabilities)[..top_k]
                .iter()
                .map(|InferenceResult(class, prob)| {
                    let label = class_labels
                        .get(*class)
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    serde_json::json!({
                        "class": class,
                        "probability": prob,
                        "label": label,
                        "image_name": name.as_str().unwrap_or("unknown")
                    })
                })
                .collect::<Vec<serde_json::Value>>();
            top_results
        })
        .collect::<Vec<Vec<serde_json::Value>>>();
    metrics.insert("postprocessing_time".to_string(), start_time.elapsed().as_secs_f64());

    let total_elapsed_time = total_start_time.elapsed();
    metrics.insert("total_func_time".to_string(),total_elapsed_time.as_secs_f64());

    Ok(serde_json::json!({
        "results": results,
        "metrics": metrics
    }))
}


// Transform a list of images into a batch tensor
fn preprocess(
    images_base64: &[serde_json::Value],
    height: u32,
    width: u32,
    mean: &[f32],
    std: &[f32],
) -> Result<Vec<u8>, anyhow::Error> {
    let mut batch_tensors = Vec::new();
    println!("Transforming images into tensors...");

    // Iterate over each image in the list
    for image_base64 in images_base64 {
        let image_base64_str = image_base64.as_str().ok_or_else(|| {
            anyhow::anyhow!("From wasm: 'image' should be a base64 string")
        })?;
        println!("Decoding image from base64...");

        let image_bytes = base64::decode(image_base64_str)?;
        println!("Image decoded, preprocessing...");

        // Preprocess the image and add it to the batch
        let tensor_data = preprocess_one(image_bytes, width, height, mean, std);
        batch_tensors.extend(tensor_data);
        println!("Image tensor added to batch.");
    }

    Ok(batch_tensors)
}

// Resize image to height x width, and then converts the pixel precision to FP32, normalize with
// given mean and std. The resulting RGB pixel vector is then returned.
fn preprocess_one(image: Vec<u8>, height: u32, width: u32, mean: &[f32], std: &[f32]) -> Vec<u8> {
    println!("Image size in bytes: {}", image.len());
    let img = image::load_from_memory(&image).unwrap().to_rgb8();
    let resized =
        image::imageops::resize(&img, height, width, ::image::imageops::FilterType::Triangle);

    let mut flat_img: Vec<f32> = Vec::new();
    for rgb in resized.pixels() {
        flat_img.push((rgb[0] as f32 / 255. - 0.485) / 0.229);
        flat_img.push((rgb[1] as f32 / 255. - 0.456) / 0.224);
        flat_img.push((rgb[2] as f32 / 255. - 0.406) / 0.225);
    }
    let bytes_required = flat_img.len() * 4;
    let mut u8_f32_arr: Vec<u8> = vec![0; bytes_required];

    for c in 0..3 {
        for i in 0..(flat_img.len() / 3) {
            // Read the number as a f32 and break it into u8 bytes
            let u8_f32: f32 = flat_img[i * 3 + c] as f32;
            let u8_bytes = u8_f32.to_ne_bytes();

            for j in 0..4 {
                u8_f32_arr[((flat_img.len() / 3 * c + i) * 4) + j] = u8_bytes[j];
            }
        }
    }

    println!("Image preprocessed.");
    u8_f32_arr
}

fn softmax(output_tensor: Vec<f32>) -> Vec<f32> {
    let max_val = output_tensor
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max);

    // Compute the exponential of each element subtracted by max_val for numerical stability.
    let exps: Vec<f32> = output_tensor.iter().map(|&x| (x - max_val).exp()).collect();

    // Compute the sum of the exponentials.
    let sum_exps: f32 = exps.iter().sum();

    // Normalize each element to get the probabilities.
    exps.iter().map(|&exp| exp / sum_exps).collect()
}

fn sort_results(buffer: &[f32]) -> Vec<InferenceResult> {
    let mut results: Vec<InferenceResult> = buffer
        .iter()
        .enumerate()
        .map(|(c, p)| InferenceResult(c, *p))
        .collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    results
}

#[derive(Debug, PartialEq)]
struct InferenceResult(usize, f32);
