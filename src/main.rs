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

    // I2C Init Sequence (Manual)
    // We manually initialize the display to avoid issues with the ssd1306 library's init sequence in Wokwi.
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

    for cmd in cmds {
        // Send Control Byte 0x00 + Command
        // We panic if this fails as display is essential
        i2c.write(0x3Cu8, &[0x00, cmd]).unwrap();
    }
    
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    
    // Note: display.init() is skipped because we performed manual initialization above.

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let mut x = 20;
    let mut y = 20;
    let mut dx = 2;
    let mut dy = 2;
    
    // Timer state
    let mut frame_count = 0u32;
    let mut seconds = 0u32;
    
    // String buffer for display text
    // "Wokwi Rust: 12345" needs < 32 chars
    use core::fmt::Write;
    use heapless::String;

    loop {
        // Frame management (approx 20 FPS based on 50ms delay)
        frame_count += 1;
        
        // Update seconds every 20 frames (50ms * 20 = 1000ms = 1s)
        if frame_count % 20 == 0 {
            seconds += 1;
        }
        
        // Blink LED: ON for 0-9 (0.5s), OFF for 10-19 (0.5s)
        if (frame_count % 20) < 10 {
            led.set_high().unwrap();
        } else {
            led.set_low().unwrap();
        }

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
        
        // Draw Text with Counter
        let mut text_buf: String<32> = String::new();
        // If write fails, just show error char or empty (unwrap panics, which is okay here)
        write!(&mut text_buf, "Wokwi Rust: {}", seconds).unwrap();
        
        Text::new(&text_buf, Point::new(20, 10), text_style)
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

        display.flush().unwrap();
        
        // Fixed delay for ~20FPS
        delay.delay_ms(50);
    }
}
