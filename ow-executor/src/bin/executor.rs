use ow_executor::core;

static ADDRESS: &str = "127.0.0.1:9000";

#[async_std::main]
async fn main() -> anyhow::Result<()> {

    
    #[cfg(all(feature = "wasmtime", not(feature = "wasmtime_nn"), not(feature = "wasmtime_component"), not(feature = "wasmtime_component_nn") ))]
    let runtime = ow_wasmtime::Wasmtime::default();

    #[cfg(feature = "wasmtime_nn")]
    let runtime = ow_wasmtime_nn::Wasmtime::default();

    #[cfg(feature = "wasmtime_component")]
    let runtime = ow_wasmtime_component::Wasmtime::default();

    #[cfg(feature = "wasmtime_component_nn")]
    let runtime = ow_wasmtime_component_nn::Wasmtime::default();

    let mut executor = tide::with_state(runtime);

    executor.at("/:container_id/destroy").post(core::destroy);
    executor.at("/:container_id/init").post(core::init);
    executor.at("/:container_id/run").post(core::run);

    println!("Listening on: {}", ADDRESS);

    executor.listen(ADDRESS).await.unwrap();

    Ok(())
}
