#![no_std]
#![no_main]

use panic_halt as _;
use rtic::app;

mod bsp;
mod clock;
mod config;
mod display;
mod font;

use clock::ClockState;
use config::{
    BUTTON_REPEAT_DECAY_DEN, BUTTON_REPEAT_DECAY_NUM, BUTTON_REPEAT_INITIAL_US,
    BUTTON_REPEAT_MIN_US, CHAIN_LEN, INITIAL_TIME, TICK_INTERVAL_US,
};

#[app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [I2C0_IRQ])]
mod app {
    use super::*;
    use embedded_hal::digital::v2::{InputPin, ToggleableOutputPin};
    use rp_pico::hal::{
        fugit::ExtU32,
        gpio::Interrupt,
        timer::Alarm,
    };

    // Shared resources (accessed by multiple tasks)
    #[shared]
    struct Shared {
        clock: ClockState,
        button: bsp::ButtonPin,
        alarm1: bsp::Alarm1,
        repeat_delay: u32,
    }

    // Local resources (accessed by single tasks)
    #[local]
    struct Local {
        display: bsp::Display,
        led: bsp::LedPin,
        alarm: bsp::Alarm0,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        let bsp::Board {
            display,
            button,
            led,
            alarm0,
            alarm1,
        } = bsp::Board::take(ctx.device);

        let (h, m, s) = INITIAL_TIME;
        (
            Shared {
                clock: ClockState::new(h, m, s),
                button,
                alarm1,
                repeat_delay: BUTTON_REPEAT_INITIAL_US,
            },
            Local {
                display,
                led,
                alarm: alarm0,
            },
            init::Monotonics(),
        )
    }

    // Hardware Task: Timer Interrupt (1Hz)
    #[task(binds = TIMER_IRQ_0, priority = 1, shared = [clock], local = [alarm, led])]
    fn timer_tick(mut ctx: timer_tick::Context) {
        // Clear interrupt and schedule next. A failure here at runtime
        // shouldn't halt the whole firmware via panic-halt — just drop the
        // tick; the next interrupt cycle will recover.
        ctx.local.alarm.clear_interrupt();
        let _ = ctx.local.alarm.schedule(TICK_INTERVAL_US.micros());

        let _ = ctx.local.led.toggle();

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
            b.set_interrupt_enabled(Interrupt::EdgeLow, false);
            b.clear_interrupt(Interrupt::EdgeLow);
        });

        ctx.shared.clock.lock(|c| c.add_minute());

        update_display::spawn().ok();

        // Reset auto-repeat to its initial period and arm alarm1.
        let delay = BUTTON_REPEAT_INITIAL_US;
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

            // Accelerate the repeat: shrink by NUM/DEN, but never below the floor.
            let mut delay = 0;
            ctx.shared.repeat_delay.lock(|d| {
                if *d > BUTTON_REPEAT_MIN_US {
                    *d = (*d as u64 * BUTTON_REPEAT_DECAY_NUM as u64
                        / BUTTON_REPEAT_DECAY_DEN as u64) as u32;
                    if *d < BUTTON_REPEAT_MIN_US {
                        *d = BUTTON_REPEAT_MIN_US;
                    }
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
                b.clear_interrupt(Interrupt::EdgeLow);
                // Re-enable Interrupt
                b.set_interrupt_enabled(Interrupt::EdgeLow, true);
            });
        }
    }

    // Software Task: Update Display (Lower Priority if needed, but here effectively same)
    #[task(shared = [clock], local = [display])]
    fn update_display(mut ctx: update_display::Context) {
        let buffers = ctx
            .shared
            .clock
            .lock(|c| crate::display::clock_to_frame(c).to_devices());

        let display = ctx.local.display;
        for dev_idx in 0..CHAIN_LEN {
            // A transient SPI hiccup shouldn't halt the firmware; the next
            // tick will repaint the display anyway.
            let _ = display.write_raw(dev_idx, &buffers[dev_idx]);
        }
    }
}
