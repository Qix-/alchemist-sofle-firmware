use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::join::join;
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_usb::{
	Builder, Config, Handler,
	class::hid::{HidReaderWriter, ReportId, RequestHandler, State},
	control::OutResponse,
};
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};

pub static OUTGOING: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();

#[derive(Clone)]
pub enum Event {
	Update([u8; 6], u8),
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
	let mut request_handler = MyRequestHandler {};
	let mut device_handler = MyDeviceHandler::new();

	let mut state = State::new();

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
		poll_ms:           2,
		max_packet_size:   64,
	};
	let hid = HidReaderWriter::<_, 1, 8>::new(&mut builder, &mut state, config);

	let mut usb = builder.build();

	let usb_fut = usb.run();

	let (reader, mut writer) = hid.split();

	loop {
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

						match writer.write_serialize(&report).await {
							Ok(()) => {}
							Err(_) => panic!(),
						};
					}
				}
			}
		};

		let out_fut = async {
			reader.run(false, &mut request_handler).await;
		};

		join(usb_fut, join(in_fut, out_fut)).await;
		panic!();
	}
}

struct MyRequestHandler {}

impl RequestHandler for MyRequestHandler {
	fn get_report(&mut self, _id: ReportId, _buf: &mut [u8]) -> Option<usize> {
		None
	}

	fn set_report(&mut self, _id: ReportId, _data: &[u8]) -> OutResponse {
		OutResponse::Accepted
	}

	fn set_idle_ms(&mut self, _id: Option<ReportId>, _dur: u32) {}

	fn get_idle_ms(&mut self, _id: Option<ReportId>) -> Option<u32> {
		None
	}
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
