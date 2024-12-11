use anyhow::{Result, anyhow};
use image::{DynamicImage, RgbImage};
use base64;
use bytemuck::cast_slice;

pub fn func(json: serde_json::Value) -> Result<serde_json::Value, anyhow::Error> {

    println!("\n\nFROM WASM: running");

    let class_labels = json["class_labels"].as_array().ok_or_else(|| {
        anyhow::anyhow!("From wasm: 'class_labels' not found or not an array in JSON")
    })?;

    // Get the model
    let model_base64 = json["model"].as_str().ok_or_else(|| {
        anyhow::anyhow!("From wasm: 'model' not found or not a string in JSON")
    })?;
    let model_bytes = base64::decode(model_base64).unwrap();

    //let graph = GraphBuilder::new(GraphEncoding::Pytorch, ExecutionTarget::Cpu)
    //    .build_from_bytes(&[&model_bytes])?;
    let graph = load(&[model_bytes], GraphEncoding::Pytorch, ExecutionTarget::Cpu).unwrap();

    let context = graph.init_execution_context().unwrap();

    // Get the image bytes from JSON
    let image_base64 = json["image"].as_str().ok_or_else(|| {
        anyhow::anyhow!("From wasm: 'image' not found or not a string in JSON")
    })?;
    let image_bytes = base64::decode(image_base64)?;

    // Preprocessing. Normalize data based on model requirements https://github.com/onnx/models/tree/main/validated/vision/classification/mobilenet#preprocessing
    let tensor_data = preprocess_one(
        image_bytes,
        224,
        224,
        &[0.485, 0.456, 0.406],
        &[0.229, 0.224, 0.225],
    );
    let precision = TensorType::Fp32;
    let shape: TensorDimensions = vec![1, 3, 224, 224];
    let tensor = Tensor::new(
        &shape,
        precision,
        &tensor_data,
    );
    //context.set_input(0, precision, shape, &tensor_data).unwrap();
    context.set_input("data", tensor).unwrap();

    context.compute().unwrap();

    let output_data = context.get_output("squeezenet0_flatten0_reshape0").unwrap().data();

    let output_len = output_data.len();
    println!("Output data length: {}", output_len);
    let output_vec: Vec<f32> = cast_slice(&output_data).to_vec();
//     let output_vec = vec![0f32; 1000];
    let result = softmax(output_vec);

    let result = sort_results(&result)[..5]
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
            })
        })
        .collect::<Vec<serde_json::Value>>();



    Ok(serde_json::json!({"result": result}))
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
