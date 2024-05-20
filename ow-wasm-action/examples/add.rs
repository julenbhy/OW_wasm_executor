use std::{error::Error, io::stdin};

use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Input {
    pub param1: i32,
    pub param2: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Output {
    pub response: i32,
}


fn main() -> Result<(), Box<dyn Error>> {

    // Get the input
    let input: Input = serde_json::from_reader(stdin()).map_err(|e| {
        eprintln!("ser: {e}");
        e
    })?;

    // Perform the action
    let response: i32 = input.param2 + input.param1;

    // Set the output
    let output = Output { response };
    let serialized = serde_json::to_string(&output).map_err(|e| {
        eprintln!("de: {e}");
        e
    })?;

    println!("{serialized}");

    Ok(())
}
