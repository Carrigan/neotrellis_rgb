//! Provides an embedded-hal I2C driver for the [Adafruit NeoTrellis RGB](https://www.adafruit.com/product/3954)
//! key grid. This code is derived from the [Adafruit Seesaw](https://github.com/adafruit/Adafruit_Seesaw)
//! repository but specialized for the use case of a
//! [simple, singular NeoTrellis configuration](https://github.com/adafruit/Adafruit_Seesaw/blob/master/examples/NeoTrellis/basic/basic.ino).
//!
//! Having multiple NeoTrellis boards linked is currently unsupported due to
//! the single Neotrellis structure consuming the I2C structure. If you would
//! like this feature, pull requests are encouraged.
//!
//! The Adafruit Seesaw board requires delays between writes and reads. The
//! application that this library was intended for is a synthesizer which
//! cannot support such delays synchronously. For many of the Neotrellis functions,
//! there is a more convenient write/read combo that guarantees the timing requirements
//! as well as independent write and read functions that do not enforce a given delay.

#![no_std]
use embedded_hal::blocking::i2c::{Write, Read};

const STATUS_REGISTER_BASE: u8 = 0x00;
const KEYPAD_REGISTER_BASE: u8 = 0x10;
const NEOPIXEL_REGISTER_BASE: u8 = 0x0E;

enum StatusRegister {
    HardwareId = 0x01
}

enum KeypadRegister {
    Event = 0x01,
    Count = 0x04,
    Fifo = 0x10
}

enum NeopixelRegister {
    Pin = 0x01,
    Speed = 0x02,
    BufferLength = 0x03,
    Buffer = 0x04,
    Show = 0x05
}

/// Main structure used for interfacing with the Neotrellis RGB board.
///
/// Example usage: turning an LED white while it is pressed:
/// ```
/// let mut nt = Neotrellis::new(i2c, delay_us, None);
/// nt.initialize().unwrap();
///
/// loop {
///     delay(20_000);
///
///     let mut raw_events: [u8; 16] = [0; 16];
///     for event in nt.key_event_iterate(&mut raw_events).unwrap() {
///         match event.event_type {
///             NeotrellisEventType::KeyPress =>
///                 nt.set_led(event.key_index as u8, 106, 13, 173).unwrap(),
///             NeotrellisEventType::KeyRelease =>
///                 nt.set_led(event.key_index as u8, 0, 0, 0).unwrap()
///         };
///     }
///
///     delay(500);
///     nt.refresh_leds().unwrap();
/// }
/// ```
pub struct Neotrellis<I2C> {
    i2c: I2C,
    address: u8,
    delay_function: fn(u32)
}

fn neo_trellis_index(index: usize) -> u8 {
    (index as u8 / 4) * 8 + (index as u8 % 4)
}

fn see_saw_index(index: u8) -> u8 {
    (index / 8) * 4 + (index % 8)
}

/// Iterator returned by the `key_event_iterate` and `key_event_iterate_read` functions
/// of the Neotrellis struct. Iterates through the raw bytes returned by the Neotrellis
/// event FIFO and turns them into NeotrellisEvents.
pub struct NeotrellisEventIterator<'a> {
    index: usize,
    raw_events: &'a [u8]
}

/// Event type of a `NeotrellisEvent`.
pub enum NeotrellisEventType {
    /// The event occurred because of a keypad being pressed.
    KeyPress,

    /// The event occurred because of a keypad being released.
    KeyRelease
}

/// A structure that represents a Neotrellis keypad event.
///
/// Events are generated whenever a keypad key is pressed or released.
pub struct NeotrellisEvent {
    /// The index of the keypad key that generated the event, 0-15.
    pub key_index: usize,

    /// The type of the event; KeyPress or KeyRelease.
    pub event_type: NeotrellisEventType
}

impl From<u8> for NeotrellisEvent {
    fn from(input: u8) -> Self {
        let event_type = match input & 0x01 {
            0 => NeotrellisEventType::KeyRelease,
            _ => NeotrellisEventType::KeyPress
        };

        Self {
            event_type, key_index: see_saw_index(input >> 2) as usize
        }
    }
}

impl <'a> Iterator for NeotrellisEventIterator<'a> {
    type Item = NeotrellisEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.raw_events.len() { return None; }

        let event_parsed = NeotrellisEvent::from(self.raw_events[self.index]);
        self.index += 1;

        Some(event_parsed)
    }
}

impl <I2C, E> Neotrellis <I2C> where I2C: Write<Error = E> + Read<Error = E> {
    /// Creates a new Neotrellis struct which can be used communicate with the neotrellis board.
    ///
    /// The Neotrellis specifies that it requires some delays in between writes and reads. The
    /// `delay_function` parameter should be a function that can be called to delay for some
    /// number of microseconds.
    ///
    /// The address will default to 0x2E (the default address with no jumpers). If the Neotrellis
    /// has another address due to jumpers, it can be given in the `custom_address` parameter.
    pub fn new(i2c: I2C, delay_function: fn(u32), custom_address: Option<u8>) -> Neotrellis<I2C> {
        Neotrellis { i2c, delay_function, address: custom_address.unwrap_or(0x2E) }
    }

    /// Checks if the Neotrellis board is communicable and sends several commands to initialize
    /// it to use its keypad and LED array. This function uses delays and has an assertion that
    /// the board is communicating correctly.
    pub fn initialize(&mut self) -> Result<(), E> {
        // Make sure it is working
        assert_eq!(self.hardware_id()?, 0x55);

        // Activate all keys on the trellis
        for key in 0..16 { self.key_event_enable(key)? }

        // Start up and clear the leds
        self.enable_leds()?;
        self.clear_leds()?;
        (self.delay_function)(300);
        self.refresh_leds()?;

        Ok(())
    }

    /// Delay-based function that checks the hardware ID of the Neotrellis. A value of 0x55 is expected.
    pub fn hardware_id(&mut self) -> Result<u8, E> {
        self.hardware_id_write()?;
        (self.delay_function)(500);
        self.hardware_id_read()
    }

    /// Delay-based write/read operation that checks how many events have been generated
    /// by the Neotrellis keypad, retrieves them in their raw format, and returns an iterator
    /// that will iterate through the events in a `NeotrellisEvent` form that is more
    /// usable.
    ///
    /// The `event_buffer` passed in will be filled with the raw events and the event
    /// iterator will use it to read out the parsed events.
    pub fn key_event_iterate<'a>(&mut self, event_buffer: &'a mut [u8]) -> Result<NeotrellisEventIterator<'a>, E> {
        let count = self.key_event_count()?;
        if count == 0 {
            return Ok(NeotrellisEventIterator { index: 0, raw_events: &event_buffer[..0] });
        }

        self.key_event_iterate_write()?;
        (self.delay_function)(500);
        self.key_event_iterate_read(event_buffer, count)
    }

    /// Delay-based write/read operation that returns how many events have been
    /// generated by the Neotrellis keypad.
    pub fn key_event_count(&mut self) -> Result<u8, E> {
        self.key_event_count_write()?;
        (self.delay_function)(500);
        self.key_event_count_read()
    }

    /// The `write` part of the `hardware_id` function.
    pub fn hardware_id_write(&mut self) -> Result<(), E> {
        let register_set = [STATUS_REGISTER_BASE, StatusRegister::HardwareId as u8];
        self.i2c.write(self.address, &register_set)
    }

    /// The `read` operation corresponding to the `hardware_id_write` function.
    pub fn hardware_id_read(&mut self) -> Result<u8, E> {
        let mut read_buffer: [u8; 1] = [0; 1];
        self.i2c
            .read(self.address, &mut read_buffer)
            .map(|_| read_buffer[0])
    }

    fn key_event_enable(&mut self, key_index: usize) -> Result<(), E> {
        let command = [
            KEYPAD_REGISTER_BASE,
            KeypadRegister::Event as u8,
            neo_trellis_index(key_index),
            0b00011001 // Rising and Falling edge events enabled
        ];

        self.i2c.write(self.address, &command)
    }

    /// A write operation that sets the next read to see how many keypad events are in the FIFO.
    pub fn key_event_count_write(&mut self) -> Result<(), E> {
        let command: [u8; 2] = [KEYPAD_REGISTER_BASE, KeypadRegister::Count as u8];
        self.i2c.write(self.address, &command)
    }

    /// The read operation corresponding to `key_event_count_write`.
    pub fn key_event_count_read(&mut self) -> Result<u8, E> {
        let mut read_buffer: [u8; 1] = [0; 1];
        self.i2c
            .read(self.address, &mut read_buffer)
            .map(|_| read_buffer[0])
    }

    /// A write operation that sets the next read to the keypad event buffer.
    pub fn key_event_iterate_write(&mut self) -> Result<(), E> {
        let command: [u8; 2] = [KEYPAD_REGISTER_BASE, KeypadRegister::Fifo as u8];
        self.i2c.write(self.address, &command)
    }

    /// A read event that reads the events out to `event_buffer` and returns
    /// an iterator to these events.
    pub fn key_event_iterate_read<'a>(&mut self, event_buffer: &'a mut [u8], count: u8) -> Result<NeotrellisEventIterator<'a>, E> {
        self.i2c.read(self.address, &mut event_buffer[..count as usize])?;
        Ok(NeotrellisEventIterator { index: 0, raw_events: &event_buffer[..count as usize] })
    }

    fn enable_leds(&mut self) -> Result<(), E> {
        // Set the speed to 800khz
        let command: [u8; 3] = [
            NEOPIXEL_REGISTER_BASE,
            NeopixelRegister::Speed as u8,
            0x01
        ];
        self.i2c.write(self.address, &command)?;

        // Update the length of the remote buffer
        let command: [u8; 4] = [
            NEOPIXEL_REGISTER_BASE,
            NeopixelRegister::BufferLength as u8,
            0,
            16 * 3
        ];
        self.i2c.write(self.address, &command)?;

        // Set the pin
        let command: [u8; 3] = [
            NEOPIXEL_REGISTER_BASE,
            NeopixelRegister::Pin as u8,
            3
        ];
        self.i2c.write(self.address, &command)
    }

    /// Sets all LEDs on the Neotrellis to an RGB value of #000000
    pub fn clear_leds(&mut self) -> Result<(), E> {
        // The SeeSaw cannot clear it all in one write; split into two writes with offset
        let mut command = [0; 8 * 3 + 4];
        command[0] = NEOPIXEL_REGISTER_BASE;
        command[1] = NeopixelRegister::Buffer as u8;
        self.i2c.write(self.address, &command)?;

        command[3] = 24;
        self.i2c.write(self.address, &command)
    }

    /// Sets an LED on the Neotrellis to an RGB value
    pub fn set_led(&mut self, led_index: u8, red: u8, green: u8, blue: u8) -> Result<(), E> {
        let command = [
            NEOPIXEL_REGISTER_BASE,
            NeopixelRegister::Buffer as u8,
            0, // Offset high
            3 * led_index, // Offset low
            green,
            red,
            blue
        ];

        self.i2c.write(self.address, &command)
    }

    /// Refreshes the LED display based on changes made in `clear_leds` and `set_led`.
    /// Should be called 300us after these functions to allow time for the data to
    /// latch.
    pub fn refresh_leds(&mut self) -> Result<(), E> {
        let command: [u8; 2] = [NEOPIXEL_REGISTER_BASE, NeopixelRegister::Show as u8];
        self.i2c.write(self.address, &command)
    }
}
