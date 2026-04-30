//! Board support package for the wokwi-rust-demo Pico clock.
//!
//! This module is the single source of truth for **what hardware we use**:
//! - GPIO pin assignments,
//! - the SPI instance + MAX7219 wiring,
//! - the timer alarms.
//!
//! The bring-up sequence (`Board::take`) configures everything so `main.rs`
//! can stay focused on RTIC task wiring and ignore HAL boilerplate.
//!
//! If you change a pin number, you almost certainly only need to edit this
//! file and the matching connection in `diagram.json`.

use max7219::MAX7219;
use rp_pico::hal::{
    clocks::{init_clocks_and_plls, Clock},
    fugit::RateExtU32,
    gpio::{
        bank0::{Gpio15, Gpio16, Gpio17, Gpio18, Gpio19, Gpio25},
        FunctionSio, FunctionSpi, Interrupt, Pin, PullDown, PullUp, SioInput, SioOutput,
    },
    pac,
    sio::Sio,
    spi::Spi,
    timer::Timer,
    watchdog::Watchdog,
};

pub use rp_pico::hal::timer::{Alarm0, Alarm1};

use wokwi_test::config::{CHAIN_LEN, DISPLAY_INTENSITY, SPI_FREQ_HZ, TICK_INTERVAL_US};

// === Configured-peripheral type aliases ===

/// SPI0 wired up as MOSI=GP19, MISO=GP16, SCK=GP18, ready for the MAX7219 chain.
pub type Spi0 = Spi<
    rp_pico::hal::spi::Enabled,
    pac::SPI0,
    (
        Pin<Gpio19, FunctionSpi, PullDown>,
        Pin<Gpio16, FunctionSpi, PullDown>,
        Pin<Gpio18, FunctionSpi, PullDown>,
    ),
>;

/// MAX7219 chip-select on GP17 as a push-pull output.
pub type CsPin = Pin<Gpio17, FunctionSio<SioOutput>, PullDown>;

/// The full daisy-chained MAX7219 display driver.
pub type Display = MAX7219<max7219::connectors::SpiConnectorSW<Spi0, CsPin>>;

/// Push-button on GP15, active-low with internal pull-up.
pub type ButtonPin = Pin<Gpio15, FunctionSio<SioInput>, PullUp>;

/// Onboard LED on GP25, used as a 1 Hz heartbeat.
pub type LedPin = Pin<Gpio25, FunctionSio<SioOutput>, PullDown>;

// === Bring-up ===

/// Everything the RTIC tasks need from configured hardware.
pub struct Board {
    pub display: Display,
    pub button: ButtonPin,
    pub led: LedPin,
    pub alarm0: Alarm0,
    pub alarm1: Alarm1,
}

impl Board {
    /// Configure clocks/PLL, GPIO, SPI, the MAX7219 chain, and the two
    /// timer alarms used by the firmware. Panics on init-time failure —
    /// there's no useful recovery for a Pico that won't bring its own
    /// peripherals up.
    pub fn take(mut pac: pac::Peripherals) -> Self {
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

        // alarm0: periodic 1 Hz tick that drives the clock.
        let mut alarm0 = timer.alarm_0().unwrap();
        use rp_pico::hal::fugit::ExtU32 as _;
        use rp_pico::hal::timer::Alarm as _;
        alarm0.schedule(TICK_INTERVAL_US.micros()).unwrap();
        alarm0.enable_interrupt();

        // alarm1: button auto-repeat, scheduled on demand by ISRs.
        let mut alarm1 = timer.alarm_1().unwrap();
        alarm1.enable_interrupt();

        let pins = rp_pico::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );

        let led: LedPin = pins.led.into_push_pull_output();

        let button: ButtonPin = pins.gpio15.into_pull_up_input();
        button.set_interrupt_enabled(Interrupt::EdgeLow, true);

        let mosi = pins.gpio19.into_function::<FunctionSpi>();
        let sck = pins.gpio18.into_function::<FunctionSpi>();
        let miso = pins.gpio16.into_function::<FunctionSpi>();
        let cs: CsPin = pins.gpio17.into_push_pull_output();

        let spi = Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, miso, sck));
        let spi = spi.init(
            &mut pac.RESETS,
            clocks.peripheral_clock.freq(),
            SPI_FREQ_HZ.Hz(),
            embedded_hal::spi::MODE_0,
        );

        let mut display = MAX7219::from_spi_cs(CHAIN_LEN, spi, cs).unwrap();
        display.power_on().unwrap();
        for i in 0..CHAIN_LEN {
            display.set_intensity(i, DISPLAY_INTENSITY).unwrap();
            display.clear_display(i).unwrap();
        }

        Board {
            display,
            button,
            led,
            alarm0,
            alarm1,
        }
    }
}
