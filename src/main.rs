#![no_std]
#![no_main]

pub mod i2c;
pub mod keyprobe;
pub mod led;

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_rp::{
	Peripherals, bind_interrupts,
	gpio::{Input, Level, Output, OutputOpenDrain, Pin, Pull},
	i2c::{self as rp_i2c, I2c},
	peripherals::{
		I2C0, I2C1, PIN_2, PIN_3, PIN_5, PIN_6, PIN_7, PIN_8, PIN_9, PIN_12, PIN_20, PIN_21,
		PIN_22, PIN_23, PIN_25, PIN_26, PIN_27, USB,
	},
	usb::{self, Driver},
};
use embassy_sync::{
	blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex},
	pubsub::{PubSubChannel, Publisher, Subscriber},
};
use embassy_time::Timer;
use embassy_usb::{
	Builder, Config, Handler,
	class::hid::{HidReaderWriter, ReportId, RequestHandler, State},
	control::OutResponse,
};
use i2c::{I2cMasterConfig, I2cSlaveConfig, i2c_master_task, i2c_slave_task};
use keyprobe::{KeyprobeConfig, keyprobe_task};
use led::{LED_STATE, LedConfig, led_task};
use panic_reset as _;
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};

bind_interrupts!(pub struct Irqs {
	USBCTRL_IRQ => usb::InterruptHandler<USB>;
	I2C0_IRQ => rp_i2c::InterruptHandler<I2C0>;
	I2C1_IRQ => rp_i2c::InterruptHandler<I2C1>;
});

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
	// i2c::OLED_CMD.signal(i2c::OledCommand::Clear);
	// i2c::OLED_IDLE.wait().await;

	if right_side {
		i2c::OUTGOING.send(i2c::Packet::Ready).await;
	} else {
		i2c::OUTGOING.send(i2c::Packet::Reset).await;
		let i2c::Packet::Ready = i2c::INCOMING.receive().await else {
			panic!();
		};
	}

	loop {
		match select(keyprobe::EVENTS.receive(), i2c::INCOMING.receive()).await {
			Either::First(keyprobe::Event::Down(x, y)) => {
				i2c::OUTGOING.send(i2c::Packet::Down(x, y)).await;
			}
			Either::First(keyprobe::Event::Up(x, y)) => {
				i2c::OUTGOING.send(i2c::Packet::Up(x, y)).await;
			}
			Either::Second(i2c::Packet::Down(_, _)) => {
				led::LED_STATE.signal(led::LedState::On);
			}
			Either::Second(i2c::Packet::Up(_, _)) => {
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

	// Create the driver, from the HAL.
	// let driver = Driver::new(p.USB, Irqs);
	//
	// Create embassy-usb Config
	// let mut config = Config::new(0xC0DE, 0xCAFE);
	// config.manufacturer = Some("Embassy");
	// config.product = Some("HID keyboard example");
	// config.serial_number = Some("12345678");
	// config.max_power = 100;
	// config.max_packet_size_0 = 64;
	//
	// Create embassy-usb DeviceBuilder using the driver and config.
	// It needs some buffers for building the descriptors.
	// let mut config_descriptor = [0; 256];
	// let mut bos_descriptor = [0; 256];
	// You can also add a Microsoft OS descriptor.
	// let mut msos_descriptor = [0; 256];
	// let mut control_buf = [0; 64];
	// let mut request_handler = MyRequestHandler {};
	// let mut device_handler = MyDeviceHandler::new();
	//
	// let mut state = State::new();
	//
	// let mut builder = Builder::new(
	// driver,
	// config,
	// &mut config_descriptor,
	// &mut bos_descriptor,
	// &mut msos_descriptor,
	// &mut control_buf,
	// );
	//
	// builder.handler(&mut device_handler);
	//
	// Create classes on the builder.
	// let config = embassy_usb::class::hid::Config {
	// report_descriptor: KeyboardReport::desc(),
	// request_handler:   None,
	// poll_ms:           60,
	// max_packet_size:   64,
	// };
	// let hid = HidReaderWriter::<_, 1, 8>::new(&mut builder, &mut state, config);
	//
	// Build the builder.
	// let mut usb = builder.build();
	//
	// Run the USB device.
	// let usb_fut = usb.run();
	//
	// Set up the signal pin that will be used to trigger the keyboard.
	// let mut signal_pin = Input::new(p.PIN_16, Pull::None);
	//
	// Enable the schmitt trigger to slightly debounce.
	// signal_pin.set_schmitt(true);
	//
	// let (reader, mut writer) = hid.split();
	//
	// Do stuff with the class!
	// let in_fut = async {
	// loop {
	// signal_pin.wait_for_high().await;
	// Create a report with the A key pressed. (no shift modifier)
	// let report = KeyboardReport {
	// keycodes: [4, 0, 0, 0, 0, 0],
	// leds:     0,
	// modifier: 0,
	// reserved: 0,
	// };
	// Send the report.
	// match writer.write_serialize(&report).await {
	// Ok(()) => {}
	// Err(e) => warn!("Failed to send report: {:?}", e),
	// };
	// signal_pin.wait_for_low().await;
	// let report = KeyboardReport {
	// keycodes: [0, 0, 0, 0, 0, 0],
	// leds:     0,
	// modifier: 0,
	// reserved: 0,
	// };
	// match writer.write_serialize(&report).await {
	// Ok(()) => {}
	// Err(e) => warn!("Failed to send report: {:?}", e),
	// };
	// }
	// };
	//
	// let out_fut = async {
	// reader.run(false, &mut request_handler).await;
	// };
	//
	// Run everything concurrently.
	// If we had made everything `'static` above instead, we could do this using separate tasks instead.
	// join(usb_fut, join(in_fut, out_fut)).await;
}

struct MyRequestHandler {}

impl RequestHandler for MyRequestHandler {
	fn get_report(&mut self, id: ReportId, _buf: &mut [u8]) -> Option<usize> {
		None
	}

	fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
		OutResponse::Accepted
	}

	fn set_idle_ms(&mut self, id: Option<ReportId>, dur: u32) {}

	fn get_idle_ms(&mut self, id: Option<ReportId>) -> Option<u32> {
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
	fn enabled(&mut self, enabled: bool) {
		self.configured.store(false, Ordering::Relaxed);
	}

	fn reset(&mut self) {
		self.configured.store(false, Ordering::Relaxed);
	}

	fn addressed(&mut self, addr: u8) {
		self.configured.store(false, Ordering::Relaxed);
	}

	fn configured(&mut self, configured: bool) {
		self.configured.store(configured, Ordering::Relaxed);
	}
}
