use core::{
	convert::Infallible,
	sync::atomic::{AtomicBool, Ordering},
};

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_rp::{
	Peripheral, Peripherals,
	gpio::{Input, Level, Output, Pull},
	i2c::{self, Async, I2c, SclPin, SdaPin},
	i2c_slave::{self, Command, I2cSlave},
	interrupt::typelevel::Binding,
	peripherals::{I2C0, I2C1},
};
use embassy_sync::{
	blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Timer, with_timeout};

const FREQ: u32 = 100_000;

pub const LINK_ADDR: u16 = 0x32;
pub const OLED_ADDR: u16 = 0x3C;

pub const PACKET_SIZE: usize = 4;

pub static OUTGOING: Channel<CriticalSectionRawMutex, Packet, 128> = Channel::new();
pub static INCOMING: Channel<CriticalSectionRawMutex, Packet, 64> = Channel::new();

pub static OLED_CMD: Signal<CriticalSectionRawMutex, OledCommand> = Signal::new();
pub static OLED_IDLE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub static OLED_OK: AtomicBool = AtomicBool::new(true);

static OUTGOING_MULTIPLEX: Channel<CriticalSectionRawMutex, MultiplexedPacket, 128> =
	Channel::new();

enum MultiplexedPacket {
	Oled(OledCommand),
	I2c(Packet),
}

#[derive(Clone)]
pub enum OledCommand {
	Clear,
	Buffer(&'static [u8]),
	Debug,
	Init,
}

#[derive(Clone)]
pub enum Packet {
	Down(u8, u8),
	Up(u8, u8),
	EncoderCw,
	EncoderCcw,
	Noop,
}

impl Packet {
	pub fn serialize(&self, buf: &mut [u8; PACKET_SIZE]) -> usize {
		// NOTE: do NOT use 0 as a packet type!
		match self {
			Packet::Down(x, y) => {
				buf[0] = 1;
				buf[1] = *x;
				buf[2] = *y;
				3
			}
			Packet::Up(x, y) => {
				buf[0] = 2;
				buf[1] = *x;
				buf[2] = *y;
				3
			}
			Packet::EncoderCw => {
				buf[0] = 5;
				1
			}
			Packet::EncoderCcw => {
				buf[0] = 6;
				1
			}
			Packet::Noop => {
				buf[0] = 0xFF;
				1
			}
		}
	}

	pub fn deserialize(buf: &[u8]) -> Option<Self> {
		if buf.len() == 0 {
			return None;
		}

		// NOTE: do NOT use 0 as a packet type!
		match buf[0] {
			1 if buf.len() >= 3 => Some(Packet::Down(buf[1], buf[2])),
			2 if buf.len() >= 3 => Some(Packet::Up(buf[1], buf[2])),
			5 => Some(Packet::EncoderCw),
			6 => Some(Packet::EncoderCcw),
			0xFF => Some(Packet::Noop),
			_ => None,
		}
	}
}

#[embassy_executor::task]
pub async fn i2c_left_task() -> ! {
	let spawner = Spawner::for_current_executor().await;
	spawner.spawn(multiplex_outgoing()).unwrap();
	spawner.spawn(multiplex_oled()).unwrap();
	run_comms_i2c1().await;
}

#[embassy_executor::task]
pub async fn i2c_right_task() -> ! {
	let spawner = Spawner::for_current_executor().await;
	spawner.spawn(multiplex_outgoing()).unwrap();
	spawner.spawn(handle_local_oled()).unwrap();

	run_comms_i2c0().await;
}

#[embassy_executor::task]
async fn handle_local_oled() -> ! {
	let p = unsafe { Peripherals::steal() };
	let mut config = i2c::Config::default();
	config.frequency = FREQ;
	let mut i2c = I2c::new_async(p.I2C1, p.PIN_3, p.PIN_2, crate::Irqs, config);

	loop {
		let cmd = OLED_CMD.wait().await;
		i2c.handle_oled_command(&cmd).await.ok();
		OLED_IDLE.signal(());
	}
}

#[embassy_executor::task]
async fn multiplex_outgoing() -> ! {
	loop {
		let p = OUTGOING.receive().await;
		OUTGOING_MULTIPLEX.send(MultiplexedPacket::I2c(p)).await;
	}
}

#[embassy_executor::task]
async fn multiplex_oled() -> ! {
	loop {
		let cmd = OLED_CMD.wait().await;
		OUTGOING_MULTIPLEX.send(MultiplexedPacket::Oled(cmd)).await;
	}
}

async fn ssd1306_init(i2c: &mut I2c<'_, I2C1, i2c::Async>) -> bool {
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

async fn run_comms_i2c1() -> ! {
	loop {
		run_comms(crate::Irqs, || {
			// SAFETY: None. To heck with the law.
			let p = unsafe { Peripherals::steal() };
			(p.I2C1, p.PIN_3, p.PIN_2)
		})
		.await
		.ok();
	}
}

async fn run_comms_i2c0() -> ! {
	loop {
		run_comms(crate::Irqs, || {
			// SAFETY: None. To heck with the law.
			let p = unsafe { Peripherals::steal() };
			(p.I2C0, p.PIN_25, p.PIN_12)
		})
		.await
		.ok();
	}
}

async fn run_comms<'d, F, T, Dev, SclP, SdaP, Scl, Sda, Irqs>(
	irqs: Irqs,
	get_device_pins: F,
) -> Result<Infallible, ()>
where
	T: i2c::Instance + 'd,
	F: Fn() -> (Dev, Scl, Sda),
	Dev: Peripheral<P = T> + 'd,
	SdaP: SdaPin<T>,
	SclP: SclPin<T>,
	Sda: Peripheral<P = SdaP> + 'd,
	Scl: Peripheral<P = SclP> + 'd,
	Irqs: Binding<T::Interrupt, i2c::InterruptHandler<T>> + Copy,
	I2c<'d, T, i2c::Async>: HandleOledCommand,
{
	let mut buf = [0_u8; PACKET_SIZE];

	let slave_config = {
		let mut d = i2c_slave::Config::default();
		d.addr = LINK_ADDR;
		d.general_call = false;
		d
	};

	let master_config = {
		let mut d = i2c::Config::default();
		d.frequency = FREQ;
		d
	};

	let p = unsafe { Peripherals::steal() };
	let mut dbgpin = Output::new(p.PIN_15, Level::High);

	loop {
		// Wait for incoming or outgoing call
		{
			let (_, _, sda_pin) = get_device_pins();

			let mut input = Input::new(sda_pin, Pull::Down);

			let on_fall = async {
				loop {
					input.wait_for_low().await;
					Timer::after_micros(20).await;
					if input.is_low() {
						break;
					}
				}
			};
			let on_outgoing = OUTGOING_MULTIPLEX.receive();

			let r = select(on_fall, on_outgoing).await;

			drop(input);

			match r {
				// The other side is calling
				Either::First(_) => {
					// Answer the call.
					dbgpin.set_low();
					let (_, _, sda_pin) = get_device_pins();
					let output = Output::new(sda_pin, Level::High);
					Timer::after_micros(10).await;
					drop(output);
					dbgpin.set_high();

					// Configure an I2C Slave.
					let (dev, scl_pin, sda_pin) = get_device_pins();

					let mut slave = I2cSlave::new(dev, scl_pin, sda_pin, irqs, slave_config);

					let command = match with_timeout(
						Duration::from_millis(3),
						slave.listen(&mut buf),
					)
					.await
					{
						Err(_) | Ok(Err(_)) => {
							slave.reset();
							return Err(());
						}
						Ok(Ok(command)) => command,
					};

					match command {
						Command::Write(sz) => {
							if let Some(msg) = Packet::deserialize(&buf[..sz]) {
								if !matches!(msg, Packet::Noop) {
									INCOMING.try_send(msg).ok();
								}
							}
						}
						_ => {
							return Err(());
						}
					}
				}
				// We need to make a call.
				Either::Second(cmd) => {
					let mut ok = false;
					for _ in 0..3 {
						// Signal call.
						dbgpin.set_low();
						let (_, _, sda_pin) = get_device_pins();
						let mut dial_tone = Input::new(sda_pin, Pull::Down);
						Timer::after_micros(5).await;
						let dial_result =
							with_timeout(Duration::from_micros(300), dial_tone.wait_for_high())
								.await;
						drop(dial_tone);
						Timer::after_micros(5).await;
						dbgpin.set_high();
						if dial_result.is_err() {
							continue;
						}

						let (dev, scl_pin, sda_pin) = get_device_pins();

						let mut i2c = I2c::new_async(dev, scl_pin, sda_pin, irqs, master_config);

						Timer::after_micros(40).await;

						match cmd {
							MultiplexedPacket::I2c(ref cmd) => {
								let sz = cmd.serialize(&mut buf);
								ok = i2c
									.write_async(LINK_ADDR, buf.iter().take(sz).copied())
									.await
									.is_ok();
							}
							MultiplexedPacket::Oled(ref cmd) => {
								if OLED_OK.load(Ordering::Relaxed) {
									if i2c.handle_oled_command(cmd).await.is_err() {
										OLED_OK.store(false, Ordering::Relaxed);
									}
								}
								OLED_IDLE.signal(());

								// It was an OLED command; the other half is still waiting for a response.
								// Send a NOOP to signal that we're done.
								let sz = Packet::Noop.serialize(&mut buf);

								i2c.write_async(LINK_ADDR, buf.iter().take(sz).copied())
									.await
									.ok();

								// Best effort basis.
								ok = true;
							}
						}

						if ok {
							break;
						}
					}

					if !ok {
						return Err(());
					}
				}
			}
		}
	}
}

trait HandleOledCommand {
	async fn handle_oled_command(&mut self, cmd: &OledCommand) -> Result<(), ()>;
}

impl HandleOledCommand for I2c<'_, I2C0, Async> {
	async fn handle_oled_command(&mut self, _cmd: &OledCommand) -> Result<(), ()> {
		panic!();
	}
}

impl HandleOledCommand for I2c<'_, I2C1, Async> {
	async fn handle_oled_command(&mut self, cmd: &OledCommand) -> Result<(), ()> {
		match cmd {
			OledCommand::Clear => {
				self.write_async(
					OLED_ADDR,
					[0b0100_0000]
						.into_iter()
						.chain([0].into_iter().cycle().take(128 * 32 / 8)),
				)
				.await
				.map_err(|_| ())?;
			}
			OledCommand::Debug => {
				self.write_async(
					OLED_ADDR,
					[0b0100_0000].into_iter().chain(
						[0b01010101]
							.into_iter()
							.cycle()
							.take(4)
							.chain([0b10101010].into_iter().cycle().take(4))
							.cycle()
							.take(128 * 32 / 8),
					),
				)
				.await
				.map_err(|_| ())?;
			}
			OledCommand::Buffer(oled_buffer) => {
				self.write_async(
					OLED_ADDR,
					[0b0100_0000]
						.into_iter()
						.chain(oled_buffer.iter().take(128 * 32 / 8).copied()),
				)
				.await
				.map_err(|_| ())?;
			}
			OledCommand::Init => {
				if !ssd1306_init(self).await {
					return Err(());
				}
			}
		}

		Ok(())
	}
}
