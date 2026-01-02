

/// Shared state for the clock
pub struct ClockState {
    pub hours: u8,
    pub mins: u8,
    pub secs: u8,
}

impl ClockState {
    pub fn new(hours: u8, mins: u8, secs: u8) -> Self {
        Self { hours, mins, secs }
    }

    /// Increments the second. Returns true if minute also changed (display update needed).
    pub fn tick(&mut self) {
        self.secs += 1;
        if self.secs >= 60 {
            self.secs = 0;
            self.add_minute();
        }
    }

    /// Increments the minute. Handles rollover to hours.
    pub fn add_minute(&mut self) {
        self.mins += 1;
        if self.mins >= 60 {
            self.mins = 0;
            self.hours = (self.hours + 1) % 24;
        }
    }
}
