use embassy_futures::select::select5;
use embassy_rp::{
	gpio::{Input, Level, Output, Pull},
	peripherals::{
		PIN_5, PIN_6, PIN_7, PIN_8, PIN_9, PIN_20, PIN_21, PIN_22, PIN_23, PIN_26, PIN_27,
	},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::Timer;

pub static EVENTS: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();

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

	let mut bitmap: u32 = 0;

	for output in outputs.iter_mut() {
		output.set_high();
	}

	loop {
		// Strobe the pins to check for key presses.
		for (x, output) in outputs.iter_mut().enumerate() {
			for (y, input) in [&mut in_0, &mut in_1, &mut in_2, &mut in_3, &mut in_4]
				.iter_mut()
				.enumerate()
			{
				let bitmask = 1 << (x + y * 6);

				output.set_low();
				Timer::after_nanos(100).await;
				let new_state = input.is_high();
				output.set_high();

				let current_state = (bitmap & bitmask) != 0;

				match (current_state, new_state) {
					(true, false) => {
						bitmap &= !bitmask;
						EVENTS.send(Event::Up(x as u8, y as u8)).await;
					}
					(false, true) => {
						bitmap |= bitmask;
						EVENTS.send(Event::Down(x as u8, y as u8)).await;
					}
					_ => {}
				}
			}
		}

		// Go into a sleep mode if no keys are pressed.
		if bitmap == 0 {
			select5(
				in_0.wait_for_high(),
				in_1.wait_for_high(),
				in_2.wait_for_high(),
				in_3.wait_for_high(),
				in_4.wait_for_high(),
			)
			.await;
		} else {
			// Otherwise, wait for a small amount of time.
			Timer::after_nanos(10000).await;
		}
	}
}
