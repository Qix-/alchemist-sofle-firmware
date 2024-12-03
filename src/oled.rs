use embassy_rp::{
	clocks::RoscRng,
	i2c::{self, Async, I2c},
	peripherals::{I2C1, PIN_2, PIN_3},
};
use embassy_time::Timer;
use rand::{Rng, SeedableRng, rngs::SmallRng};

use crate::frames;

const SZ: usize = 128 * 32 / 8;
const ROW_SZ: usize = 32 / 8;
const OLED_ADDR: u16 = 0x3C;
const FREQUENCY: u32 = 200_000;

static mut BUFFERS: [[u8; SZ]; 2] = [[0; SZ]; 2];

static mut STAR_SPAWN_COUNT: usize = 0;
static mut SIGIL_UPDATE: bool = true;

pub fn spawn_star() {
	unsafe {
		STAR_SPAWN_COUNT += 1;
		SIGIL_UPDATE = true;
	}
}

#[derive(Clone, Copy)]
pub enum Scene {
	Banner,
	Alchemist,
}

#[derive(Clone, Copy)]
pub enum StarMovement {
	Up,
	Down,
}

pub struct OledConfig {
	pub scene:         Scene,
	pub star_movement: StarMovement,
	pub i2c1:          I2C1,
	pub pin_3:         PIN_3,
	pub pin_2:         PIN_2,
}

#[embassy_executor::task]
pub async fn oled_task(config: OledConfig) -> ! {
	static mut STAR_BUFFER: [u8; SZ] = [0; SZ];

	let mut i2c_config = i2c::Config::default();
	i2c_config.frequency = FREQUENCY;
	let mut i2c = I2c::new_async(
		config.i2c1,
		config.pin_3,
		config.pin_2,
		crate::Irqs,
		i2c_config,
	);

	init(&mut i2c).await;
	clear(&mut i2c).await;

	let mut buffer_idx = 0;
	let mut rng = SmallRng::from_rng(RoscRng).unwrap();

	let mut frame_counter: usize = 0;

	let mut sigil_num = 0;
	let mut sig_x = 0;
	let mut sig_y = 0;

	loop {
		frame_counter = frame_counter.wrapping_add(1);

		if unsafe { SIGIL_UPDATE } {
			sigil_num = rng.gen_range(0..frames::SIGILS.len());
			sig_x = (rng.gen_range(0..2) + 1) * 8;
			sig_y = (rng.gen_range(0..4) + 4) * 8;
			unsafe { SIGIL_UPDATE = false };
		}

		let buffer = unsafe { &mut BUFFERS[buffer_idx] };
		buffer_idx = 1 - buffer_idx;

		// Copy the stars
		let spawn_count = unsafe {
			if (frame_counter % 4) < rng.gen_range(0..4) {
				let spawn_count = STAR_SPAWN_COUNT.min(4);
				STAR_SPAWN_COUNT = STAR_SPAWN_COUNT.saturating_sub(spawn_count);
				spawn_count
			} else {
				0
			}
		};

		match config.star_movement {
			StarMovement::Up => {
				(&mut buffer[..(SZ - ROW_SZ)]).copy_from_slice(unsafe { &STAR_BUFFER[ROW_SZ..] });

				let last_row = &mut buffer[(SZ - ROW_SZ)..];
				last_row.fill(0);

				for _ in 0..spawn_count {
					let x = rng.gen_range(0..32);
					last_row[x / 8] |= 1 << (x % 8);
				}
			}
			StarMovement::Down => {
				(&mut buffer[(32 / 8)..])
					.copy_from_slice(unsafe { &STAR_BUFFER[..(32 / 8) * 127] });

				let first_row = &mut buffer[..(32 / 8)];
				first_row.fill(0);

				for _ in 0..spawn_count {
					let x = rng.gen_range(0..32);
					first_row[x / 8] |= 1 << (x % 8);
				}
			}
		}

		// Copy back to the star buffer
		// TODO: This is a bit wasteful, but it's fine for now.
		// TODO: Just need to implement 'scrolling' copies from the star buffer.
		#[expect(static_mut_refs)]
		unsafe {
			STAR_BUFFER.copy_from_slice(buffer)
		};

		// Apply the scene
		let sigil = &frames::SIGILS[sigil_num % frames::SIGILS.len()];

		match config.scene {
			Scene::Banner => {
				apply_mask(
					buffer,
					&frames::BANNER[(frame_counter / 10) % frames::BANNER.len()],
					0,
					0,
				);
				apply_mask(buffer, sigil, sig_x, sig_y + 32);
			}
			Scene::Alchemist => {
				apply_mask(
					buffer,
					&frames::BODY[(frame_counter / 25) % frames::BODY.len()],
					0,
					128 - 32,
				);
				apply_mask(buffer, sigil, sig_x, sig_y);
			}
		}

		send_buffer(&mut i2c, buffer).await;

		Timer::after_millis(1000 / 64).await;
	}
}

fn apply_mask(buffer: &mut [u8; 128 * 32 / 8], frame: &frames::Frame, pos_x: usize, pos_y: usize) {
	let ox = pos_x.min(32 - frame.width) / 8;
	let oy = pos_y.min(128 - frame.height);
	let w = (frame.width + 7) / 8;
	let h = frame.height;

	for x in ox..(ox + w) {
		for y in oy..(oy + h) {
			let mask = (frame.mask[(y - oy) * w + (x - ox)]).reverse_bits();
			let add = (frame.add[(y - oy) * w + (x - ox)]).reverse_bits();

			let buffer_byte = &mut buffer[y * 4 + x];

			*buffer_byte = (*buffer_byte & !mask) | add;
		}
	}
}

async fn clear(i2c: &mut I2c<'_, I2C1, Async>) {
	i2c.write_async(
		OLED_ADDR,
		[0b0100_0000]
			.into_iter()
			.chain([0].into_iter().cycle().take(128 * 32 / 8)),
	)
	.await
	.ok();
}

async fn send_buffer(i2c: &mut I2c<'_, I2C1, Async>, oled_buffer: &[u8]) {
	i2c.write_async(
		OLED_ADDR,
		[0b0100_0000]
			.into_iter()
			.chain(oled_buffer.iter().take(128 * 32 / 8).copied()),
	)
	.await
	.ok();
}

async fn init(i2c: &mut I2c<'_, I2C1, Async>) -> bool {
	macro_rules! write_cmd {
		($($data:expr),*) => {
			i2c.write_async(OLED_ADDR, [0x00, $($data),*]).await.map_err(|_| ())?;
		};
	}

	let r: Result<(), ()> = async {
		write_cmd!(0xAE); // display off
		write_cmd!(0xA8, 0x1F); // set MUX Ratio
		write_cmd!(0xD3, 0x00); // set display offset
		write_cmd!(0x40 | 0x0); // memory Start
		write_cmd!(0xA0); // normal x
		write_cmd!(0xC8); // COM output mode
		write_cmd!(0xDA, 0x02); // COM pin hardware configuration
		write_cmd!(0x81, 0x7F); // contrast max
		write_cmd!(0xA4); // A5 for on, A4 for use RAM
		write_cmd!(0xA6); // A6 for Normal/A7 for inverse
		write_cmd!(0xD5, 0x01); // set oscolation frequency
		write_cmd!(0x8D, 0x14); // set charge pump
		write_cmd!(0xAF); // turn on screen

		write_cmd!(0x20, 0b01); // set address mode
		write_cmd!(0x21, 0, 127); // set column address
		write_cmd!(0x22, 0, 3); // set page address

		Ok(())
	}
	.await;

	r.is_ok()
}
