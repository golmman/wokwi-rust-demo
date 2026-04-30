/// 24-hour wall-clock state. Values are clamped on construction so the
/// invariants `hours < 24`, `mins < 60`, `secs < 60` always hold; consumers
/// can read fields through the `hours()`/`mins()`/`secs()` accessors and
/// only mutate via `tick()` / `add_second()` / `add_minute()` / `add_hour()`,
/// which preserve those invariants.
pub struct ClockState {
    hours: u8,
    mins: u8,
    secs: u8,
}

impl ClockState {
    /// Build a clock at `hh:mm:ss`. Out-of-range fields are wrapped (`hours
    /// %= 24`, `mins %= 60`, `secs %= 60`) so a caller can't get a clock
    /// that violates the invariants by constructing one with bad data.
    pub const fn new(hours: u8, mins: u8, secs: u8) -> Self {
        Self {
            hours: hours % 24,
            mins: mins % 60,
            secs: secs % 60,
        }
    }

    pub const fn hours(&self) -> u8 {
        self.hours
    }
    pub const fn mins(&self) -> u8 {
        self.mins
    }
    pub const fn secs(&self) -> u8 {
        self.secs
    }

    /// Advance the clock by one second, rolling over to the next minute /
    /// hour as required.
    pub fn tick(&mut self) {
        self.add_second();
    }

    /// Increment the seconds counter, cascading into minute/hour rollover.
    pub fn add_second(&mut self) {
        self.secs += 1;
        if self.secs >= 60 {
            self.secs = 0;
            self.add_minute();
        }
    }

    /// Increment the minutes counter, cascading into hour rollover.
    pub fn add_minute(&mut self) {
        self.mins += 1;
        if self.mins >= 60 {
            self.mins = 0;
            self.add_hour();
        }
    }

    /// Increment the hours counter, wrapping at 24.
    pub fn add_hour(&mut self) {
        self.hours = (self.hours + 1) % 24;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_advances_seconds() {
        let mut c = ClockState::new(12, 34, 56);
        c.tick();
        assert_eq!((c.hours(), c.mins(), c.secs()), (12, 34, 57));
    }

    #[test]
    fn tick_rolls_seconds_into_minute() {
        let mut c = ClockState::new(12, 34, 59);
        c.tick();
        assert_eq!((c.hours(), c.mins(), c.secs()), (12, 35, 0));
    }

    #[test]
    fn tick_rolls_minute_into_hour() {
        let mut c = ClockState::new(12, 59, 59);
        c.tick();
        assert_eq!((c.hours(), c.mins(), c.secs()), (13, 0, 0));
    }

    #[test]
    fn tick_wraps_24_hour_day() {
        let mut c = ClockState::new(23, 59, 59);
        c.tick();
        assert_eq!((c.hours(), c.mins(), c.secs()), (0, 0, 0));
    }

    #[test]
    fn new_clamps_out_of_range_input() {
        // 99h % 24 = 3, 99m % 60 = 39, 99s % 60 = 39.
        let c = ClockState::new(99, 99, 99);
        assert_eq!((c.hours(), c.mins(), c.secs()), (3, 39, 39));
        // Crucially, subsequent ticks shouldn't panic on an oversized field.
        let mut c = c;
        for _ in 0..10 {
            c.tick();
        }
        assert!(c.hours() < 24 && c.mins() < 60 && c.secs() < 60);
    }

    #[test]
    fn sixty_add_minute_equals_one_add_hour() {
        let mut a = ClockState::new(12, 0, 0);
        let mut b = ClockState::new(12, 0, 0);
        for _ in 0..60 {
            a.add_minute();
        }
        b.add_hour();
        assert_eq!(
            (a.hours(), a.mins(), a.secs()),
            (b.hours(), b.mins(), b.secs())
        );
    }
}
