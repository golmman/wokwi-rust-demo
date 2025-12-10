#![no_std]
#![no_main]

use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use panic_halt as _;
use rp_pico::hal::{
    clocks::{init_clocks_and_plls, Clock},
    gpio::{FunctionI2C, Pin, PullUp},
    pac,
    sio::Sio,
    watchdog::Watchdog,
    I2C,
};

// Ensure we have the trait for kHz()
use rp_pico::hal::fugit::RateExtU32;

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, PrimitiveStyleBuilder},
    text::Text,
};
use embedded_hal::blocking::i2c::Write;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    // External high-speed crystal on the pico board is 12Mhz
    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = init_clocks_and_plls(
        external_xtal_freq_hz,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    let pins = rp_pico::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // LED setup
    let mut led = pins.led.into_push_pull_output();

    // DEBUG: Slow blink x2 to indicate START (Health Check)
    for _ in 0..2 {
        led.set_high().unwrap();
        delay.delay_ms(500);
        led.set_low().unwrap();
        delay.delay_ms(500);
    }
    
    // Ensure LED is OFF before I2C init
    led.set_low().unwrap();

    // I2C Setup
    // GP26 = SDA, GP27 = SCL
    let sda_pin: Pin<_, FunctionI2C, PullUp> = pins
        .gpio26
        .into_function::<FunctionI2C>()
        .into_pull_type::<PullUp>();
    let scl_pin: Pin<_, FunctionI2C, PullUp> = pins
        .gpio27
        .into_function::<FunctionI2C>()
        .into_pull_type::<PullUp>();

    let mut i2c = I2C::i2c1(
        pac.I2C1,
        sda_pin,
        scl_pin,
        100.kHz(),
        &mut pac.RESETS,
        &clocks.peripheral_clock,
    );

    // DEBUG: Raw I2C Init Sequence
    // Blink Start Man Init
    led.set_high().unwrap();
    delay.delay_ms(100);
    led.set_low().unwrap();
    delay.delay_ms(100);

    // Sequence
    let cmds = [
        0xAE, // Display Off
        0xD5, 0x80, // Clock div
        0xA8, 0x3F, // Multiplex
        0xD3, 0x00, // Offset
        0x40, // Start Line
        0x8D, 0x14, // Charge Pump
        0x20, 0x00, // Memory Mode
        0xA1, // Seg Remap
        0xC8, // Com Scan Dec
        0xDA, 0x12, // Comp Pins
        0x81, 0xCF, // Contrast
        0xD9, 0xF1, // Precharge
        0xDB, 0x40, // VCOM Detect
        0xA4, // Resume to RAM
        0xA6, // Normal Display
        0x2E, // Deactivate Scroll
        0xAF, // Display ON
    ];

    for (_i, cmd) in cmds.iter().enumerate() {
        // Send Control Byte 0x00 + Command
        if i2c.write(0x3Cu8, &[0x00, *cmd]).is_err() {
            // Panic if write fails
             loop {
                 led.set_high().unwrap();
                 delay.delay_ms(50);
                 led.set_low().unwrap();
                 delay.delay_ms(50);
             }
        }
        // Small delay to prevent overwhelming simulation?
        delay.delay_ms(10);
    }
    
    // Init Success Blink (Long)
    led.set_high().unwrap();
    delay.delay_ms(1000);
    led.set_low().unwrap();

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    
    // SKIP display.init(). We did it manually.

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let mut x = 20;
    let mut y = 20;
    let mut dx = 2;
    let mut dy = 2;

    loop {
        // Pulse LED
        led.set_high().unwrap();
        
        // Update physics
        x += dx;
        y += dy;

        if x <= 0 || x >= 120 {
            dx = -dx;
        }
        if y <= 0 || y >= 56 {
            dy = -dy;
        }

        display.clear(BinaryColor::Off).unwrap();
        
        // Draw things
        Text::new("Wokwi Rust", Point::new(30, 10), text_style)
            .draw(&mut display)
            .unwrap();
            
        Circle::new(Point::new(x, y), 8)
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .stroke_color(BinaryColor::On)
                    .stroke_width(1)
                    .fill_color(BinaryColor::Off)
                    .build(),
            )
            .draw(&mut display)
            .unwrap();

        // Flush display
        display.flush().unwrap();
        
        // Turn LED OFF after frame done
        led.set_low().unwrap();
        delay.delay_ms(50);
    }
}
