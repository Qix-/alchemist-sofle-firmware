#![no_std]
#![no_main]

pub mod i2c;
pub mod keyprobe;
pub mod led;
pub mod usb;

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_rp::{
	Peripherals, bind_interrupts,
	gpio::{Input, Level, OutputOpenDrain, Pull},
	i2c as rp_i2c,
	peripherals::{I2C0, I2C1, USB},
	usb as rp_usb,
};
use embassy_time::Timer;
use i2c::{I2cMasterConfig, I2cSlaveConfig, i2c_master_task, i2c_slave_task};
use keyprobe::{KeyprobeConfig, keyprobe_task};
use led::{LedConfig, led_task};
use panic_reset as _;

bind_interrupts!(pub struct Irqs {
	USBCTRL_IRQ => rp_usb::InterruptHandler<USB>;
	I2C0_IRQ => rp_i2c::InterruptHandler<I2C0>;
	I2C1_IRQ => rp_i2c::InterruptHandler<I2C1>;
});

#[rustfmt::skip]
static KEYMAP: [[[u8; 12]; 5]; 3] = [
	[
		[0x29, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2A],
		[0x2B, 0x14, 0x1A, 0x08, 0x15, 0x17, 0x1C, 0x18, 0x0C, 0x12, 0x13, 0x2E],
		[0xE0, 0x04, 0x16, 0x07, 0x09, 0x0A, 0x0B, 0x0D, 0x0E, 0x0F, 0x33, 0x34],
		[0xE1, 0x1D, 0x1B, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, 0x31],
		[0x4A, 0x4D, 0xE2, 0x2C, 0xE3, 0x00, 0x00, 0x28, 0x2C, 0x00, 0x4C, 0x00],
	],
	[
		[0x35, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x2D],
		[0x00, 0x44, 0x45, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x2F, 0x30],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x52, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50, 0x51, 0x4F, 0x00, 0x00],
		[0x4B, 0x4E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	],
	[
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
		[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	]
];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
	let p = embassy_rp::init(Default::default());

	// Test if we're the right side.
	//
	// We do this by attempting to write to the OLED device
	let right_side = {
		// The two pins to check are pin 25 and pin 12.
		let p = unsafe { Peripherals::steal() };
		let sda = OutputOpenDrain::new(p.PIN_25, Level::Low);
		let scl = OutputOpenDrain::new(p.PIN_12, Level::Low);
		Timer::after_millis(1).await;
		drop(sda);
		drop(scl);
		let p = unsafe { Peripherals::steal() };
		Timer::after_millis(2).await;
		let sda = Input::new(p.PIN_25, Pull::None);
		let scl = Input::new(p.PIN_12, Pull::None);
		Timer::after_millis(2).await;
		let r = sda.is_high() && scl.is_high();
		drop(sda);
		drop(scl);
		r
	};

	let led_config = LedConfig { pin_17: p.PIN_17 };

	spawner.spawn(led_task(led_config)).unwrap();

	let master_config = I2cMasterConfig {
		comms_link: !right_side,
		i2c1:       p.I2C1,
		pin_2:      p.PIN_2,
		pin_3:      p.PIN_3,
	};

	spawner.spawn(i2c_master_task(master_config)).unwrap();

	if right_side {
		let slave_config = I2cSlaveConfig {
			i2c0:   p.I2C0,
			pin_25: p.PIN_25,
			pin_12: p.PIN_12,
		};

		spawner.spawn(i2c_slave_task(slave_config)).unwrap();
	}

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

	// Clear the OLED
	i2c::OLED_CMD.signal(i2c::OledCommand::Clear);
	i2c::OLED_IDLE.wait().await;

	if right_side {
		i2c::OUTGOING.send(i2c::Packet::Ready).await;
	} else {
		i2c::OUTGOING.send(i2c::Packet::Reset).await;
		let i2c::Packet::Ready = i2c::INCOMING.receive().await else {
			panic!();
		};
	}

	let usb_config = usb::UsbConfig { usb_dev: p.USB };

	spawner.spawn(usb::usb_task(usb_config)).unwrap();

	let mut key_buffer: [u8; 6] = [0; 6];
	let mut layer_mask = 0;
	let mut modifiers = 0;

	loop {
		match select(keyprobe::EVENTS.receive(), i2c::INCOMING.receive()).await {
			Either::First(keyprobe::Event::Down(x, y)) => {
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
				i2c::OUTGOING.send(i2c::Packet::Down(x, y)).await;
			}
			Either::First(keyprobe::Event::Up(x, y)) => {
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
				i2c::OUTGOING.send(i2c::Packet::Up(x, y)).await;
			}
			Either::Second(i2c::Packet::Down(x, y)) => {
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
			}
			Either::Second(i2c::Packet::Up(x, y)) => {
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
			}
			Either::Second(i2c::Packet::Ready) => {
				panic!();
			}
			Either::Second(i2c::Packet::Reset) => {
				panic!();
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
