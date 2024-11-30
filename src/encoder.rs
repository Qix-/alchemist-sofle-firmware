use embassy_futures::select::select;
use embassy_rp::{
	gpio::{Input, Pull},
	peripherals::{PIN_28, PIN_29},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

pub const ENCODER_MODULO: i8 = 4;

pub static EVENTS: Channel<CriticalSectionRawMutex, Event, 64> = Channel::new();

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Event {
	Cw  = 0,
	Ccw = 1,
}

pub struct EncoderConfig {
	pub pin_29: PIN_29,
	pub pin_28: PIN_28,
}

#[embassy_executor::task]
pub async fn encoder_task(config: EncoderConfig) -> ! {
	let mut pin_a = Input::new(config.pin_28, Pull::Up);
	let mut pin_b = Input::new(config.pin_29, Pull::Up);

	let mut last_a = pin_a.is_high();
	let mut last_b = pin_b.is_high();

	let mut value: i8 = 0;

	loop {
		select(pin_a.wait_for_any_edge(), pin_b.wait_for_any_edge()).await;

		let a = pin_a.is_high();
		let b = pin_b.is_high();

		if a != last_a || b != last_b {
			let event = match (last_a, last_b, a, b) {
				(false, false, false, true) => Event::Cw,
				(false, true, true, true) => Event::Cw,
				(true, true, true, false) => Event::Cw,
				(true, false, false, false) => Event::Cw,
				(false, false, true, false) => Event::Ccw,
				(false, true, false, false) => Event::Ccw,
				(true, true, false, true) => Event::Ccw,
				(true, false, true, true) => Event::Ccw,
				_ => continue,
			};

			match event {
				Event::Cw => {
					if value < 0 {
						value = 0;
					}

					value += 1;
				}
				Event::Ccw => {
					if value > 0 {
						value = 0;
					}

					value -= 1;
				}
			}

			if value.abs() == ENCODER_MODULO {
				value = 0;
				EVENTS.try_send(event).ok();
			}

			last_a = a;
			last_b = b;
		}
	}
}
