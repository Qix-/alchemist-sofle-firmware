use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::join::join;
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_usb::{
	Builder, Config, Handler,
	class::hid::{HidWriter, State},
};
use usbd_hid::descriptor::{KeyboardReport, MediaKeyboardReport, SerializedDescriptor};

pub static OUTGOING: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();

#[derive(Clone)]
pub enum Event {
	Update([u8; 6], u8),
	Consumer(u16),
}

pub struct UsbConfig {
	pub usb_dev: USB,
}

#[embassy_executor::task]
pub async fn usb_task(config: UsbConfig) -> ! {
	let driver = Driver::new(config.usb_dev, crate::Irqs);

	let mut config = Config::new(0x1337, 0xA1C4);
	config.manufacturer = Some("Junon");
	config.product = Some("Alchemist");
	config.serial_number = Some("ERROR_UNPLUG_YOUR_EXISTENCE");
	config.max_power = 100;
	config.max_packet_size_0 = 64;

	let mut config_descriptor = [0; 256];
	let mut bos_descriptor = [0; 256];
	// Microsoft compatible descriptor
	let mut msos_descriptor = [0; 256];
	let mut control_buf = [0; 64];
	let mut device_handler = MyDeviceHandler::new();

	let mut state = State::new();
	let mut media_state = State::new();

	let mut builder = Builder::new(
		driver,
		config,
		&mut config_descriptor,
		&mut bos_descriptor,
		&mut msos_descriptor,
		&mut control_buf,
	);

	builder.handler(&mut device_handler);

	let config = embassy_usb::class::hid::Config {
		report_descriptor: KeyboardReport::desc(),
		request_handler:   None,
		poll_ms:           5,
		max_packet_size:   64,
	};
	let mut hid = HidWriter::<_, 16>::new(&mut builder, &mut state, config);

	let config = embassy_usb::class::hid::Config {
		report_descriptor: MediaKeyboardReport::desc(),
		request_handler:   None,
		poll_ms:           2,
		max_packet_size:   64,
	};
	let mut media_hid = HidWriter::<_, 4>::new(&mut builder, &mut media_state, config);

	let mut usb = builder.build();

	let usb_fut = usb.run();

	let in_fut = async {
		loop {
			let event = OUTGOING.receive().await;

			match event {
				Event::Update(keycodes, modifier) => {
					let report = KeyboardReport {
						keycodes,
						modifier,
						leds: 0,
						reserved: 0,
					};

					match hid.write_serialize(&report).await {
						Ok(()) => {}
						Err(_) => panic!(),
					};
				}
				Event::Consumer(usage_id) => {
					let report = MediaKeyboardReport { usage_id };

					match media_hid.write_serialize(&report).await {
						Ok(()) => {}
						Err(_) => panic!(),
					};

					let report = MediaKeyboardReport { usage_id: 0 };

					match media_hid.write_serialize(&report).await {
						Ok(()) => {}
						Err(_) => panic!(),
					};
				}
			}
		}
	};

	join(usb_fut, in_fut).await;

	panic!();
}

struct MyDeviceHandler {
	configured: AtomicBool,
}

impl MyDeviceHandler {
	fn new() -> Self {
		MyDeviceHandler {
			configured: AtomicBool::new(false),
		}
	}
}

impl Handler for MyDeviceHandler {
	fn enabled(&mut self, _enabled: bool) {
		self.configured.store(false, Ordering::Relaxed);
	}

	fn reset(&mut self) {
		self.configured.store(false, Ordering::Relaxed);
	}

	fn addressed(&mut self, _addr: u8) {
		self.configured.store(false, Ordering::Relaxed);
	}

	fn configured(&mut self, configured: bool) {
		self.configured.store(configured, Ordering::Relaxed);
	}
}
