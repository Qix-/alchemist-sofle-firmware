use embassy_futures::select::{Either, select};
use embassy_rp::{
	gpio::{Level, Output},
	peripherals::PIN_17,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;

pub static LED_STATE: Signal<CriticalSectionRawMutex, LedState> = Signal::new();

#[derive(Clone, Copy, Default)]
#[repr(u8)]
#[allow(dead_code)]
pub enum LedState {
	#[default]
	Off       = 0,
	On        = 1,
	BlinkSlow = 2,
	BlinkFast = 3,
}

pub struct LedConfig {
	pub pin_17: PIN_17,
}

#[embassy_executor::task]
pub async fn led_task(config: LedConfig) {
	let mut led_pin = Output::new(config.pin_17, Level::Low);

	let mut state = LedState::Off;
	loop {
		state = match state {
			LedState::On => {
				led_pin.set_high();
				LED_STATE.wait().await
			}
			LedState::Off => {
				led_pin.set_low();
				LED_STATE.wait().await
			}
			LedState::BlinkSlow => {
				let Either::Second(r) =
					select(led_blink_slow(&mut led_pin), LED_STATE.wait()).await;
				r
			}
			LedState::BlinkFast => {
				let Either::Second(r) =
					select(led_blink_fast(&mut led_pin), LED_STATE.wait()).await;
				r
			}
		}
	}
}

async fn led_blink_fast(led_pin: &mut Output<'_>) -> ! {
	loop {
		led_pin.set_high();
		Timer::after_millis(100).await;
		led_pin.set_low();
		Timer::after_millis(100).await;
	}
}

async fn led_blink_slow(led_pin: &mut Output<'_>) -> ! {
	loop {
		led_pin.set_high();
		Timer::after_millis(500).await;
		led_pin.set_low();
		Timer::after_millis(500).await;
	}
}
