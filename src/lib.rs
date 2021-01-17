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

pub struct NeotrellisEventIterator<'a> {
    index: usize,
    raw_events: &'a [u8]
}

pub enum NeotrellisEventType {
    KeyPress,
    KeyRelease
}

pub struct NeotrellisEvent {
    pub key_index: usize,
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
    pub fn new(i2c: I2C, delay_function: fn(u32), custom_address: Option<u8>) -> Neotrellis<I2C> {
        Neotrellis { i2c, delay_function, address: custom_address.unwrap_or(0x2E) }
    }

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

    pub fn hardware_id(&mut self) -> Result<u8, E> {
        self.hardware_id_write()?;
        (self.delay_function)(500);
        self.hardware_id_read()
    }

    pub fn key_event_iterate<'a>(&mut self, event_buffer: &'a mut [u8]) -> Result<NeotrellisEventIterator<'a>, E> {
        let count = self.key_event_count()?;
        if count == 0 {
            return Ok(NeotrellisEventIterator { index: 0, raw_events: &event_buffer[..0] });
        }

        self.key_event_iterate_write()?;
        (self.delay_function)(500);
        self.key_event_iterate_read(event_buffer, count)
    }

    pub fn key_event_count(&mut self) -> Result<u8, E> {
        self.key_event_count_write()?;
        (self.delay_function)(500);
        self.key_event_count_read()
    }

    // Delayless functions
    pub fn hardware_id_write(&mut self) -> Result<(), E> {
        let register_set = [STATUS_REGISTER_BASE, StatusRegister::HardwareId as u8];
        self.i2c.write(self.address, &register_set)
    }

    pub fn hardware_id_read(&mut self) -> Result<u8, E> {
        let mut read_buffer: [u8; 1] = [0; 1];
        self.i2c
            .read(self.address, &mut read_buffer)
            .map(|_| read_buffer[0])
    }

    pub fn key_event_enable(&mut self, key_index: usize) -> Result<(), E> {
        let command = [
            KEYPAD_REGISTER_BASE,
            KeypadRegister::Event as u8,
            neo_trellis_index(key_index),
            0b00011001 // Rising and Falling edge events enabled
        ];

        self.i2c.write(self.address, &command)
    }

    pub fn key_event_count_write(&mut self) -> Result<(), E> {
        let command: [u8; 2] = [KEYPAD_REGISTER_BASE, KeypadRegister::Count as u8];
        self.i2c.write(self.address, &command)
    }

    pub fn key_event_count_read(&mut self) -> Result<u8, E> {
        let mut read_buffer: [u8; 1] = [0; 1];
        self.i2c
            .read(self.address, &mut read_buffer)
            .map(|_| read_buffer[0])
    }

    pub fn key_event_iterate_write(&mut self) -> Result<(), E> {
        let command: [u8; 2] = [KEYPAD_REGISTER_BASE, KeypadRegister::Fifo as u8];
        self.i2c.write(self.address, &command)
    }

    pub fn key_event_iterate_read<'a>(&mut self, event_buffer: &'a mut [u8], count: u8) -> Result<NeotrellisEventIterator<'a>, E> {
        self.i2c.read(self.address, &mut event_buffer[..count as usize])?;
        Ok(NeotrellisEventIterator { index: 0, raw_events: &event_buffer[..count as usize] })
    }

    pub fn enable_leds(&mut self) -> Result<(), E> {
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

    pub fn clear_leds(&mut self) -> Result<(), E> {
        // The SeeSaw cannot clear it all in one write; split into two writes with offset
        let mut command = [0; 8 * 3 + 4];
        command[0] = NEOPIXEL_REGISTER_BASE;
        command[1] = NeopixelRegister::Buffer as u8;
        self.i2c.write(self.address, &command)?;

        command[3] = 24;
        self.i2c.write(self.address, &command)
    }

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

    pub fn refresh_leds(&mut self) -> Result<(), E> {
        let command: [u8; 2] = [NEOPIXEL_REGISTER_BASE, NeopixelRegister::Show as u8];
        self.i2c.write(self.address, &command)
    }
}
