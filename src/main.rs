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

// 3x5 Font (Centered in 8-pixel height, shifted left by 1 for alignment/reversal context)
// Because we reverse bits for display, we need to define them carefully.
// Let's define them normally first (LSB=Top, MSB=Bottom or vice versa), then flip.
// Max7219: LSB is usually top row D0.
// Let's define standard 3x5 (Rows 1-5).
// 0x1F = 0001 1111 (Rows 0-4? No, assume Row 1-5 = 0x3E).
// Let's use 0x1F (Rows 0-4) and shift later if needed? No, let's use what I derived.
// Actually, let's use 0x7F for full height if needed, but 0x1F is 5 pixels.
// To center, shift up/down.
// Width is 3 bytes.

const FONT: [[u8; 3]; 11] = [
    // 0: [0x1F, 0x11, 0x1F] -> Shift shifted to center (<<1) -> [0x3E, 0x22, 0x3E]
    [0x3E, 0x22, 0x3E], // 0
    [0x00, 0x3E, 0x00], // 1
    [0x2E, 0x2A, 0x3A], // 2 (derived from 3x5 "standard" 0x17, 0x15, 0x1D but flipped? No, let's use 0x2E(..101110), 0x2A(..101010), 0x3A(..111010) )
    // Wait, let's stick to the bits I know:
    // 3: [0x22, 0x2A, 0x3E] (Left: Top+Bot=0x22, Mid: Top+Mid+Bot=0x2A, Right: Full=0x3E)
    [0x22, 0x2A, 0x3E], // 3
    // 4: [0x1C, 0x08, 0x3E] (Left: Top+Mid? 0x0F? No. 0x18(Top+Mid)? 0x1C? )
    // Let's use standard hex for 3x5 and shift << 1.
    // 0: 1F 11 1F -> 3E 22 3E
    // 1: 00 1F 00 -> 00 3E 00
    // 2: 17 15 1D -> 2E 2A 3A
    // 3: 15 15 1F -> 2A 2A 3E (Left 15 is 10101? Top+Mid+Bot? Yes. Right 1F is full. 3 is `[2A, 2A, 3E]`)
    // 4: 07 04 1F -> 0E 08 3E (Left 07 = 11100 (Bot+Mid+Top?). 04=Mid. 1F=Full col?)
    [0x0E, 0x08, 0x3E], // 4
    // 5: 1D 15 17 -> 3A 2A 2E
    [0x3A, 0x2A, 0x2E], // 5
    // 6: 1F 15 17 -> 3E 2A 2E
    [0x3E, 0x2A, 0x2E], // 6
    // 7: 10 10 1F -> 20 20 3E (Left: Top. Mid: Top. Right: Full. Looks like 7).
    [0x20, 0x20, 0x3E], // 7
    // 8: 1F 15 1F -> 3E 2A 3E
    [0x3E, 0x2A, 0x3E], // 8
    // 9: 17 15 1F -> 2E 2A 3E
    [0x2E, 0x2A, 0x3E], // 9
    // : (Colon). 14 (0x0A<<1). 00010100.
    [0x00, 0x14, 0x00], // :
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

    // HH:MM:SS = 8 chars.
    // 12:34:56
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

        // We have 8 chars * 3 cols = 24 cols of data.
        // We have 32 cols total space.
        // Gaps: 7 gaps between 8 chars.
        // Width = 24 + 7 = 31 cols.
        // Center it? Left pad 0.
        
        let mut fb = [0u8; 32];
        let mut cursor = 0; // Start at col 0

        for (i, &d) in digits.iter().enumerate() {
            let bitmap = FONT[d as usize];
            // Draw 3 bytes
            if cursor + 3 <= 32 {
                 fb[cursor] = bitmap[0];
                 fb[cursor+1] = bitmap[1];
                 fb[cursor+2] = bitmap[2];
            }
            cursor += 3;
            // Add 1 pixel gap, except after last char
            if i < 7 {
                cursor += 1; // Leave 0 (gap)
            }
        }

        // Write to display
        // fb[0] is Leftmost col logic.
        // Map fb to MAX7219 devices (4 devices, 8 cols each).
        // Device 0 (Physical Right?) vs Device 3. 
        // Previous experiment showed we needed to reverse digit order (3-i) for digits.
        // This implies Logical Device 0 is Rightmost.
        // So FB index 0 (Leftmost) should go to Device 3, Col 0?
        // We construct a full 8-byte array for each device to use write_raw correctly
        
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
        
        // Inc time
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
        
        // Blink LED
        led.toggle().unwrap();
    }
}
