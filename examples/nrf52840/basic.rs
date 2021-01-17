#![no_std]
#![no_main]

use panic_halt as _;
use cortex_m::asm;
use cortex_m_rt::entry;
use nrf52840_hal as _;
use nrf52840_hal::{
    gpio::{p0::Parts},
    pac::Peripherals,
    twim::*
};

use neotrellis_rgb::{ Neotrellis, NeotrellisEventType };

fn delay(microseconds: u32) {
    asm::delay(64 * microseconds);
}

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take().unwrap();

    // Initialize our I2C
    let p0 = Parts::new(peripherals.P0);
    let scl = p0.p0_11.into_floating_input().degrade();
    let sda = p0.p0_12.into_floating_input().degrade();
    let twim = Twim::new(
        peripherals.TWIM0,
        Pins { scl, sda },
        nrf52840_hal::twim::Frequency::K100
    );

    let mut nt = Neotrellis::new(twim, delay, None);
    nt.initialize().unwrap();

    loop {
        delay(20_000);

        let mut raw_events: [u8; 16] = [0; 16];
        for event in nt.key_event_iterate(&mut raw_events).unwrap() {
            match event.event_type {
                NeotrellisEventType::KeyPress =>
                    nt.set_led(event.key_index as u8, 106, 13, 173).unwrap(),
                NeotrellisEventType::KeyRelease =>
                    nt.set_led(event.key_index as u8, 0, 0, 0).unwrap()
            };
        }

        delay(500);
        nt.refresh_leds().unwrap();
    }
}
