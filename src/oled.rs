use embassy_futures::join::join;
use embassy_rp::clocks::RoscRng;
use embassy_time::Timer;
use rand::Rng;

use crate::frames;

const SZ: usize = 128 * 32 / 8;
const ROW_SZ: usize = 32 / 8;

static mut BUFFERS: [[u8; SZ]; 2] = [[0; SZ]; 2];

static mut STAR_SPAWN_COUNT: usize = 0;
static mut SIGIL_NUM: usize = 0;

pub fn spawn_star() {
	unsafe {
		STAR_SPAWN_COUNT += 1;
		SIGIL_NUM = SIGIL_NUM.wrapping_add(1);
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
}

#[embassy_executor::task]
pub async fn oled_task(config: OledConfig) -> ! {
	static mut STAR_BUFFER: [u8; SZ] = [0; SZ];

	let mut buffer_idx = 0;
	let mut rng = RoscRng;

	let mut frame_counter: usize = 0;

	unsafe {
		SIGIL_NUM = rng.gen_range(0..frames::SIGILS.len());
	}

	loop {
		frame_counter = frame_counter.wrapping_add(1);

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
		let sigil = &frames::SIGILS[unsafe { SIGIL_NUM } % frames::SIGILS.len()];
		match config.scene {
			Scene::Banner => {
				apply_mask(
					buffer,
					&frames::BANNER[(frame_counter / 5) % frames::BANNER.len()],
					0,
					0,
				);
				apply_mask(buffer, sigil, (32 - sigil.width) >> 1 + 8, 36);
			}
			Scene::Alchemist => {
				apply_mask(
					buffer,
					&frames::BODY[(frame_counter / 5) % frames::BODY.len()],
					0,
					128 - 32,
				);
				apply_mask(
					buffer,
					sigil,
					(32 - sigil.width) >> 1,
					128 - 64 - sigil.height,
				);
			}
		}

		crate::i2c::OLED_CMD.signal(crate::i2c::OledCommand::Buffer(buffer));
		join(Timer::after_millis(1000 / 8), crate::i2c::OLED_IDLE.wait()).await;
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
