use ow_executor::core;
// use tide_tracing::TraceMiddleware;
// use tracing::Level;

static ADDRESS: &str = "127.0.0.1:9000";

#[async_std::main]
async fn main() -> anyhow::Result<()> {

    #[cfg(feature = "wasmtime_args")]
    let runtime = ow_wasmtime_args::Wasmtime::default();

    #[cfg(feature = "wasmtime_stdio")]
    let runtime = ow_wasmtime_stdio::Wasmtime::default();

    #[cfg(feature = "wasmtime_memory")]
    let runtime = ow_wasmtime_memory::Wasmtime::default();

    #[cfg(feature = "wasmtime_component")]
    let runtime = ow_wasmtime_component::Wasmtime::default();




    // let subscriber = tracing_subscriber::fmt()
    //     .with_max_level(Level::TRACE)
    //     .finish();

    // tracing::subscriber::set_global_default(subscriber).expect("no global subscriber has been set");

    let mut executor = tide::with_state(runtime);
    // executor.with(TraceMiddleware::new());

    executor.at("/:container_id/destroy").post(core::destroy);
    executor.at("/:container_id/init").post(core::init);
    executor.at("/:container_id/run").post(core::run);

    println!("Listening on: {}", ADDRESS);

    executor.listen(ADDRESS).await.unwrap();

    Ok(())
}
