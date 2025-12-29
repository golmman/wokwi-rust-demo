#![no_std]
#![no_main]

use panic_halt as _;
use max7219::MAX7219;
use rtic::app;

// Visual 3x8 Font (11 chars, 8 rows, 3 cols) - same as before
const FONT: [[[u8; 3]; 8]; 11] = [
    [
        [0, 1, 0], [1, 0, 1], [1, 0, 1], [1, 0, 1], 
        [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0]
    ], // 0
    [
        [0, 0, 1], [0, 1, 1], [1, 0, 1], [0, 0, 1], 
        [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1], 
    ], // 1
    [
        [0, 1, 0], [1, 0, 1], [0, 0, 1], [0, 1, 0], 
        [1, 0, 0], [1, 0, 0], [1, 0, 0], [1, 1, 1]
    ], // 2
    [
        [0, 1, 0], [1, 0, 1], [0, 0, 1], [0, 1, 0], 
        [0, 0, 1], [0, 0, 1], [1, 0, 1], [0, 1, 0]
    ], // 3
    [
        [0, 0, 1], [0, 1, 1], [1, 0, 1], [1, 0, 1], 
        [1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1]
    ], // 4
    [
        [1, 1, 1], [1, 0, 0], [1, 0, 0], [1, 1, 0], 
        [0, 0, 1], [0, 0, 1], [1, 0, 1], [0, 1, 0]
    ], // 5
    [
        [0, 1, 0], [1, 0, 0], [1, 0, 0], [1, 1, 0], 
        [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0]
    ], // 6
    [
        [1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 1, 0], 
        [0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0]
    ], // 7
    [
        [0, 1, 0], [1, 0, 1], [1, 0, 1], [0, 1, 0], 
        [1, 0, 1], [1, 0, 1], [1, 0, 1], [0, 1, 0]
    ], // 8
    [
        [0, 1, 0], [1, 0, 1], [1, 0, 1], [0, 1, 1], 
        [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 1, 0]
    ], // 9
    [
        [0, 0, 0], [0, 0, 0], [0, 1, 0], [0, 0, 0], 
        [0, 0, 0], [0, 1, 0], [0, 0, 0], [0, 0, 0]
    ] // :
];

/// Shared state for the clock
pub struct ClockState {
    hours: u8,
    mins: u8,
    secs: u8,
}

#[app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [I2C0_IRQ])]
mod app {
    use super::*;
    use rp_pico::hal::{
        clocks::{init_clocks_and_plls, Clock},
        gpio::{bank0::Gpio15, FunctionSio, Pin, PullUp, SioInput},
        sio::Sio,
        spi::Spi,
        timer::{Alarm, Alarm0, Timer},
        watchdog::Watchdog,
        fugit::{RateExtU32, ExtU32},
    };
    use embedded_hal::digital::v2::ToggleableOutputPin;

    // Type definition for the MAX7219 display
    type Spi0 = Spi<rp_pico::hal::spi::Enabled, rp_pico::hal::pac::SPI0, (
        Pin<rp_pico::hal::gpio::bank0::Gpio19, rp_pico::hal::gpio::FunctionSpi, rp_pico::hal::gpio::PullDown>,
        Pin<rp_pico::hal::gpio::bank0::Gpio16, rp_pico::hal::gpio::FunctionSpi, rp_pico::hal::gpio::PullDown>,
        Pin<rp_pico::hal::gpio::bank0::Gpio18, rp_pico::hal::gpio::FunctionSpi, rp_pico::hal::gpio::PullDown>
    )>;
    type CsPin = Pin<rp_pico::hal::gpio::bank0::Gpio17, rp_pico::hal::gpio::FunctionSio<rp_pico::hal::gpio::SioOutput>, rp_pico::hal::gpio::PullDown>;
    type DisplayType = MAX7219<max7219::connectors::SpiConnectorSW<Spi0, CsPin>>;

    // Shared resources (accessed by multiple tasks)
    #[shared]
    struct Shared {
        clock: ClockState,
    }

    // Local resources (accessed by single tasks)
    #[local]
    struct Local {
        display: DisplayType,
        led: rp_pico::hal::gpio::Pin<rp_pico::hal::gpio::bank0::Gpio25, rp_pico::hal::gpio::FunctionSio<rp_pico::hal::gpio::SioOutput>, rp_pico::hal::gpio::PullDown>,
        button: Pin<Gpio15, FunctionSio<SioInput>, PullUp>,
        alarm: Alarm0,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        let mut pac = ctx.device;
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
        let mut alarm = timer.alarm_0().unwrap();
        // Schedule first tick in 1 second
        alarm.schedule(1_000_000u32.micros()).unwrap();
        alarm.enable_interrupt();

        let pins = rp_pico::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );

        let led = pins.led.into_push_pull_output();
        let button = pins.gpio15.into_pull_up_input();
        
        // Enable interrupt for button (Falling Edge)
        button.set_interrupt_enabled(rp_pico::hal::gpio::Interrupt::EdgeLow, true);

        let mosi = pins.gpio19.into_function::<rp_pico::hal::gpio::FunctionSpi>();
        let sck = pins.gpio18.into_function::<rp_pico::hal::gpio::FunctionSpi>();
        let miso = pins.gpio16.into_function::<rp_pico::hal::gpio::FunctionSpi>();
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
            display.set_intensity(i, 0x0).unwrap();
            display.clear_display(i).unwrap();
        }

        (
            Shared {
                clock: ClockState { hours: 12, mins: 34, secs: 56 },
            },
            Local {
                display,
                led,
                button,
                alarm,
            },
            init::Monotonics(),
        )
    }

    // Hardware Task: Timer Interrupt (1Hz)
    #[task(binds = TIMER_IRQ_0, priority = 1, shared = [clock], local = [alarm, led])]
    fn timer_tick(mut ctx: timer_tick::Context) {
        // Clear interrupt and schedule next
        ctx.local.alarm.clear_interrupt();
        ctx.local.alarm.schedule(1_000_000u32.micros()).unwrap();
        
        ctx.local.led.toggle().unwrap();

        // Update time
        ctx.shared.clock.lock(|c| {
            c.secs += 1;
            if c.secs >= 60 {
                c.secs = 0;
                c.mins += 1;
            }
            if c.mins >= 60 {
                c.mins = 0;
                c.hours = (c.hours + 1) % 24;
            }
        });

        // Spawn display update
        update_display::spawn().ok();
    }

    // Hardware Task: GPIO Interrupt (Button Press)
    #[task(binds = IO_IRQ_BANK0, priority = 1, shared = [clock], local = [button])]
    fn button_press(mut ctx: button_press::Context) {
        // Clear interrupt
        ctx.local.button.clear_interrupt(rp_pico::hal::gpio::Interrupt::EdgeLow);

        // Simple Debounce: ideally use monotonic, but for now we assume 
        // the interrupt won't trigger too rapidly or we rely on user not spamming.
        // A better way is preventing next update for X ms.
        // For simplicity in this demo, strict debouncing is omitted to keep code small,
        // relying on Wokwi's clean signals or adding a small software check.
        
        ctx.shared.clock.lock(|c| {
            c.mins += 1;
             if c.mins >= 60 {
                c.mins = 0;
                c.hours = (c.hours + 1) % 24;
            }
        });

        update_display::spawn().ok();
    }

    // Software Task: Update Display (Lower Priority if needed, but here effectively same)
    #[task(shared = [clock], local = [display])]
    fn update_display(mut ctx: update_display::Context) {
        let (h, m, s) = ctx.shared.clock.lock(|c| (c.hours, c.mins, c.secs));
        
        let digits = [
            (h / 10), (h % 10),
            10, // :
            (m / 10), (m % 10),
            10, // :
            (s / 10), (s % 10),
        ];

        let mut fb_rows = [0u32; 8];
        let mut cursor = 0;

        for (i, &d) in digits.iter().enumerate() {
            for r in 0..8 {
                for c in 0..3 {
                    // Access FONT global
                    if FONT[d as usize][r][c] != 0 {
                        let bit_pos = 31 - (cursor + c);
                        if bit_pos < 32 {
                            fb_rows[r] |= 1 << bit_pos;
                        }
                    }
                }
            }
            cursor += 3;
            if i < 7 {
                cursor += 1;
            }
        }

        let display = ctx.local.display;
        for dev_idx in 0..4 {
            let mut dev_buffer = [0u8; 8];
            for r in 0..8 {
                let shift = 24 - (dev_idx * 8);
                dev_buffer[r] = ((fb_rows[r] >> shift) & 0xFF) as u8;
            }
            display.write_raw(dev_idx, &dev_buffer).unwrap();
        }
    }
}
