#![no_main]
#![no_std]
use panic_halt as _; // panic handler

use stm32f4xx_hal as hal;

use crate::{
    button::{Active, Button, ButtonEvent, Debouncer},
    counter::Counter,
    overhead_light::OverheadLight,
    rgb_led::{LedStrip, Rgb},
    serial::{Command, Report, SerialProtocol},
};
use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use hal::{
    otg_fs::{UsbBus, USB},
    prelude::*,
    pwm,
    qei::Qei,
    spi::{Mode as SpiMode, NoMiso, NoSck, Phase, Polarity, Spi},
    stm32,
};
use usb_device::device::{UsbDeviceBuilder, UsbVidPid};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

mod button;
mod counter;
mod overhead_light;
mod rgb_led;
mod serial;

static mut USB_ENDPOINT_MEMORY: [u32; 1024] = [0; 1024];

#[entry]
fn main() -> ! {
    let mut cp =
        cortex_m::peripheral::Peripherals::take().expect("failed to get cortex_m peripherals");
    let dp = stm32::Peripherals::take().expect("failed to get stm32 peripherals");

    // Take ownership over the raw devices and convert them into the corresponding
    // HAL structs.
    // RCC = Reset and Clock Control
    let mut rcc = dp.RCC.constrain();

    // The various system clocks need to be configured to particular values
    // to work with USB - we'll set them up here.
    let clocks = rcc
        .cfgr
        .use_hse(25.mhz()) // Use the High Speed External 25MHz crystal
        .sysclk(48.mhz()) // The main system clock will be 48MHz
        // .pclk1(24.mhz())  // Use 24MHz for the APB1 (Advanced Peripheral Bus 1)
        .require_pll48clk()
        .freeze();

    // assert!(clocks.usbclk_valid());

    // Needed in order for MonoTimer to work properly
    cp.DCB.enable_trace();

    // Prepare the alternate function I/O registers
    // let mut afio = dp.AFIO.constrain(&mut rcc.apb2);

    // Grab the GPIO banks we'll use.
    let mut gpioa = dp.GPIOA.split();
    let mut gpiob = dp.GPIOB.split();

    // Set up the LED (B12).
    let mut led = gpiob.pb12.into_push_pull_output();

    // Set up USB communications
    let usb_pin_d_plus = gpioa.pa12.into_alternate_af10();
    let usb_pin_d_minus = gpioa.pa11.into_alternate_af10();

    let usb = USB {
        usb_global: dp.OTG_FS_GLOBAL,
        usb_device: dp.OTG_FS_DEVICE,
        usb_pwrclk: dp.OTG_FS_PWRCLK,

        pin_dm: usb_pin_d_minus,
        pin_dp: usb_pin_d_plus,
    };

    let usb_bus = UsbBus::new(usb, unsafe { &mut USB_ENDPOINT_MEMORY });
    let serial = SerialPort::new(&usb_bus);

    let usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .manufacturer("tonari")
        .product("tonari dashboard controller")
        .serial_number("tonari-dashboard-controller-v1")
        .device_class(USB_CLASS_CDC)
        .build();

    let mut protocol = SerialProtocol::new(usb_dev, serial);

    // Disable JTAG so that we can use the pin PB4 for the timer
    // let (_pa15, _pb3, pb4) = afio.mapr.disable_jtag(gpioa.pa15, gpiob.pb3, gpiob.pb4);

    // SPI Setup (for WS8212b RGB LEDs)
    let mosi_pin = gpioa.pa7.into_alternate_af5();
    let spi_pins = (NoSck, NoMiso, mosi_pin);
    let spi_mode = SpiMode { polarity: Polarity::IdleLow, phase: Phase::CaptureOnFirstTransition };

    let spi = Spi::spi1(dp.SPI1, spi_pins, spi_mode, 2250.khz().into(), clocks);

    // let spi = Spi::<_, Spi1NoRemap, _, u8>::spi1(
    //     dp.SPI1,
    //     spi_pins,
    //     &mut afio.mapr,
    //     spi_mode,
    //     2250.khz(), // https://os.mbed.com/teams/ST/wiki/SPI-output-clock-frequency
    //     clocks,
    //     &mut rcc.apb2,
    // );

    let mut led_strip = LedStrip::new(spi);
    led_strip.set_all(Rgb::new(0, 30, 255));

    // PWM Setup
    // https://docs.rs/stm32f1xx-hal/0.6.1/stm32f1xx_hal/timer/index.html
    let pwm_freq = 1.khz();

    let timer3_pwm_pins = (
        gpioa.pa6.into_alternate_af2(),
        gpioa.pa7.into_alternate_af2(),
        gpiob.pb0.into_alternate_af2(),
        gpiob.pb1.into_alternate_af2(),
    );

    let timer4_pwm_pins = (
        gpiob.pb6.into_alternate_af2(),
        gpiob.pb7.into_alternate_af2(),
        gpiob.pb8.into_alternate_af2(),
        gpiob.pb9.into_alternate_af2(),
    );

    // let (pwm1, pwm2, pwm3, pwm4) = Timer::tim3(dp.TIM3, &clocks, &mut rcc.apb1)
    //     .pwm::<Tim3PartialRemap, _, _, _>(timer3_pwm_pins, &mut afio.mapr, 1.khz())
    //     .split();
    let (pwm1, pwm2, pwm3, pwm4) = pwm::tim3(dp.TIM3, timer3_pwm_pins, clocks, pwm_freq);

    // let (pwm5, pwm6, pwm7, pwm8) = Timer::tim4(dp.TIM4, &clocks, &mut rcc.apb1)
    //     .pwm(timer4_pwm_pins, &mut afio.mapr, 1.khz())
    //     .split();
    let (pwm5, pwm6, pwm7, pwm8) = pwm::tim4(dp.TIM4, timer4_pwm_pins, clocks, pwm_freq);

    // The overhead light closer to the screen.
    let mut front_light = OverheadLight::new(pwm1, pwm2, pwm3, pwm4);

    // The overhead light farther away from the screen.
    let mut back_light = OverheadLight::new(pwm5, pwm6, pwm7, pwm8);

    // Connect a rotary encoder to pins A0 and A1.
    let rotary_encoder_pins = (gpioa.pa0, gpioa.pa1);
    // Tim2NoRemap relates to how you can "remap" pins used on timer 2 for certain peripherals.
    // https://docs.rs/stm32f1xx-hal/0.6.1/stm32f1xx_hal/timer/index.html

    let rotary_encoder_timer = dp.TIM2;
    let rotary_encoder_pins = (gpioa.pa0.into_alternate_af1(), gpioa.pa1.into_alternate_af1());
    let rotary_encoder = Qei::tim2(rotary_encoder_timer, rotary_encoder_pins);

    // let rotary_encoder = Timer::tim2(dp.TIM2, &clocks, &mut rcc.apb1).qei::<Tim2NoRemap, _>(
    //     rotary_encoder_pins,
    //     &mut afio.mapr,
    //     QeiOptions::default(),
    // );

    let mut counter = Counter::new(rotary_encoder);

    let button_pin = gpioa.pa3.into_pull_up_input();
    let debounced_encoder_pin = Debouncer::new(button_pin, Active::Low, 30, 3000);
    let mut encoder_button = Button::new(debounced_encoder_pin, 1000, cp.DWT, cp.DCB, clocks);

    loop {
        match encoder_button.poll() {
            Some(ButtonEvent::Pressed) => {
                led.set_low().unwrap();
            },
            Some(ButtonEvent::ShortRelease) => {
                protocol.report(Report::Press).unwrap();
                led.set_high().unwrap();
            },
            Some(ButtonEvent::LongPress) => {
                protocol.report(Report::LongPress).unwrap();
                led.set_high().unwrap();
            },
            Some(ButtonEvent::LongRelease) => {},
            _ => {},
        }

        if let Some(diff) = counter.poll() {
            if !encoder_button.is_pressed() {
                protocol.report(Report::DialValue { diff }).unwrap();
            }
        }

        // TODO(bschwind) - Report any poll errors back to the USB host if possible.
        for command in protocol.poll().unwrap() {
            match command {
                Command::Brightness { target, value } => match target {
                    0 => front_light.set_brightness(value),
                    1 => back_light.set_brightness(value),
                    _ => {},
                },
                Command::Temperature { target, value } => match target {
                    0 => front_light.set_color_temperature(value),
                    1 => back_light.set_color_temperature(value),
                    _ => {},
                },
                _ => {},
            }
        }
    }
}
