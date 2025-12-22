#![no_std]
#![no_main]

use cortex_m_rt::entry;
use embedded_hal::digital::v2::{OutputPin, ToggleableOutputPin};
use panic_halt as _;
use rp_pico::hal::{
    clocks::{init_clocks_and_plls, Clock},
    gpio::FunctionSpi,
    pac,
    sio::Sio,
    spi::Spi,
    watchdog::Watchdog,
    Timer,
};
use rp_pico::hal::fugit::RateExtU32;
use embedded_hal::blocking::delay::DelayMs;
use max7219::MAX7219;

// Visual 3x8 Font (11 chars, 8 rows, 3 cols)
const FONT: [[[u8; 3]; 8]; 11] = [
    [
        [0, 1, 0], 
        [1, 0, 1], 
        [1, 0, 1], 
        [1, 0, 1], 
        [1, 0, 1], 
        [1, 0, 1], 
        [1, 0, 1], 
        [0, 1, 0]
    ],
    [
        [0, 0, 1], 
        [0, 1, 1], 
        [1, 0, 1], 
        [0, 0, 1], 
        [0, 0, 1], 
        [0, 0, 1], 
        [0, 0, 1], 
        [0, 0, 1], 
    ],
    [
        [0, 1, 0],
        [1, 0, 1],
        [0, 0, 1],
        [0, 0, 1],
        [0, 1, 0],
        [1, 0, 0],
        [1, 0, 0],
        [1, 1, 1]
    ],
    [
        [0, 1, 0],
        [1, 0, 1],
        [0, 0, 1],
        [0, 1, 0],
        [0, 0, 1],
        [0, 0, 1],
        [1, 0, 1],
        [0, 1, 0]
    ],
    [
        [0, 0, 1],
        [0, 1, 1],
        [1, 0, 1],
        [1, 0, 1],
        [1, 1, 1],
        [0, 0, 1],
        [0, 0, 1],
        [0, 0, 1]
    ],
    [
        [1, 1, 1],
        [1, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [0, 0, 1],
        [0, 0, 1],
        [1, 0, 1],
        [0, 1, 0]
    ],
    [
        [0, 1, 0],
        [1, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [1, 0, 1],
        [1, 0, 1],
        [1, 0, 1],
        [0, 1, 0]
    ],
    [
        [1, 1, 1],
        [0, 0, 1],
        [0, 0, 1],
        [0, 1, 0],
        [0, 1, 0],
        [0, 1, 0],
        [0, 1, 0],
        [0, 1, 0]
    ],
    [
        [0, 1, 0],
        [1, 0, 1],
        [1, 0, 1],
        [0, 1, 0],
        [1, 0, 1],
        [1, 0, 1],
        [1, 0, 1],
        [0, 1, 0]
    ],
    [
        [0, 1, 0],
        [1, 0, 1],
        [1, 0, 1],
        [0, 1, 1],
        [0, 0, 1],
        [0, 0, 1],
        [0, 0, 1],
        [0, 1, 0]
    ],
    [
        [0, 0, 0],
        [0, 0, 0],
        [0, 1, 0],
        [0, 0, 0],
        [0, 0, 0],
        [0, 1, 0],
        [0, 0, 0],
        [0, 0, 0]
    ]
];

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

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

    let mut timer = Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let pins = rp_pico::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut led = pins.led.into_push_pull_output();

    let mosi = pins.gpio19.into_function::<FunctionSpi>();
    let sck = pins.gpio18.into_function::<FunctionSpi>();
    let miso = pins.gpio16.into_function::<FunctionSpi>();
    let cs = pins.gpio17.into_push_pull_output();

    let spi = Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck));
    let spi = spi.init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        2_000_000u32.Hz(),
        &embedded_hal::spi::MODE_0,
    );

    let mut display = MAX7219::from_spi_cs(4, spi, cs).unwrap();
    display.power_on().unwrap();
    for i in 0..4 {
        display.set_intensity(i, 0x4).unwrap();
        display.clear_display(i).unwrap();
    }

    let mut hours = 12u8;
    let mut mins = 34u8;
    let mut secs = 56u8;

    loop {
        // Construct display string
        let digits = [
            (hours / 10), (hours % 10),
            10, // :
            (mins / 10), (mins % 10),
            10, // :
            (secs / 10), (secs % 10),
        ];

        let mut fb = [0u8; 32];
        let mut cursor = 0; // Start at col 0

        for (i, &d) in digits.iter().enumerate() {
            // Convert visual font to bitmap cols
            let mut bitmap = [0u8; 3];
            for r in 0..8 {
                for c in 0..3 {
                    if FONT[d as usize][r][c] != 0 {
                        bitmap[c] |= 1 << r;
                    }
                }
            }

            // Draw 3 bytes
            if cursor + 3 <= 32 {
                 fb[cursor] = bitmap[0];
                 fb[cursor+1] = bitmap[1];
                 fb[cursor+2] = bitmap[2];
            }
            cursor += 3;
            // Add 1 pixel gap, except after last char
            if i < 7 {
                cursor += 1;
            }
        }

        // Write to display
        for dev_idx in 0..4 {
            // Fix string order: 0->0, 1->1 ...
            let logical_dev = dev_idx; 
            let start_col = logical_dev * 8;
            
            let mut dev_buffer = [0u8; 8];

            // Extract 8 columns for this device
            let mut cols = [0u8; 8];
            for col in 0..8 {
                let fb_idx = start_col + col;
                if fb_idx < 32 {
                    cols[col] = fb[fb_idx];
                }
            }

            // Transpose: Map Columns to Rows
            for r in 0..8 {
                for c in 0..8 {
                    // Fix character mirroring: Use c instead of 7-c
                    let bit = (cols[c] >> r) & 1;
                    if bit != 0 {
                        dev_buffer[r] |= 1 << c;
                    }
                }
            }
            
            display.write_raw(dev_idx, &dev_buffer).unwrap();
        }

        timer.delay_ms(1000);
        
        secs += 1;
        if secs >= 60 {
            secs = 0;
            mins += 1;
        }
        if mins >= 60 {
            mins = 0;
            hours += 1;
        }
        if hours >= 24 {
            hours = 0;
        }
        
        led.toggle().unwrap();
    }
}
