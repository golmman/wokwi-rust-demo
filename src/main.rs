#![no_std]
#![no_main]

use panic_halt as _;
use rtic::app;

mod bsp;

use wokwi_test::clock::ClockState;
use wokwi_test::config::{
    BUTTON_DEBOUNCE_US, BUTTON_REPEAT_DECAY_DEN, BUTTON_REPEAT_DECAY_NUM, BUTTON_REPEAT_INITIAL_US,
    BUTTON_REPEAT_MIN_US, INITIAL_TIME, TICK_INTERVAL_US,
};
use wokwi_test::display;

#[app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [I2C0_IRQ])]
mod app {
    use super::*;
    use embedded_hal::digital::v2::{InputPin, ToggleableOutputPin};
    use rp_pico::hal::{fugit::ExtU32, gpio::Interrupt, timer::Alarm};

    // Shared resources (accessed by multiple tasks)
    #[shared]
    struct Shared {
        clock: ClockState,
        button: bsp::ButtonPin,
        alarm1: bsp::Alarm1,
        alarm2: bsp::Alarm2,
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
            alarm2,
        } = bsp::Board::take(ctx.device);

        let (h, m, s) = INITIAL_TIME;
        (
            Shared {
                clock: ClockState::new(h, m, s),
                button,
                alarm1,
                alarm2,
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
    #[task(binds = IO_IRQ_BANK0, priority = 1, shared = [clock, button, alarm1, alarm2, repeat_delay])]
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

        // Arm alarm2 for the short debounce — re-enables the GPIO IRQ once
        // bouncing has settled, so a fast re-click registers as a fresh
        // event instead of being eaten by the long auto-repeat-arming
        // window.
        ctx.shared.alarm2.lock(|a| {
            a.clear_interrupt();
            a.schedule(BUTTON_DEBOUNCE_US.micros()).ok();
        });
    }

    // Hardware Task: Button Repeat (Timer 1)
    #[task(binds = TIMER_IRQ_1, priority = 1, shared = [clock, button, alarm1, alarm2, repeat_delay])]
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
            // Button has been released. Hand off to alarm2 so any
            // release-side contact bouncing settles before we re-enable
            // the GPIO IRQ.
            ctx.shared.alarm2.lock(|a| {
                a.clear_interrupt();
                a.schedule(BUTTON_DEBOUNCE_US.micros()).ok();
            });
        }
    }

    // Hardware Task: Button Debounce (Timer 2)
    #[task(binds = TIMER_IRQ_2, priority = 1, shared = [button, alarm2])]
    fn button_debounce(mut ctx: button_debounce::Context) {
        ctx.shared.alarm2.lock(|a| a.clear_interrupt());

        let is_held = ctx.shared.button.lock(|b| b.is_low().unwrap_or(false));

        if is_held {
            // Still held — keep polling. The GPIO IRQ stays disabled so
            // press-side contact bounce can't fire spurious events; we'll
            // re-enable it once we see the button settled HIGH.
            ctx.shared.alarm2.lock(|a| {
                a.schedule(BUTTON_DEBOUNCE_US.micros()).ok();
            });
            return;
        }

        // Button is settled HIGH. Clear any latched edge from contact
        // bounce and re-enable EdgeLow so the next press registers as a
        // fresh event.
        ctx.shared.button.lock(|b| {
            b.clear_interrupt(Interrupt::EdgeLow);
            b.set_interrupt_enabled(Interrupt::EdgeLow, true);
        });
    }

    // Software Task: Update Display (Lower Priority if needed, but here effectively same)
    #[task(shared = [clock], local = [display])]
    fn update_display(mut ctx: update_display::Context) {
        let buffers = ctx
            .shared
            .clock
            .lock(|c| display::clock_to_frame(c).to_devices());

        let display = ctx.local.display;
        for (dev_idx, buf) in buffers.iter().enumerate() {
            // A transient SPI hiccup shouldn't halt the firmware; the next
            // tick will repaint the display anyway.
            let _ = display.write_raw(dev_idx, buf);
        }
    }
}
