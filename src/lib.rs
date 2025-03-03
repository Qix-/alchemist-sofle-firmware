#![no_std]

pub mod encoder;
pub mod frames;
pub mod keyprobe;
pub mod led;
pub mod oled;
pub mod uart;
pub mod usb;

use embassy_executor::Spawner;
use embassy_futures::select::{Either3, select3};
use embassy_rp::{
	bind_interrupts, i2c as rp_i2c,
	peripherals::{I2C1, PIO0, USB},
	pio, usb as rp_usb,
};
use encoder::EncoderConfig;
use keyprobe::{KeyprobeConfig, keyprobe_task};
use led::{LedConfig, led_task};
use panic_reset as _;

bind_interrupts!(pub struct Irqs {
	USBCTRL_IRQ => rp_usb::InterruptHandler<USB>;
	I2C1_IRQ => rp_i2c::InterruptHandler<I2C1>;
	PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

#[rustfmt::skip]
static KEYMAP: [[[u8; 12]; 5]; 3] = [
	[
		[0x29, 0x1E, 0x1F, 0x20, 0x21, 0x22,    0x23, 0x24, 0x25, 0x26, 0x27, 0x2A],
		[0x2B, 0x14, 0x1A, 0x08, 0x15, 0x17,    0x1C, 0x18, 0x0C, 0x12, 0x13, 0x2E],
		[0xE0, 0x04, 0x16, 0x07, 0x09, 0x0A,    0x0B, 0x0D, 0x0E, 0x0F, 0x33, 0x34],
		[0xE1, 0x1D, 0x1B, 0x06, 0x19, 0x05,    0x11, 0x10, 0x36, 0x37, 0x38, 0x31],
		[0x4A, 0x4D, 0xE2, 0x2C, 0xE3, 0x00,    0x00, 0x28, 0x2C, 0x00, 0x00, 0x00],
	],
	[
		[0x35, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E,    0x3F, 0x40, 0x41, 0x42, 0x43, 0x2D],
		[0x00, 0x44, 0x45, 0x68, 0x69, 0x6A,    0x6B, 0x6C, 0x6D, 0x6E, 0x2F, 0x30],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x52, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x50, 0x51, 0x4F, 0x00, 0x00],
		[0x4B, 0x4E, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	],
	[
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x00, 0x00, 0x00, 0x4C],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00,    0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	]
];

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum BoardSide {
	Left,
	Right,
}

pub async fn run_alchemist(spawner: Spawner, side: BoardSide) -> ! {
	let p = embassy_rp::init(Default::default());

	let led_config = LedConfig { pin_17: p.PIN_17 };

	spawner.spawn(led_task(led_config)).unwrap();

	let keyprobe_config = KeyprobeConfig {
		pin_27: p.PIN_27,
		pin_26: p.PIN_26,
		pin_22: p.PIN_22,
		pin_20: p.PIN_20,
		pin_23: p.PIN_23,
		pin_21: p.PIN_21,
		pin_5:  p.PIN_5,
		pin_6:  p.PIN_6,
		pin_7:  p.PIN_7,
		pin_8:  p.PIN_8,
		pin_9:  p.PIN_9,
	};

	spawner.spawn(keyprobe_task(keyprobe_config)).unwrap();

	let encoder_config = EncoderConfig {
		pin_29: p.PIN_29,
		pin_28: p.PIN_28,
	};

	spawner
		.spawn(encoder::encoder_task(encoder_config))
		.unwrap();

	let usb_config = usb::UsbConfig { usb_dev: p.USB };

	spawner.spawn(usb::usb_task(usb_config)).unwrap();

	let oled_config = oled::OledConfig {
		i2c1:  p.I2C1,
		pin_3: p.PIN_3,
		pin_2: p.PIN_2,
	};

	spawner.spawn(oled::oled_task(oled_config)).unwrap();

	let uart_config = uart::UartConfig {
		pin_1: p.PIN_1,
		pin_4: p.PIN_4,
		side,
		pio0: p.PIO0,
	};

	spawner.spawn(uart::uart_task(uart_config)).unwrap();

	let mut key_buffer: [u8; 6] = [0; 6];
	let mut layer_mask = 0;
	let mut modifiers = 0;

	let right_side = side == BoardSide::Right;

	loop {
		match select3(
			keyprobe::EVENTS.receive(),
			uart::INCOMING.receive(),
			encoder::EVENTS.receive(),
		)
		.await
		{
			Either3::First(keyprobe::Event::Down(x, y)) => {
				dispatch_key(
					&mut key_buffer,
					&mut layer_mask,
					&mut modifiers,
					x,
					y,
					true,
					right_side,
					true,
				);
				uart::OUTGOING.send(uart::Packet::Down(x, y)).await;
				oled::spawn_star();
			}
			Either3::First(keyprobe::Event::Up(x, y)) => {
				dispatch_key(
					&mut key_buffer,
					&mut layer_mask,
					&mut modifiers,
					x,
					y,
					true,
					right_side,
					false,
				);
				uart::OUTGOING.send(uart::Packet::Up(x, y)).await;
				oled::spawn_star();
			}
			Either3::Second(uart::Packet::Down(x, y)) => {
				dispatch_key(
					&mut key_buffer,
					&mut layer_mask,
					&mut modifiers,
					x,
					y,
					false,
					right_side,
					true,
				);
				led::LED_STATE.signal(led::LedState::On);
				oled::spawn_star();
			}
			Either3::Second(uart::Packet::Up(x, y)) => {
				dispatch_key(
					&mut key_buffer,
					&mut layer_mask,
					&mut modifiers,
					x,
					y,
					false,
					right_side,
					false,
				);
				led::LED_STATE.signal(led::LedState::Off);
				oled::spawn_star();
			}
			Either3::Third(encoder::Event::Cw) => {
				if right_side {
					// Volume down
					usb::OUTGOING.try_send(usb::Event::Consumer(0xEA)).ok();
					uart::OUTGOING.try_send(uart::Packet::EncoderCcw).ok(); // (flipped)
				} else {
					// Next track
					usb::OUTGOING.try_send(usb::Event::Consumer(0xB5)).ok();
					uart::OUTGOING.try_send(uart::Packet::EncoderCw).ok();
				}
			}
			Either3::Third(encoder::Event::Ccw) => {
				if right_side {
					// Volume up
					usb::OUTGOING.try_send(usb::Event::Consumer(0xE9)).ok();
					uart::OUTGOING.try_send(uart::Packet::EncoderCw).ok(); // (flipped)
				} else {
					// Previous track
					usb::OUTGOING.try_send(usb::Event::Consumer(0xB6)).ok();
					uart::OUTGOING.try_send(uart::Packet::EncoderCcw).ok();
				}
			}
			Either3::Second(uart::Packet::EncoderCw) => {
				// NOTE: Reversed (since these are coming in from the other side)
				if right_side {
					// Next track
					usb::OUTGOING.try_send(usb::Event::Consumer(0xB5)).ok();
				} else {
					// Volume up
					usb::OUTGOING.try_send(usb::Event::Consumer(0xE9)).ok();
				}
			}
			Either3::Second(uart::Packet::EncoderCcw) => {
				// NOTE: Reversed (since these are coming in from the other side)
				if right_side {
					// Previous track
					usb::OUTGOING.try_send(usb::Event::Consumer(0xB6)).ok();
				} else {
					// Volume down
					usb::OUTGOING.try_send(usb::Event::Consumer(0xEA)).ok();
				}
			}
		}
	}
}

fn dispatch_key(
	key_buffer: &mut [u8; 6],
	layers: &mut u8,
	modifiers: &mut u8,
	x: u8,
	y: u8,
	from_us: bool,
	right_side: bool,
	down: bool,
) {
	if update_key_data(
		key_buffer, layers, modifiers, x, y, from_us, right_side, down,
	) {
		usb::OUTGOING
			.try_send(usb::Event::Update(*key_buffer, *modifiers))
			.ok();
	}
}

fn update_key_data(
	key_buffer: &mut [u8; 6],
	layers: &mut u8,
	modifiers: &mut u8,
	mut x: u8,
	y: u8,
	from_us: bool,
	right_side: bool,
	down: bool,
) -> bool {
	let is_right = from_us == right_side;

	if is_right {
		x = (5 - x.min(5)) + 6;
	}

	if x >= 12 || y >= 5 {
		return false;
	}

	if x == 9 && y == 4 {
		if down {
			*layers |= 1 << 0;
		} else {
			*layers &= !(1 << 0);
		}
		return false;
	}

	if x == 11 && y == 4 {
		if down {
			*layers |= 1 << 1;
		} else {
			*layers &= !(1 << 1);
		}
		return false;
	}

	// A bit of a hack - we handle media keys here, directly.
	if x == 5 && y == 4 {
		if down {
			usb::OUTGOING.try_send(usb::Event::Consumer(0xCD)).ok();
			led::LED_STATE.signal(led::LedState::BlinkFast);
		} else {
			led::LED_STATE.signal(led::LedState::Off);
		}
		return false;
	}
	if x == 6 && y == 4 {
		if down {
			usb::OUTGOING.try_send(usb::Event::Consumer(0xE2)).ok();
			led::LED_STATE.signal(led::LedState::BlinkSlow);
		} else {
			led::LED_STATE.signal(led::LedState::Off);
		}
		return false;
	}

	const LAYER_LUT: [u8; 4] = [0, 1, 2, 2];

	let layer = LAYER_LUT[(*layers & 0b11) as usize];
	let mut key = KEYMAP[layer as usize][y as usize][x as usize];

	if key == 0 {
		// Try to fall back to the base layer.
		key = KEYMAP[0][y as usize][x as usize];
	}

	if key == 0 {
		// Not mapped; ignore.
		return false;
	}

	if down {
		add_keycode(key_buffer, modifiers, key)
	} else {
		// Kind of weird, but we want to un-press any keys that are
		// mapped to the same key code on that key on any layer.
		let mut update = false;

		for layer in 0..KEYMAP.len() {
			let key = KEYMAP[layer as usize][y as usize][x as usize];
			update = update || remove_keycode(key_buffer, modifiers, key);
		}

		update
	}
}

fn add_keycode(key_buffer: &mut [u8; 6], modifiers: &mut u8, code: u8) -> bool {
	match code {
		0 => false,
		// Right Shift
		0xC6 | 0xE5 => {
			let r = ((*modifiers) & (1 << 5)) == 0;
			*modifiers |= 1 << 5;
			r
		}
		// Left shift
		0xE1 | 0xC5 => {
			let r = ((*modifiers) & (1 << 1)) == 0;
			*modifiers |= 1 << 1;
			r
		}
		// Right Control
		0xE4 => {
			let r = ((*modifiers) & (1 << 4)) == 0;
			*modifiers |= 1 << 4;
			r
		}
		// Left Control
		0xE0 => {
			let r = ((*modifiers) & (1 << 0)) == 0;
			*modifiers |= 1 << 0;
			r
		}
		// Right Alt
		0xE6 => {
			let r = ((*modifiers) & (1 << 6)) == 0;
			*modifiers |= 1 << 6;
			r
		}
		// Left Alt
		0xE2 => {
			let r = ((*modifiers) & (1 << 2)) == 0;
			*modifiers |= 1 << 2;
			r
		}
		// Right GUI
		0xE7 => {
			let r = ((*modifiers) & (1 << 7)) == 0;
			*modifiers |= 1 << 7;
			r
		}
		// Left GUI
		0xE3 => {
			let r = ((*modifiers) & (1 << 3)) == 0;
			*modifiers |= 1 << 3;
			r
		}
		_ => {
			for i in 0..6 {
				if key_buffer[i] == code {
					return false;
				}
			}

			for i in 0..6 {
				if key_buffer[i] == 0 {
					key_buffer[i] = code;
					return true;
				}
			}

			false
		}
	}
}

fn remove_keycode(key_buffer: &mut [u8; 6], modifiers: &mut u8, code: u8) -> bool {
	match code {
		0 => false,
		// Right Shift
		0xC6 | 0xE5 => {
			let r = ((*modifiers) & (1 << 5)) != 0;
			*modifiers &= !(1 << 5);
			r
		}
		// Left shift
		0xE1 | 0xC5 => {
			let r = ((*modifiers) & (1 << 1)) != 0;
			*modifiers &= !(1 << 1);
			r
		}
		// Right Control
		0xE4 => {
			let r = ((*modifiers) & (1 << 4)) != 0;
			*modifiers &= !(1 << 4);
			r
		}
		// Left Control
		0xE0 => {
			let r = ((*modifiers) & (1 << 0)) != 0;
			*modifiers &= !(1 << 0);
			r
		}
		// Right Alt
		0xE6 => {
			let r = ((*modifiers) & (1 << 6)) != 0;
			*modifiers &= !(1 << 6);
			r
		}
		// Left Alt
		0xE2 => {
			let r = ((*modifiers) & (1 << 2)) != 0;
			*modifiers &= !(1 << 2);
			r
		}
		// Right GUI
		0xE7 => {
			let r = ((*modifiers) & (1 << 7)) != 0;
			*modifiers &= !(1 << 7);
			r
		}
		// Left GUI
		0xE3 => {
			let r = ((*modifiers) & (1 << 3)) != 0;
			*modifiers &= !(1 << 3);
			r
		}
		_ => {
			let mut update = false;
			for i in 0..6 {
				if key_buffer[i] == code {
					key_buffer[i] = 0;
					update = true;
				}
			}
			update
		}
	}
}
