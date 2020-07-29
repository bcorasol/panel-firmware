use core::convert::Infallible;
use embedded_hal::digital::v2::InputPin;

pub struct Debouncer<T: InputPin> {
    pin: T,
    integrator: u8,
    max: u8,
    output: bool,
    active_mode: Active,
}

pub enum Active {
    Low,
    High,
}

impl<T: InputPin<Error = Infallible>> Debouncer<T> {
    pub fn new(pin: T, active_mode: Active, debounce_time_ms: u16, sample_frequency: u16) -> Self {
        let max = ((debounce_time_ms as f32 / 1000.0) * sample_frequency as f32) as u8;

        let integrator = match active_mode {
            Active::Low => max,
            Active::High => 0,
        };

        let output = match active_mode {
            Active::Low => true,
            Active::High => false,
        };

        Self { pin, integrator, max, output, active_mode }
    }

    pub fn update(&mut self) {
        if self.pin.is_low().unwrap() {
            if self.integrator > 0 {
                self.integrator -= 1;
            }
        } else if self.integrator < self.max {
            self.integrator += 1;
        }

        if self.integrator == 0 {
            self.output = false;
        } else if self.integrator >= self.max {
            self.output = true;
            self.integrator = self.max;
        }
    }

    pub fn is_pressed(&self) -> bool {
        match (&self.active_mode, self.output) {
            (Active::High, true) => true,
            (Active::Low, false) => true,
            _ => false,
        }
    }
}
