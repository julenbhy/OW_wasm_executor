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
