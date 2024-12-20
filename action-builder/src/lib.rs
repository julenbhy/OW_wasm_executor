#[macro_export]
macro_rules! args_method {
    ($($t:ident)*) => ($(

        static mut RESULT: Option<String> = None;

        #[no_mangle]
        pub extern "C" fn get_result() -> *const u8 {
            unsafe {
                    RESULT.as_ref().unwrap().as_ptr()
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result_len() -> usize {
            unsafe {
                    RESULT.as_ref().unwrap().len()
            }
        }

        pub fn main() -> anyhow::Result<()> {

            let args: Vec<String> = std::env::args().collect();
            let json_str = &args[0];
            let json: serde_json::Value = serde_json::from_str(json_str)?;

            let result_json = $t(json)?;

            unsafe {
                RESULT = Some(result_json.to_string());
            }
            
            /*
            unsafe {
                println!(
                    "From WASM:\n\tResult ptr (decimal): {}\n\tResult length: {}\n\tResult: {}",
                    RESULT.as_ref().unwrap().as_ptr() as usize,
                    RESULT.as_ref().unwrap().len(),
                    RESULT.as_ref().unwrap()
                );
            }
            */
            Ok(())
        }
        
    )*)
}


#[macro_export]
macro_rules! stdio_method {
    ($($t:ident)*) => ($(

        use std::{error::Error, io::stdin};


        pub fn main() -> anyhow::Result<()> {

            let json: serde_json::Value = serde_json::from_reader(stdin())?;

            let result_json = $t(json)?;

            // Build a response JSON such as: {"response": "value"}
            let response_json = serde_json::json!({
                "response": result_json
            });

            println!("{}", response_json.to_string());
            
            Ok(())
        }
        
    )*)
}


#[macro_export]
macro_rules! memory_method {
    ($($t:ident)*) => ($(

        use serde_json::Value;
        use std::ptr;
        use std::alloc::{alloc, Layout};

        static mut INPUT: *mut u8 = ptr::null_mut();
        static mut INPUT_LEN: usize = 0;
        static mut RESULT: Option<String> = None;

        #[no_mangle]
        pub extern "C" fn set_input(size: usize) -> *mut u8 {
            unsafe {
                INPUT = alloc(Layout::from_size_align(size, 1).unwrap());
                INPUT_LEN = size;
                INPUT
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result() -> *const u8 {
            unsafe {
                    RESULT.as_ref().unwrap().as_ptr()
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result_len() -> usize {
            unsafe {
                    RESULT.as_ref().unwrap().len()
            }
        }

        pub fn main() -> anyhow::Result<()> {
            unsafe {
                // Parse the input JSON
                let input_slice = std::slice::from_raw_parts(INPUT, INPUT_LEN);
                let input_str = std::str::from_utf8(input_slice).unwrap();
                let json: Value = serde_json::from_str(input_str).unwrap();
        
                // Call the function
                let result_json = $t(json)?;
        
                // Save the result as a string
                RESULT = Some(result_json.to_string());
        
                /*
                println!(
                    "From rust:\n\tResult ptr (decimal): {}\n\tResult length: {}\n\tResult: {}",
                    RESULT.as_ref().unwrap().as_ptr() as usize,
                    RESULT.as_ref().unwrap().len(),
                    RESULT.as_ref().unwrap()
                );
                */
            }
        
            Ok(())
        }
        
    )*)
}


#[macro_export]
macro_rules! memory_nn_method {
    ($($t:ident)*) => ($(

        use serde_json::Value;
        use std::ptr;
        use std::alloc::{alloc, Layout};

        static mut INPUT: *mut u8 = ptr::null_mut();
        static mut INPUT_LEN: usize = 0;
        static mut RESULT: Option<String> = None;
        static mut MODEL: *mut u8 = ptr::null_mut();
        static mut MODEL_LEN: usize = 0;

        #[no_mangle]
        pub extern "C" fn set_input(size: usize) -> *mut u8 {
            unsafe {
                INPUT = alloc(Layout::from_size_align(size, 1).unwrap());
                INPUT_LEN = size;
                INPUT
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result() -> *const u8 {
            unsafe {
                    RESULT.as_ref().unwrap().as_ptr()
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result_len() -> usize {
            unsafe {
                    RESULT.as_ref().unwrap().len()
            }
        }

        #[no_mangle]
        pub extern "C" fn set_model(size: usize) -> *mut u8 {
            unsafe {
                MODEL = alloc(Layout::from_size_align(size, 1).unwrap());
                MODEL_LEN = size;
                MODEL
            }
        }

        pub fn main() -> anyhow::Result<()> {
            unsafe {
                // Parse the input JSON
                let input_slice = std::slice::from_raw_parts(INPUT, INPUT_LEN);
                let input_str = std::str::from_utf8(input_slice).unwrap();
                let json: Value = serde_json::from_str(input_str).unwrap();

                let model_bytes = unsafe { std::slice::from_raw_parts(MODEL, MODEL_LEN) };
        
                // Call the function
                let result_json = $t(json, model_bytes)?;
        
                // Save the result as a string
                RESULT = Some(result_json.to_string());
        
                /*
                println!(
                    "From rust:\n\tResult ptr (decimal): {}\n\tResult length: {}\n\tResult: {}",
                    RESULT.as_ref().unwrap().as_ptr() as usize,
                    RESULT.as_ref().unwrap().len(),
                    RESULT.as_ref().unwrap()
                );
                */
            }
        
            Ok(())
        }
        
    )*)
}


#[macro_export]
macro_rules! memory_nn_parallel_method {
    ($($t:ident)*) => ($(

        use serde_json::Value;
        use std::ptr;
        use std::alloc::{alloc, Layout};
        use serde_json::{json};

        static mut INPUT: *mut u8 = ptr::null_mut();
        static mut INPUT_LEN: usize = 0;
        static mut RESULT: Option<String> = None;
        static mut MODEL: *mut u8 = ptr::null_mut();
        static mut MODEL_LEN: usize = 0;

        #[no_mangle]
        pub extern "C" fn set_input(size: usize) -> *mut u8 {
            unsafe {
                INPUT = alloc(Layout::from_size_align(size, 1).unwrap());
                INPUT_LEN = size;
                INPUT
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result() -> *const u8 {
            unsafe {
                    RESULT.as_ref().unwrap().as_ptr()
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result_len() -> usize {
            unsafe {
                    RESULT.as_ref().unwrap().len()
            }
        }

        #[no_mangle]
        pub extern "C" fn set_model(size: usize) -> *mut u8 {
            unsafe {
                MODEL = alloc(Layout::from_size_align(size, 1).unwrap());
                MODEL_LEN = size;
                MODEL
            }
        }

        pub fn main() -> anyhow::Result<()> {
            unsafe {
                let instance_start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();
                // Parse the input JSON
                let input_slice = std::slice::from_raw_parts(INPUT, INPUT_LEN);
                let input_str = std::str::from_utf8(input_slice).unwrap();
                let json: Value = serde_json::from_str(input_str).unwrap();

                let model_bytes = unsafe { std::slice::from_raw_parts(MODEL, MODEL_LEN) };

                // Call the function
                let mut result_json = $t(json, model_bytes)?;
                let instance_end = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();

                if let Some(result_obj) = result_json.as_object_mut() {
                    let metrics = result_obj
                        .entry("metrics")
                        .or_insert_with(|| json!({}));

                    if let Some(metrics_obj) = metrics.as_object_mut() {
                        metrics_obj.insert("instance_start".to_string(), json!(instance_start));
                        metrics_obj.insert("instance_end".to_string(), json!(instance_end));
                    }
                }

                // Save the result as a string
                RESULT = Some(result_json.to_string());

                /*
                println!(
                    "From rust:\n\tResult ptr (decimal): {}\n\tResult length: {}\n\tResult: {}",
                    RESULT.as_ref().unwrap().as_ptr() as usize,
                    RESULT.as_ref().unwrap().len(),
                    RESULT.as_ref().unwrap()
                );
                */
            }

            Ok(())
        }

    )*)
}