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
        bank0::{Gpio13, Gpio14, Gpio15, Gpio16, Gpio17, Gpio18, Gpio19, Gpio25},
        FunctionSio, FunctionSpi, Interrupt, Pin, PullDown, PullUp, SioInput, SioOutput,
    },
    pac,
    sio::Sio,
    spi::Spi,
    timer::Timer,
    watchdog::Watchdog,
};

pub use rp_pico::hal::timer::{Alarm0, Alarm1, Alarm2, Alarm3};

use wokwi_test::config::{
    CHAIN_LEN, DCF77_SAMPLE_US, DISPLAY_INTENSITY, SPI_FREQ_HZ, TICK_INTERVAL_US,
};

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

/// DCF77 receiver data line on GP14. Active-HIGH idle, LOW pulses. The
/// internal pull-up means the pin reads HIGH when no receiver is wired,
/// which keeps the decoder in its initial `SearchingForGap` state (zero
/// churn, no spurious decodes).
pub type Dcf77InPin = Pin<Gpio14, FunctionSio<SioInput>, PullUp>;

/// DCF77 loopback transmitter line on GP13. Push-pull output that
/// re-broadcasts the firmware's current `(hours, minutes)` as a real
/// DCF77 pulse stream — used by the Wokwi sim to drive its own receiver.
/// Only configured when the `dcf77-loopback` Cargo feature is enabled
/// (otherwise GP13 is left untouched and `Board::dcf77_out` is `None`).
pub type Dcf77OutPin = Pin<Gpio13, FunctionSio<SioOutput>, PullDown>;

// === Bring-up ===

/// Everything the RTIC tasks need from configured hardware.
pub struct Board {
    pub display: Display,
    pub button: ButtonPin,
    pub led: LedPin,
    pub dcf77_in: Dcf77InPin,
    /// Loopback TX output. `Some(pin)` only with the `dcf77-loopback`
    /// feature on; `None` in production builds (GP13 is left as-is).
    pub dcf77_out: Option<Dcf77OutPin>,
    pub alarm0: Alarm0,
    pub alarm1: Alarm1,
    pub alarm2: Alarm2,
    pub alarm3: Alarm3,
}

impl Board {
    /// Configure clocks/PLL, GPIO, SPI, the MAX7219 chain, and the four
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

        // alarm2: button debounce window, scheduled after every press
        // (and after release detected by alarm1) to gate the GPIO IRQ
        // re-enable through bouncing.
        let mut alarm2 = timer.alarm_2().unwrap();
        alarm2.enable_interrupt();

        // alarm3: periodic DCF77 receiver poll. 10 ms cadence oversamples
        // the shortest valid pulse (100 ms) by 10x.
        let mut alarm3 = timer.alarm_3().unwrap();
        alarm3.schedule(DCF77_SAMPLE_US.micros()).unwrap();
        alarm3.enable_interrupt();

        let pins = rp_pico::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );

        let led: LedPin = pins.led.into_push_pull_output();

        let button: ButtonPin = pins.gpio15.into_pull_up_input();
        button.set_interrupt_enabled(Interrupt::EdgeLow, true);

        // DCF77 input: polled (no GPIO IRQ wiring). Internal pull-up means
        // "no receiver attached" reads as idle-HIGH, and the decoder
        // stays in `SearchingForGap` until a real receiver drives the
        // line with a minute-marker gap.
        let dcf77_in: Dcf77InPin = pins.gpio14.into_pull_up_input();

        // DCF77 loopback output: only configured with the
        // `dcf77-loopback` feature. Initialise HIGH (idle) so the
        // decoder doesn't see a spurious falling edge before the TX
        // state machine kicks in. Without the feature, GP13 is left
        // in its reset state — `dcf77_out` is `None`.
        #[cfg(feature = "dcf77-loopback")]
        let dcf77_out: Option<Dcf77OutPin> = {
            use embedded_hal::digital::v2::OutputPin;
            let mut pin: Dcf77OutPin = pins.gpio13.into_push_pull_output();
            let _ = pin.set_high();
            Some(pin)
        };
        #[cfg(not(feature = "dcf77-loopback"))]
        let dcf77_out: Option<Dcf77OutPin> = None;

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
            dcf77_in,
            dcf77_out,
            alarm0,
            alarm1,
            alarm2,
            alarm3,
        }
    }
}
