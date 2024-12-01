#[allow(dead_code)]
pub struct Frame {
	pub width:  usize,
	pub height: usize,
	pub mask:   &'static [u8],
	pub add:    &'static [u8],
}

macro_rules! frames {
	($id:ident = [$($frame_name:ident),* $(,)?]) => {
		$(
			pub mod $frame_name;
		)*

		#[allow(dead_code)]
		pub const $id: &[Frame] = &[
			$(
				Frame {
					width: $frame_name::WIDTH,
					height: $frame_name::HEIGHT,
					mask: $frame_name::MASK,
					add: $frame_name::ADD,
				},
			)*
		];
	};
}

frames!(
	SIGILS = [
		sigil_1, sigil_2, sigil_3, sigil_4, sigil_5, sigil_6, sigil_7, sigil_8, sigil_9, sigil_10
	]
);
frames!(BANNER = [banner_1, banner_2]);
frames!(BODY = [body_1, body_2]);
