#[cfg(feature = "component")]
wit_bindgen::generate!({
    path: "wit",
    world: "simple",
});

#[cfg(feature = "component_nn")]
wit_bindgen::generate!({
    path: "wit",
    world: "nn",
});

#[cfg(feature = "component_nn")]
use self::wasi::nn::{
    graph::{Graph, GraphBuilder, load, ExecutionTarget, GraphEncoding},
    tensor::{Tensor, TensorData, TensorDimensions, TensorType},
};



struct MyWorld;
impl Guest for MyWorld {
    fn func_wrapper(json_string: std::string::String) -> std::string::String {
        let json: serde_json::Value = serde_json::from_str(&json_string).unwrap();
        let result = func(json).unwrap();
        result.to_string()
    }
}
export!(MyWorld);





// The function that will be called by the wrapper will be added bellow
use anyhow::{Result, anyhow};
use image::{DynamicImage, RgbImage};
use base64;

pub fn func(json: serde_json::Value) -> Result<serde_json::Value, anyhow::Error> {

    println!("\n\nFROM WASM: running");

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
    let tensor_data = preprocess(
        image_bytes.as_slice(),
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
    
    let output_buffer = vec![0f32; 1000];
    let _output_data = context.get_output("squeezenet0_flatten0_reshape0").unwrap().data();

    let result = softmax(output_buffer);

    /*
        println!(
        "Found results, sorted top 5: {:?}",
        &sort_results(&result)[..5]
    );
    */
    // Build the response json sunch as the previous print statement
    let result = sort_results(&result)[..5]
        .iter()
        .map(|InferenceResult(class, prob)| {
            serde_json::json!({
                "class": class,
                "probability": prob
            })
        })
        .collect::<Vec<serde_json::Value>>();



    Ok(serde_json::json!({"result": result}))
}


// Resize image to height x width, and then converts the pixel precision to FP32, normalize with
// given mean and std. The resulting RGB pixel vector is then returned.
fn preprocess(image: &[u8], height: u32, width: u32, mean: &[f32], std: &[f32]) -> Vec<u8> {
    let dyn_img: DynamicImage = image::load_from_memory(image).unwrap().resize_exact(
        width,
        height,
        image::imageops::Triangle,
    );
    let rgb_img: RgbImage = dyn_img.to_rgb8();

    // Get an array of the pixel values
    let raw_u8_arr: &[u8] = &rgb_img.as_raw()[..];

    // Create an array to hold the f32 value of those pixels
    let bytes_required = raw_u8_arr.len() * 4;
    let mut u8_f32_arr: Vec<u8> = vec![0; bytes_required];

    // Read the number as a f32 and break it into u8 bytes
    for i in 0..raw_u8_arr.len() {
        let u8_f32: f32 = raw_u8_arr[i] as f32;
        let rgb_iter = i % 3;

        // Normalize the pixel
        let norm_u8_f32: f32 = (u8_f32 / 255.0 - mean[rgb_iter]) / std[rgb_iter];

        // Convert it to u8 bytes and write it with new shape
        let u8_bytes = norm_u8_f32.to_ne_bytes();
        for j in 0..4 {
            u8_f32_arr[(raw_u8_arr.len() * 4 * rgb_iter / 3) + (i / 3) * 4 + j] = u8_bytes[j];
        }
    }
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
