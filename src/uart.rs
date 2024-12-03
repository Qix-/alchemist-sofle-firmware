use embassy_futures::join::join;
use embassy_rp::{
	peripherals::{PIN_1, PIN_4, PIO0},
	pio,
	pio_programs::uart::{PioUartRx, PioUartRxProgram, PioUartTx, PioUartTxProgram},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embedded_io_async::{Read, Write};

use crate::BoardSide;

pub static INCOMING: Channel<CriticalSectionRawMutex, Packet, 64> = Channel::new();
pub static OUTGOING: Channel<CriticalSectionRawMutex, Packet, 64> = Channel::new();

pub const PACKET_SIZE: usize = 3;

#[derive(Clone)]
pub enum Packet {
	Down(u8, u8),
	Up(u8, u8),
	EncoderCw,
	EncoderCcw,
}

impl Packet {
	pub fn serialize(&self, buf: &mut [u8; PACKET_SIZE]) {
		match self {
			Packet::Down(x, y) => {
				buf[0] = 1;
				buf[1] = *x;
				buf[2] = *y;
			}
			Packet::Up(x, y) => {
				buf[0] = 2;
				buf[1] = *x;
				buf[2] = *y;
			}
			Packet::EncoderCw => {
				buf[0] = 5;
				buf[1] = 0;
				buf[2] = 0;
			}
			Packet::EncoderCcw => {
				buf[0] = 6;
				buf[1] = 0;
				buf[2] = 0;
			}
		}
	}

	pub fn deserialize(buf: [u8; PACKET_SIZE]) -> Option<Self> {
		match buf[0] {
			1 => Some(Packet::Down(buf[1], buf[2])),
			2 => Some(Packet::Up(buf[1], buf[2])),
			5 => Some(Packet::EncoderCw),
			6 => Some(Packet::EncoderCcw),
			_ => None,
		}
	}
}

pub struct UartConfig {
	pub pio0:  PIO0,
	pub pin_1: PIN_1,
	pub pin_4: PIN_4,
	pub side:  BoardSide,
}

#[embassy_executor::task]
pub async fn uart_task(config: UartConfig) -> ! {
	let pio::Pio {
		mut common,
		sm0,
		sm1,
		..
	} = pio::Pio::new(config.pio0, crate::Irqs);

	match config.side {
		BoardSide::Left => {
			let tx_program = PioUartTxProgram::new(&mut common);
			let mut uart_tx = PioUartTx::new(9600, &mut common, sm0, config.pin_1, &tx_program);

			let rx_program = PioUartRxProgram::new(&mut common);
			let mut uart_rx = PioUartRx::new(9600, &mut common, sm1, config.pin_4, &rx_program);

			join(uart_read(&mut uart_rx), uart_write(&mut uart_tx)).await;
		}
		BoardSide::Right => {
			let tx_program = PioUartTxProgram::new(&mut common);
			let mut uart_tx = PioUartTx::new(9600, &mut common, sm0, config.pin_4, &tx_program);

			let rx_program = PioUartRxProgram::new(&mut common);
			let mut uart_rx = PioUartRx::new(9600, &mut common, sm1, config.pin_1, &rx_program);

			join(uart_read(&mut uart_rx), uart_write(&mut uart_tx)).await;
		}
	}

	unreachable!();
}

async fn uart_read<const S: usize>(uart_rx: &mut PioUartRx<'_, PIO0, S>) -> ! {
	let mut buf = [0; PACKET_SIZE];
	loop {
		uart_rx.read_exact(&mut buf).await.unwrap();
		if let Some(packet) = Packet::deserialize(buf) {
			INCOMING.send(packet).await;
		}
	}
}

async fn uart_write<const S: usize>(uart_rx: &mut PioUartTx<'_, PIO0, S>) -> ! {
	let mut buf = [0; PACKET_SIZE];
	loop {
		let packet = OUTGOING.receive().await;
		packet.serialize(&mut buf);
		uart_rx.write_all(&buf).await.unwrap();
	}
}
