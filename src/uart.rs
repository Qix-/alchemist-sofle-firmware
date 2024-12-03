use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

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
