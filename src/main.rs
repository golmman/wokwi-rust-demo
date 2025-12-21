#![no_std]
#![no_main]

use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use panic_halt as _;
use rp_pico::hal::{
    clocks::{init_clocks_and_plls, Clock},
    gpio::{FunctionSpi},
    pac,
    sio::Sio,
    spi::Spi,
    watchdog::Watchdog,
};
use rp_pico::hal::fugit::RateExtU32;

// We use the max7219 crate to drive the display
use max7219::MAX7219;

// Bitmap font for digits 0-9 (8x8)
const DIGITS: [[u8; 8]; 10] = [
    [0x3C, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C], // 0
    [0x10, 0x30, 0x50, 0x10, 0x10, 0x10, 0x10, 0x7C], // 1
    [0x3C, 0x42, 0x02, 0x04, 0x18, 0x20, 0x40, 0x7E], // 2
    [0x3C, 0x42, 0x02, 0x1C, 0x02, 0x02, 0x42, 0x3C], // 3
    [0x08, 0x18, 0x28, 0x48, 0x7E, 0x08, 0x08, 0x08], // 4
    [0x7E, 0x40, 0x78, 0x04, 0x02, 0x02, 0x42, 0x3C], // 5
    [0x3C, 0x40, 0x40, 0x7C, 0x42, 0x42, 0x42, 0x3C], // 6
    [0x7E, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x40], // 7
    [0x3C, 0x42, 0x42, 0x3C, 0x42, 0x42, 0x42, 0x3C], // 8
    [0x3C, 0x42, 0x42, 0x3E, 0x02, 0x02, 0x02, 0x3C], // 9
];

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

    let mut led = pins.led.into_push_pull_output();

    // SPI0 Setup
    // GP19 = TX (MOSI), GP18 = SCK, GP16 = RX (MISO)
    let mosi = pins.gpio19.into_function::<FunctionSpi>();
    let sck = pins.gpio18.into_function::<FunctionSpi>();
    let miso = pins.gpio16.into_function::<FunctionSpi>();
    
    // CS Pin (GP17)
    let cs = pins.gpio17.into_push_pull_output();

    // Init SPI with const generic DS=8 (Data Size 8-bits)
    let spi = Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck));
    let spi = spi.init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        2_000_000u32.Hz(), // 2 MHz
        &embedded_hal::spi::MODE_0,
    );

    // Initialize MAX7219
    // Daisychain length = 4
    let mut display = MAX7219::from_spi_cs(4, spi, cs).unwrap();
    
    // Initialization sequence
    display.power_on().unwrap();
    for i in 0..4 {
        display.set_intensity(i, 0x4).unwrap();
        display.clear_display(i).unwrap();
    }

    let mut count = 0u32;

    loop {
        // Split count into digits up to 9999
        let val = count % 10000;
        let d3 = (val / 1000) % 10;
        let d2 = (val / 100) % 10;
        let d1 = (val / 10) % 10;
        let d0 = val % 10;

        let digits = [d3, d2, d1, d0];
        // Assumes Device 0 is Leftmost (Thousands) ... Device 3 is Rightmost (Ones)
        // If display order is reversed in simulation, invert this loop.
        for (i, &digit) in digits.iter().enumerate() {
             display.write_raw(i, &DIGITS[digit as usize]).unwrap();
        }

        // Delay 100ms
        delay.delay_ms(100);
        count += 1;

        // Blink LED
        if (count / 5) % 2 == 0 {
            led.set_high().unwrap();
        } else {
            led.set_low().unwrap();
        }
    }
}
