use embassy_rp::{
	gpio::{Input, Level, Output, Pull},
	peripherals::{
		PIN_5, PIN_6, PIN_7, PIN_8, PIN_9, PIN_20, PIN_21, PIN_22, PIN_23, PIN_26, PIN_27,
	},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::Timer;

pub const KEY_BOUNCE_THRESHOLD: u8 = 7;

pub static EVENTS: Channel<CriticalSectionRawMutex, Event, 64> = Channel::new();

#[derive(Clone)]
pub enum Event {
	Down(u8, u8),
	Up(u8, u8),
}

pub struct KeyprobeConfig {
	pub pin_27: PIN_27,
	pub pin_26: PIN_26,
	pub pin_22: PIN_22,
	pub pin_20: PIN_20,
	pub pin_23: PIN_23,
	pub pin_21: PIN_21,
	pub pin_5:  PIN_5,
	pub pin_6:  PIN_6,
	pub pin_7:  PIN_7,
	pub pin_8:  PIN_8,
	pub pin_9:  PIN_9,
}

#[embassy_executor::task]
pub async fn keyprobe_task(keyboard_config: KeyprobeConfig) {
	let mut outputs = [
		Output::new(keyboard_config.pin_27, Level::High),
		Output::new(keyboard_config.pin_26, Level::High),
		Output::new(keyboard_config.pin_22, Level::High),
		Output::new(keyboard_config.pin_20, Level::High),
		Output::new(keyboard_config.pin_23, Level::High),
		Output::new(keyboard_config.pin_21, Level::High),
	];

	let mut in_0 = Input::new(keyboard_config.pin_5, Pull::Down);
	let mut in_1 = Input::new(keyboard_config.pin_6, Pull::Down);
	let mut in_2 = Input::new(keyboard_config.pin_7, Pull::Down);
	let mut in_3 = Input::new(keyboard_config.pin_8, Pull::Down);
	let mut in_4 = Input::new(keyboard_config.pin_9, Pull::Down);

	in_0.set_schmitt(true);
	in_1.set_schmitt(true);
	in_2.set_schmitt(true);
	in_3.set_schmitt(true);
	in_4.set_schmitt(true);

	let mut counters = [0_u8; 30];

	loop {
		// Strobe the pins to check for key presses.
		for (x, output) in outputs.iter_mut().enumerate() {
			output.set_high();
			Timer::after_micros(3).await;

			for (y, input) in [&mut in_0, &mut in_1, &mut in_2, &mut in_3, &mut in_4]
				.iter_mut()
				.enumerate()
			{
				let idx = x + y * 6;

				let new_state = input.is_high();

				let last_state = counters[idx];

				counters[idx] = if new_state {
					counters[idx].saturating_add(1).min(KEY_BOUNCE_THRESHOLD)
				} else {
					counters[idx].saturating_sub(1)
				};

				let new_state = counters[idx];

				match (last_state, new_state) {
					(1, 0) => {
						EVENTS.send(Event::Up(x as u8, y as u8)).await;
					}
					(l, n) if l == (KEY_BOUNCE_THRESHOLD - 1) && n == KEY_BOUNCE_THRESHOLD => {
						EVENTS.send(Event::Down(x as u8, y as u8)).await;
					}
					_ => {}
				}
			}

			output.set_low();
		}

		Timer::after_micros(100).await;
	}
}
