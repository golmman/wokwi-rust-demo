#![no_std]
#![no_main]

use panic_halt as _;
use max7219::MAX7219;
use rtic::app;

mod font;
mod clock;
mod display;

use clock::ClockState;

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
    use embedded_hal::digital::v2::{InputPin, ToggleableOutputPin};

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
        button: Pin<Gpio15, FunctionSio<SioInput>, PullUp>,
        alarm1: rp_pico::hal::timer::Alarm1,
        repeat_delay: u32,
    }

    // Local resources (accessed by single tasks)
    #[local]
    struct Local {
        display: DisplayType,
        led: rp_pico::hal::gpio::Pin<rp_pico::hal::gpio::bank0::Gpio25, rp_pico::hal::gpio::FunctionSio<rp_pico::hal::gpio::SioOutput>, rp_pico::hal::gpio::PullDown>,
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

        let mut alarm1 = timer.alarm_1().unwrap();
        alarm1.enable_interrupt();

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
                clock: ClockState::new(12, 34, 56),
                button,
                alarm1,
                repeat_delay: 500_000,
            },
            Local {
                display,
                led,
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
        ctx.shared.clock.lock(|c| c.tick());

        // Spawn display update
        update_display::spawn().ok();
    }

    // Hardware Task: GPIO Interrupt (Button Press)
    #[task(binds = IO_IRQ_BANK0, priority = 1, shared = [clock, button, alarm1, repeat_delay])]
    fn button_press(mut ctx: button_press::Context) {
        // Initial Press
        
        // Disable interrupt to prevent bouncing re-entry
        ctx.shared.button.lock(|b| {
            b.set_interrupt_enabled(rp_pico::hal::gpio::Interrupt::EdgeLow, false);
            b.clear_interrupt(rp_pico::hal::gpio::Interrupt::EdgeLow);
        });

        ctx.shared.clock.lock(|c| c.add_minute());

        update_display::spawn().ok();

        // Initialize repeat delay and schedule repeat task
        let delay = 500_000; // Start with 500ms
        ctx.shared.repeat_delay.lock(|d| *d = delay);
        
        ctx.shared.alarm1.lock(|a| {
            a.clear_interrupt();
            a.schedule(delay.micros()).ok(); // Ignore if already running, though shouldn't be
        });
    }

    // Hardware Task: Button Repeat (Timer 1)
    #[task(binds = TIMER_IRQ_1, priority = 1, shared = [clock, button, alarm1, repeat_delay])]
    fn button_repeat(mut ctx: button_repeat::Context) {
        // Clear alarm interrupt first
        ctx.shared.alarm1.lock(|a| a.clear_interrupt());

        let is_held = ctx.shared.button.lock(|b| b.is_low().unwrap_or(false));

        if is_held {
            // Button is still held, update clock
             ctx.shared.clock.lock(|c| c.add_minute());
            update_display::spawn().ok();

            // Accelerate
            let mut delay = 0;
            ctx.shared.repeat_delay.lock(|d| {
                if *d > 20_000 { // Min 20ms
                     *d = (*d as u64 * 8 / 10) as u32; // Decrease by 20%
                     if *d < 20_000 { *d = 20_000; }
                }
                delay = *d;
            });

            // Schedule next repeat
            ctx.shared.alarm1.lock(|a| {
                a.schedule(delay.micros()).ok();
            });

        } else {
            // Button released
            ctx.shared.button.lock(|b| {
                // Clear any pending gpio interrupt flags that might have accumulated during bounce
                b.clear_interrupt(rp_pico::hal::gpio::Interrupt::EdgeLow);
                // Re-enable Interrupt
                b.set_interrupt_enabled(rp_pico::hal::gpio::Interrupt::EdgeLow, true);
            });
        }
    }

    // Software Task: Update Display (Lower Priority if needed, but here effectively same)
    #[task(shared = [clock], local = [display])]
    fn update_display(mut ctx: update_display::Context) {
        let buffers = ctx.shared.clock.lock(|c| crate::display::prepare_buffer(c));

        let display = ctx.local.display;
        for dev_idx in 0..4 {
            display.write_raw(dev_idx, &buffers[dev_idx as usize]).unwrap();
        }
    }
}
