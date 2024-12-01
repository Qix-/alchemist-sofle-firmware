use embassy_futures::select::{Either3, select3};
use embassy_rp::{
	i2c::{self, Async, I2c},
	i2c_slave::{self, Command, I2cSlave},
	peripherals::{I2C0, I2C1, PIN_2, PIN_3, PIN_12, PIN_25},
};
use embassy_sync::{
	blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Timer, with_timeout};

pub const LINK_ADDR: u16 = 0x32;
pub const OLED_ADDR: u16 = 0x3C;

pub const PACKET_SIZE: usize = 4;

pub static OUTGOING: Channel<CriticalSectionRawMutex, Packet, 64> = Channel::new();
pub static INCOMING: Channel<CriticalSectionRawMutex, Packet, 64> = Channel::new();
pub static OLED_CMD: Signal<CriticalSectionRawMutex, OledCommand> = Signal::new();
pub static OLED_IDLE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[derive(Clone)]
pub enum OledCommand {
	Clear,
	Buffer(&'static [u8]),
	Debug,
}

#[derive(Clone)]
pub enum Packet {
	// Reset,
	// Ready,
	Down(u8, u8),
	Up(u8, u8),
	EncoderCw,
	EncoderCcw,
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
			// Packet::Reset => {
			// 	buf[0] = 3;
			// 	1
			//}
			// Packet::Ready => {
			// 	buf[0] = 4;
			// 	1
			//}
			Packet::EncoderCw => {
				buf[0] = 5;
				1
			}
			Packet::EncoderCcw => {
				buf[0] = 6;
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
			// 3 if buf.len() >= 1 => Some(Packet::Reset),
			// 4 if buf.len() >= 1 => Some(Packet::Ready),
			5 if buf.len() >= 1 => Some(Packet::EncoderCw),
			6 if buf.len() >= 1 => Some(Packet::EncoderCcw),
			_ => None,
		}
	}
}

pub struct I2cSlaveConfig {
	pub i2c0:   I2C0,
	pub pin_25: PIN_25,
	pub pin_12: PIN_12,
}

pub struct I2cMasterConfig {
	pub i2c1:       I2C1,
	pub pin_2:      PIN_2,
	pub pin_3:      PIN_3,
	pub comms_link: bool,
}

#[embassy_executor::task]
pub async fn i2c_master_task(config: I2cMasterConfig) {
	let mut i2c_config = i2c::Config::default();
	i2c_config.frequency = 200_000;

	let mut i2c = I2c::new_async(
		config.i2c1,
		config.pin_3,
		config.pin_2,
		crate::Irqs,
		i2c_config,
	);

	let oled_ok = ssd1306_init(&mut i2c).await;

	let mut buf = [0u8; PACKET_SIZE];

	loop {
		let oled_cmd = if config.comms_link {
			let r = select3(
				OUTGOING.receive(),
				Timer::after_nanos(100000),
				OLED_CMD.wait(),
			)
			.await;

			match r {
				Either3::First(msg) => {
					let sz = msg.serialize(&mut buf);
					with_timeout(
						Duration::from_millis(100),
						i2c.write_async(LINK_ADDR, buf.iter().take(sz).copied()),
					)
					.await
					.ok();
					continue;
				}
				Either3::Second(_) => {
					if let Ok(Ok(_)) = with_timeout(
						Duration::from_millis(100),
						i2c.read_async(LINK_ADDR, &mut buf),
					)
					.await
					{
						if let Some(msg) = Packet::deserialize(&buf) {
							INCOMING.try_send(msg).ok();
						}
					}

					continue;
				}
				Either3::Third(cmd) => cmd,
			}
		} else {
			OLED_CMD.wait().await
		};

		if oled_ok {
			match oled_cmd {
				OledCommand::Clear => {
					i2c.write_async(
						OLED_ADDR,
						[0b0100_0000]
							.into_iter()
							.chain([0].into_iter().cycle().take(128 * 32 / 8)),
					)
					.await
					.unwrap();
				}
				OledCommand::Debug => {
					i2c.write_async(
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
					.unwrap();
				}
				OledCommand::Buffer(oled_buffer) => {
					i2c.write_async(
						OLED_ADDR,
						[0b0100_0000]
							.into_iter()
							.chain(oled_buffer.iter().take(128 * 32 / 8).copied()),
					)
					.await
					.unwrap();
				}
			}
		}

		Timer::after_nanos(10000).await;

		// Tell the OLED task that we're done with the buffer.
		OLED_IDLE.signal(());
	}
}

#[embassy_executor::task]
pub async fn i2c_slave_task(config: I2cSlaveConfig) {
	let mut i2c_config = i2c_slave::Config::default();
	i2c_config.addr = LINK_ADDR;
	i2c_config.general_call = true;
	let mut i2c = I2cSlave::new(
		config.i2c0,
		config.pin_25,
		config.pin_12,
		crate::Irqs,
		i2c_config,
	);

	let mut buf = [0u8; PACKET_SIZE];

	loop {
		let cmd = i2c.listen(&mut buf).await.unwrap();

		let respond = match cmd {
			Command::Read => true,
			Command::WriteRead(sz) => {
				if let Some(msg) = Packet::deserialize(&buf[0..sz]) {
					INCOMING.send(msg).await;
				}
				true
			}
			Command::Write(sz) => {
				if let Some(msg) = Packet::deserialize(&buf[0..sz]) {
					INCOMING.send(msg).await;
				}
				false
			}
			Command::GeneralCall(_) => false,
		};

		if respond {
			if let Ok(msg) = OUTGOING.try_receive() {
				let sz = msg.serialize(&mut buf);
				i2c.respond_and_fill(&buf[..sz], 0).await.unwrap();
			} else {
				i2c.respond_till_stop(0).await.unwrap();
			}
		}
	}
}

async fn ssd1306_init(i2c: &mut I2c<'_, I2C1, Async>) -> bool {
	macro_rules! write_cmd {
		($($data:expr),*) => {
			i2c.blocking_write(OLED_ADDR, &[0x00, $($data),*]).map_err(|_| ())?;
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
