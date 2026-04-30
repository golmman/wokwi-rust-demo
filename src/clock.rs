

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
