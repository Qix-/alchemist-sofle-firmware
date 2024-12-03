#![no_std]
#![no_main]

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
	alchemist::run_alchemist(spawner, alchemist::BoardSide::Left).await
}
