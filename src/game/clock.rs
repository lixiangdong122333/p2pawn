//! A simple chess clock: base time + increment per move.

use std::time::{Duration, Instant};

/// Two-sided clock. `running` is the side whose time is currently ticking.
#[derive(Clone, Debug)]
pub struct Clock {
    /// [white, black] remaining time.
    remaining: [Duration; 2],
    increment: Duration,
    running: Option<chess::Color>,
    last_tick: Instant,
    /// Freeze the clock once the game is over.
    frozen: bool,
}

impl Clock {
    pub fn new(base: Duration, increment: Duration) -> Clock {
        Clock {
            remaining: [base, base],
            increment,
            running: None,
            last_tick: Instant::now(),
            frozen: false,
        }
    }

    /// Advance the running clock by real elapsed time. Call regularly.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let elapsed = now - self.last_tick;
        self.last_tick = now;
        if self.frozen {
            return;
        }
        if let Some(color) = self.running {
            let i = idx(color);
            self.remaining[i] = self.remaining[i].saturating_sub(elapsed);
        }
    }

    /// Stop the mover's clock, credit the increment, start the opponent's.
    pub fn switch(&mut self, mover: chess::Color) {
        if self.frozen {
            return;
        }
        let i = idx(mover);
        self.remaining[i] += self.increment;
        self.running = Some(!mover);
        self.last_tick = Instant::now();
    }

    /// Start the given side's clock (used when the game begins).
    pub fn start(&mut self, color: chess::Color) {
        if !self.frozen {
            self.running = Some(color);
            self.last_tick = Instant::now();
        }
    }

    /// Overwrite one side's remaining time (clock sync from a network move).
    pub fn set_remaining(&mut self, color: chess::Color, dur: Duration) {
        self.remaining[idx(color)] = dur;
    }

    pub fn remaining(&self, color: chess::Color) -> Duration {
        self.remaining[idx(color)]
    }

    /// The side whose clock is currently ticking.
    #[cfg(test)]
    pub fn running(&self) -> Option<chess::Color> {
        self.running
    }

    /// The side whose clock has hit zero, if any.
    pub fn flagged(&self) -> Option<chess::Color> {
        if self.frozen {
            return None;
        }
        [chess::Color::White, chess::Color::Black]
            .into_iter()
            .find(|&c| self.running == Some(c) && self.remaining[idx(c)].is_zero())
    }

    pub fn freeze(&mut self) {
        self.tick();
        self.frozen = true;
        self.running = None;
    }

    /// "5+3" style label.
    pub fn label(base: Duration, inc: Duration) -> String {
        format!("{}+{}", base.as_secs(), inc.as_secs())
    }
}

fn idx(c: chess::Color) -> usize {
    match c {
        chess::Color::White => 0,
        chess::Color::Black => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_and_increment() {
        let mut c = Clock::new(Duration::from_secs(60), Duration::from_secs(2));
        c.start(chess::Color::White);
        std::thread::sleep(Duration::from_millis(50));
        c.switch(chess::Color::White);
        assert!(c.remaining(chess::Color::White) > Duration::from_secs(60));
        assert!(c.remaining(chess::Color::White) <= Duration::from_secs(62));
        assert_eq!(c.running(), Some(chess::Color::Black));
    }

    #[test]
    fn flag_detection() {
        let mut c = Clock::new(Duration::from_millis(30), Duration::from_secs(0));
        c.start(chess::Color::White);
        std::thread::sleep(Duration::from_millis(60));
        c.tick();
        assert_eq!(c.flagged(), Some(chess::Color::White));
        c.freeze();
        assert_eq!(c.flagged(), None);
    }
}
