use core::convert::Infallible;
use stm32f1xx_hal as hal;

use embedded_hal::digital::v2::InputPin;
use hal::time::{Instant, MonoTimer};

pub struct Button<T: InputPin> {
    pin: Debouncer<T>,
    timer: MonoTimer,
    button_state: ButtonState,
    long_press_timeout_ticks: u32,
}

pub enum ButtonEvent {
    /// The button has just been pressed down.
    Pressed,

    /// The button was released before the "long press" timeout.
    ShortRelease,

    /// The button has been held for at least the "long press" timeout.
    LongPress,

    /// The button has been released after a "long press".
    LongRelease,
}

enum ButtonState {
    Released,
    Pressed(Instant),
    LongPressed,
}

impl<T: InputPin<Error = Infallible>> Button<T> {
    pub fn new(pin: Debouncer<T>, long_press_timeout_ms: u32, timer: MonoTimer) -> Self {
        let button_state = ButtonState::Released;
        let long_press_timeout_ticks =
            (timer.frequency().0 as f32 * (long_press_timeout_ms as f32 / 1000.0)) as u32;

        Self { pin, timer, button_state, long_press_timeout_ticks }
    }

    pub fn is_pressed(&self) -> bool {
        self.pin.is_pressed()
    }

    pub fn poll(&mut self) -> Option<ButtonEvent> {
        self.pin.poll();

        match self.button_state {
            ButtonState::Released => {
                if self.pin.is_pressed() {
                    let now = self.timer.now();
                    self.button_state = ButtonState::Pressed(now);
                    return Some(ButtonEvent::Pressed);
                }
            },
            ButtonState::Pressed(press_start) => {
                if !self.pin.is_pressed() {
                    self.button_state = ButtonState::Released;
                    return Some(ButtonEvent::ShortRelease);
                } else if press_start.elapsed() > self.long_press_timeout_ticks {
                    self.button_state = ButtonState::LongPressed;
                    return Some(ButtonEvent::LongPress);
                }
            },
            ButtonState::LongPressed => {
                if !self.pin.is_pressed() {
                    self.button_state = ButtonState::Released;
                    return Some(ButtonEvent::LongRelease);
                }
            },
        }

        None
    }
}

// Debouncer code inspired by Kenneth Kuhn's C debouncer:
// http://www.kennethkuhn.com/electronics/debounce.c
pub struct Debouncer<T: InputPin> {
    pin: T,
    integrator: u8,
    max: u8,
    output: bool,
    active_mode: Active,
}

#[allow(dead_code)]
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

    pub fn poll(&mut self) {
        if self.pin.is_low().unwrap() {
            self.integrator = self.integrator.saturating_sub(1);
        } else if self.integrator < self.max {
            self.integrator += 1;
        }

        if self.integrator == 0 {
            self.output = false;
        } else if self.integrator >= self.max {
            self.output = true;
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
