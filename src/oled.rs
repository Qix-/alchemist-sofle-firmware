// Based entirely on the stars example from:
// https://people.ece.cornell.edu/land/courses/ece4760/labs/s2021/stars/stars.html

use embassy_rp::{
	clocks::RoscRng,
	i2c::{self, Async, I2c},
	peripherals::{I2C1, PIN_2, PIN_3},
};
use embassy_time::Timer;
use rand::{Rng, SeedableRng, rngs::SmallRng};

use crate::frames;

const SZ: usize = 128 * 32 / 8;
const OLED_ADDR: u16 = 0x3C;
const FREQUENCY: u32 = 200_000;
const STAR_TOTAL: usize = 90;
const MIN_STARS: usize = 10;
const MIN_LIFETIME: u16 = 80;
const MAX_LIFETIME: u16 = 1000;
const LONG_LIVED_CHANCE: u16 = 10;
const LONG_LIVED_MULTIPLIER: u16 = 10;
const MAX_SPAWN_COUNT: usize = STAR_TOTAL * 10;

static DEATH_FADEOUT_LUT: &[u8] = &[0b1101_1111, 0b1110_1001, 0b1001_1000, 0b1100_0001];
const DEATH_TIME: u16 = (DEATH_FADEOUT_LUT.len() * 8) as u16;

static mut BUFFERS: [[u8; SZ]; 2] = [[0; SZ]; 2];

static mut SPAWN_COUNT: usize = 0;

#[expect(non_camel_case_types)]
type fx16 = ::fixed::FixedI32<::fixed::types::extra::U16>;

pub fn spawn_star() {
	unsafe {
		SPAWN_COUNT += 1;
	}
}

static TURN_FACTOR: fx16 = fx16::lit("0.2");
static VISUAL_RANGE: fx16 = fx16::lit("16");
static PROTECTED_RANGE: fx16 = fx16::lit("3");
static CENTERING_FACTOR: fx16 = fx16::lit("0.0005");
static AVOID_FACTOR: fx16 = fx16::lit("0.05");
static MATCHING_FACTOR: fx16 = fx16::lit("0.04");
static MAX_SPEED: fx16 = fx16::lit("1.7");
static MIN_SPEED: fx16 = fx16::lit("0.4");

static WIDTH: fx16 = fx16::lit("32");
static HEIGHT: fx16 = fx16::lit("128");
static PADDING: fx16 = fx16::lit("5");

pub struct OledConfig {
	pub i2c1:  I2C1,
	pub pin_3: PIN_3,
	pub pin_2: PIN_2,
}

#[derive(Clone, Copy)]
struct Star {
	x:        fx16,
	y:        fx16,
	vx:       fx16,
	vy:       fx16,
	lifetime: u16,
}

#[embassy_executor::task]
pub async fn oled_task(config: OledConfig) -> ! {
	static mut STAR_BUFFER: [Option<Star>; STAR_TOTAL] = [None; STAR_TOTAL];

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

	let mut xpos_avg: fx16;
	let mut ypos_avg: fx16;
	let mut xvel_avg: fx16;
	let mut yvel_avg: fx16;
	let mut neighboring_stars: fx16;
	let mut close_dx: fx16;
	let mut close_dy: fx16;

	#[expect(static_mut_refs)]
	loop {
		unsafe {
			SPAWN_COUNT = SPAWN_COUNT.min(MAX_SPAWN_COUNT);
		}

		frame_counter = frame_counter.wrapping_add(1);

		let buffer = unsafe { &mut BUFFERS[buffer_idx] };
		buffer_idx = 1 - buffer_idx;

		buffer.fill(0);

		let protected_range_squared = PROTECTED_RANGE * PROTECTED_RANGE;
		let visual_range_squared = VISUAL_RANGE * VISUAL_RANGE;

		let left_bound = PADDING;
		let right_bound = WIDTH - PADDING;
		let top_bound = PADDING;
		let bottom_bound = HEIGHT - PADDING;

		let mut total_found = 0;

		for our_idx in 0..unsafe { STAR_BUFFER.len() } {
			if unsafe { STAR_BUFFER[our_idx] }.is_none() {
				if unsafe { SPAWN_COUNT } > 0 {
					unsafe {
						SPAWN_COUNT -= 1;
					}

					let x = fx16::from_num(rng.gen_range(0..32));
					let y = fx16::from_num(rng.gen_range(0..128));
					let vx = fx16::from_num(rng.gen_range(-2..2));
					let vy = fx16::from_num(rng.gen_range(-2..2));
					let mut lifetime = rng.gen_range(MIN_LIFETIME..MAX_LIFETIME);

					if rng.gen_range(0..LONG_LIVED_CHANCE) == 0 {
						lifetime = lifetime.saturating_mul(LONG_LIVED_MULTIPLIER);
					}

					unsafe {
						STAR_BUFFER[our_idx] = Some(Star {
							x,
							y,
							vx,
							vy,
							lifetime,
						});
					}
				}
			}

			if unsafe { STAR_BUFFER[our_idx] }.is_none() {
				continue;
			}

			total_found += 1;

			// Zero all accumulator registers
			xpos_avg = fx16::lit("0");
			ypos_avg = fx16::lit("0");
			xvel_avg = fx16::lit("0");
			yvel_avg = fx16::lit("0");
			neighboring_stars = fx16::lit("0");
			close_dx = fx16::lit("0");
			close_dy = fx16::lit("0");

			for other_idx in 0..unsafe { STAR_BUFFER.len() } {
				if our_idx == other_idx || unsafe { STAR_BUFFER[other_idx] }.is_none() {
					continue;
				}

				let star = unsafe { STAR_BUFFER[our_idx].as_ref().unwrap() };
				let other_star = unsafe { STAR_BUFFER[other_idx].as_ref().unwrap() };

				// Compute differences in x and y coordinates
				let dx = star.x - other_star.x;
				let dy = star.y - other_star.y;

				// Are both those differences less than the visual range?
				if dx.abs() < VISUAL_RANGE && dy.abs() < VISUAL_RANGE {
					// If so, calculate the squared distance
					let squared_distance = dx * dx + dy * dy;

					if squared_distance < protected_range_squared {
						// Is squared distance less than the protected range?
						// If so, calculate difference in x/y-coordinates to nearfield star
						close_dx += star.x - other_star.x;
						close_dy += star.y - other_star.y;
					} else if squared_distance < visual_range_squared {
						// If not in protected range, is the star in the visual range?
						// Add other star's x/y-coord and x/y vel to accumulator variables
						xpos_avg += other_star.x;
						ypos_avg += other_star.y;
						xvel_avg += other_star.vx;
						yvel_avg += other_star.vy;

						// Increment number of stars within visual range
						neighboring_stars += fx16::lit("1");
					}
				}
			}

			let star = unsafe { STAR_BUFFER[our_idx].as_mut().unwrap() };

			// If there were any stars in the visual range...
			if neighboring_stars > fx16::lit("0") {
				// Divide accumulator variables by number of stars in visual range
				xpos_avg /= neighboring_stars;
				ypos_avg /= neighboring_stars;
				xvel_avg /= neighboring_stars;
				yvel_avg /= neighboring_stars;

				// Add the centering/matching contributions to velocity
				star.vx +=
					(xpos_avg - star.x) * CENTERING_FACTOR + (xvel_avg - star.vx) * MATCHING_FACTOR;

				star.vy +=
					(ypos_avg - star.y) * CENTERING_FACTOR + (yvel_avg - star.vy) * MATCHING_FACTOR;
			}

			// Add the avoidance contribution to velocity
			star.vx += close_dx * AVOID_FACTOR;
			star.vy += close_dy * AVOID_FACTOR;

			// If the star is near an edge, make it turn by turnfactor
			if star.y < top_bound {
				star.vy += TURN_FACTOR;
			}
			if star.x > right_bound {
				star.vx -= TURN_FACTOR;
			}
			if star.x < left_bound {
				star.vx += TURN_FACTOR;
			}
			if star.y > bottom_bound {
				star.vy -= TURN_FACTOR;
			}

			// Calculate the star's speed
			let speed = (star.vx * star.vx + star.vy * star.vy).sqrt();

			// Enforce min and max speeds
			if speed < MIN_SPEED {
				// Prevent division by zero
				let speed = speed.max(fx16::from_bits(1));
				star.vx = (star.vx / speed) * MIN_SPEED;
				star.vy = (star.vy / speed) * MIN_SPEED;
			}
			if speed > MAX_SPEED {
				star.vx = (star.vx / speed) * MAX_SPEED;
				star.vy = (star.vy / speed) * MAX_SPEED;
			}

			// Update star's position
			star.x += star.vx;
			star.y += star.vy;

			// If the star is puttering out (flickering) determine
			// if it should display this frame.
			let show_star = {
				if star.lifetime <= DEATH_TIME {
					let death_index = DEATH_TIME - star.lifetime;
					(DEATH_FADEOUT_LUT[death_index as usize / 8] >> (death_index % 8)) & 1 == 1
				} else {
					true
				}
			};

			if show_star {
				// Set its position in the buffer
				let x: i32 = star.x.to_num();
				let y: i32 = star.y.to_num();

				if x < -10 || x >= 42 || y < -10 || y >= 138 {
					// Insta-kill
					star.lifetime = 0;
				} else {
					let x = x.max(0).min(31);
					let y = y.max(0).min(127);

					let idx = y as usize * 32 + x as usize;
					let byte = idx / 8;
					let bit = idx % 8;
					let mask = 1_u8 << bit;

					buffer.get_mut(byte).map(|b| {
						*b |= mask;
					});
				}
			}

			// Decrement the star's lifetime
			star.lifetime = star.lifetime.saturating_sub(rng.gen_range(1..4));
			if star.lifetime == 0 {
				unsafe {
					STAR_BUFFER[our_idx] = None;
				}
			}
		}

		if total_found < MIN_STARS && rng.gen_range(0..10) == 0 {
			unsafe {
				SPAWN_COUNT += 1;
			}
		}

		send_buffer(&mut i2c, buffer).await;

		Timer::after_millis(1000 / 120).await;
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
