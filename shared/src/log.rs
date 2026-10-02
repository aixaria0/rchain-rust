//! Logging facade.
//!
//! Mirrors `shared/src/main/scala/coop/rchain/shared/Log.scala`. The Scala `F[_]` effect (a
//! `Sync[F].delay` around slf4j) is simplified to synchronous calls, matching the crate's sync
//! `store` convention. The no-op instance (port of `Log.NOPLog`) is the load-bearing piece here.
//!
//! **The stderr logger carries the wall clock and a level (AUDIT C145).** It used to write
//! `INFO  [class] msg` with no time at all, and its `debug`/`trace` were empty bodies with
//! `is_trace_enabled` hardcoded `false` — so every debug/trace diagnostic in the tree was
//! unreachable in the shipped binary, and incident forensics on one node degraded to ordering-only:
//! cross-node timeline reconstruction, the common case for a consensus stall or a refused block,
//! was impossible from the logs alone. Both halves are here now, and the level is chosen by the
//! operator (`--log-level`, see `node/src/configuration/commandline/options.rs`).

use std::time::Duration;

// The wasm build takes the host's clock (`web-time`); `std`'s panics there (issue #98).
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
use web_time::{SystemTime, UNIX_EPOCH};

/// Identifies the source class of a log message (port of `LogSource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogSource {
    pub class_name: &'static str,
}

impl LogSource {
    pub const fn new(class_name: &'static str) -> Self {
        Self { class_name }
    }
}

/// A log level, lowest first, so the comparison that gates a line is the enum's own `Ord`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    /// The level's name as it appears in a log line.
    pub const fn name(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// Parse an operator's spelling of a level (case-insensitive). The error names the accepted
    /// values, because a typo here is a node whose logs are quieter than the operator believes.
    pub fn parse(value: &str) -> Result<Level, String> {
        match value.to_ascii_lowercase().as_str() {
            "error" => Ok(Level::Error),
            "warn" | "warning" => Ok(Level::Warn),
            "info" => Ok(Level::Info),
            "debug" => Ok(Level::Debug),
            "trace" => Ok(Level::Trace),
            other => Err(format!(
                "unknown log level '{other}': expected one of error, warn, info, debug, trace"
            )),
        }
    }
}

/// A logger (port of `Log[F]`).
pub trait Log: Send + Sync {
    fn is_trace_enabled(&self, source: LogSource) -> bool;
    fn trace(&self, source: LogSource, msg: &str);
    fn debug(&self, source: LogSource, msg: &str);
    fn info(&self, source: LogSource, msg: &str);
    fn warn(&self, source: LogSource, msg: &str);
    fn error(&self, source: LogSource, msg: &str);
}

/// No-op logger (port of `Log.NOPLog`).
#[derive(Default)]
pub struct NopLog;

impl Log for NopLog {
    fn is_trace_enabled(&self, _source: LogSource) -> bool {
        false
    }
    fn trace(&self, _source: LogSource, _msg: &str) {}
    fn debug(&self, _source: LogSource, _msg: &str) {}
    fn info(&self, _source: LogSource, _msg: &str) {}
    fn warn(&self, _source: LogSource, _msg: &str) {}
    fn error(&self, _source: LogSource, _msg: &str) {}
}

/// A logger writing to stderr, at or below a chosen level (a concrete `Log` for tests and CLI use).
pub struct StderrLog {
    level: Level,
}

impl Default for StderrLog {
    /// `info` — what the node logged before there was a choice, so an operator who sets nothing sees
    /// exactly the lines they saw before.
    fn default() -> Self {
        Self { level: Level::Info }
    }
}

impl StderrLog {
    pub const fn new(level: Level) -> Self {
        Self { level }
    }

    /// Whether a line at `level` is emitted. The comparison is the enum's derived order, so a
    /// `trace` logger enables everything and an `error` logger emits only errors.
    pub fn enabled(&self, level: Level) -> bool {
        level <= self.level
    }

    fn emit(&self, level: Level, source: LogSource, msg: &str) {
        if !self.enabled(level) {
            return;
        }
        eprintln!(
            "{ts} {level:<5} [{class}] {msg}",
            ts = utc_timestamp(SystemTime::now()),
            level = level.name(),
            class = source.class_name,
            msg = msg
        );
    }
}

impl Log for StderrLog {
    fn is_trace_enabled(&self, _source: LogSource) -> bool {
        self.enabled(Level::Trace)
    }
    fn trace(&self, source: LogSource, msg: &str) {
        self.emit(Level::Trace, source, msg);
    }
    fn debug(&self, source: LogSource, msg: &str) {
        self.emit(Level::Debug, source, msg);
    }
    fn info(&self, source: LogSource, msg: &str) {
        self.emit(Level::Info, source, msg);
    }
    fn warn(&self, source: LogSource, msg: &str) {
        self.emit(Level::Warn, source, msg);
    }
    fn error(&self, source: LogSource, msg: &str) {
        self.emit(Level::Error, source, msg);
    }
}

/// The instant as an ISO-8601 UTC timestamp with milliseconds (`2026-09-27T12:34:56.789Z`).
///
/// Takes the clock as an argument so the calendar below is testable against a fixed instant rather
/// than against whatever time the test ran.
///
/// **The arithmetic is `div_euclid`/`rem_euclid`, deliberately.** A calendar wants *floor* division:
/// the day count is negative for any instant before 1970, and floor division is what keeps the
/// time-of-day and the date consistent through the epoch (C's truncating division does not, which is
/// why the reference algorithm is written this way). Saying so in the spelling is the point.
pub fn utc_timestamp(now: SystemTime) -> String {
    let since_epoch = match now.duration_since(UNIX_EPOCH) {
        Ok(d) => d,
        // A clock behind the epoch is reported as the epoch rather than panicking: a node with a
        // wrong clock is a node whose logs are hard to read, not one that should die.
        Err(_) => Duration::ZERO,
    };
    // `try_from` rather than `as`: a `u64 -> i64` cast wraps to a negative instant at the far end of
    // the range, and a clamp that says so is a better answer than a date in 1700-and-something. The
    // branch is unreachable for any real clock (year 292 billion).
    let secs = i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX);
    let millis = since_epoch.subsec_millis();

    let days = secs.div_euclid(86_400);
    let second_of_day = secs.rem_euclid(86_400);
    let (hour, minute, second) = (
        second_of_day.div_euclid(3600),
        second_of_day.rem_euclid(3600).div_euclid(60),
        second_of_day.rem_euclid(60),
    );
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Days since 1970-01-01 to a proleptic-Gregorian `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, with the constant names kept: the shift to a March-based year
/// is what makes the leap day the *last* day of the year, which is what removes every special case
/// from the month arithmetic.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    const DAYS_FROM_0000_TO_1970: i64 = 719_468;
    let z = days + DAYS_FROM_0000_TO_1970;
    let era = z.div_euclid(146_097); // 146_097 days per 400-year era
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era.div_euclid(1460) + day_of_era.div_euclid(36_524)
        - day_of_era.div_euclid(146_096))
    .div_euclid(365);
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100));
    let month_index = (5 * day_of_year + 2).div_euclid(153); // 153 days per 5-month block
    let day = day_of_year - (153 * month_index + 2).div_euclid(5) + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nop_log_never_traces() {
        let log = NopLog;
        let source = LogSource::new("test");
        assert!(!log.is_trace_enabled(source));
        log.error(source, "ignored");
    }

    /// **AUDIT C145.** The wall clock on the line is the half a reader reconstructs a cross-node
    /// timeline from, so it is pinned against a fixed instant rather than eyeballed. `1_700_000_000`
    /// is 2023-11-14T22:13:20Z; the millisecond is asserted separately because it is the part that
    /// orders two lines written in the same second.
    #[test]
    fn a_log_line_carries_the_wall_clock_in_utc() {
        let fixed = UNIX_EPOCH + Duration::new(1_700_000_000, 123_000_000);
        assert_eq!(utc_timestamp(fixed), "2023-11-14T22:13:20.123Z");

        // The epoch itself, and a leap day — the two the calendar arithmetic gets wrong if the
        // March-based shift or the floor division is dropped.
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00.000Z"
        );
    }

    /// The level is a threshold in one direction only: a logger at `info` emits `info` and above, and
    /// **not** `debug`. Getting this backwards is the failure that matters — it would leave the
    /// debug/trace diagnostics unreachable again, which is the defect this closed.
    #[test]
    fn the_level_gates_the_line_and_a_trace_logger_traces() {
        let info = StderrLog::default();
        assert!(info.enabled(Level::Error));
        assert!(info.enabled(Level::Info));
        assert!(!info.enabled(Level::Debug));
        assert!(!info.is_trace_enabled(LogSource::new("test")));

        let trace = StderrLog::new(Level::Trace);
        assert!(trace.enabled(Level::Trace));
        assert!(trace.enabled(Level::Error));
        assert!(trace.is_trace_enabled(LogSource::new("test")));

        let errors_only = StderrLog::new(Level::Error);
        assert!(errors_only.enabled(Level::Error));
        assert!(!errors_only.enabled(Level::Warn));
    }

    /// The operator's spelling, including the one that must be refused: a typo has to be an error
    /// naming the accepted values, not a silent fall back to the default.
    #[test]
    fn a_level_is_parsed_from_the_operators_spelling_and_a_typo_is_refused() {
        assert_eq!(Level::parse("info"), Ok(Level::Info));
        assert_eq!(Level::parse("DEBUG"), Ok(Level::Debug));
        assert_eq!(Level::parse("warning"), Ok(Level::Warn));
        assert_eq!(Level::parse("trace"), Ok(Level::Trace));
        let err = Level::parse("verbose").expect_err("a typo must not be accepted");
        assert!(err.contains("verbose") && err.contains("debug"), "{err}");
    }
}
