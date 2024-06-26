//! Appender to local file
//!
//! # Normal file appender
//!
//! `FileAppender` use `BufWriter` internally to improve IO performance.
//!
//! ```rust
//! # use ftlog::appender::FileAppender;
//! let appender = FileAppender::builder().path("./mylog.log").build();
//! ```
//!
//! # Rotation
//! `ftlog` supports log rotation in local timezone. The available rotation
//! periods are:
//!
//! - minute `Period::Minute`
//! - hour `Period::Hour`
//! - day `Period::Day`
//! - month `Period::Month`
//! - year `Period::Year`
//!
//! ```rust
//! use ftlog::appender::{FileAppender, Period};
//! // rotate every minute
//! let appender = FileAppender::builder()
//!     .path("./mylog.log")
//!     .rotate(Period::Minute)
//!     .build();
//! ```
//!
//! When configured to divide log file by minutes, the file name of log file is in the format of
//! `mylog-{MMMM}{YY}{DD}T{hh}{mm}.log`. When by days, the log file names is
//! something like `mylog-{MMMM}{YY}{DD}.log`.
//!
//! Log filename examples:
//! ```sh
//! $ ls
//! // by minute
//! current-20221026T1351.log
//! // by hour
//! current-20221026T13.log
//! // by day
//! current-20221026.log
//! // by month
//! current-202211.log
//! // by year
//! current-2022.log
//! // omitting extension (e.g. "./log") will add datetime to the end of log filename
//! log-20221026T1353
//! ```
//!
//! ## Rotation and auto delete outdated logs
//!
//! `ftlog` first finds files generated by `ftlog` and cleans outdated logs by
//! last modified time. `ftlog` find generated logs by filename matched by file
//! stem and added datetime.
//!
//! **ATTENTION**: Any files that matchs the pattern will be deleted.
//!
//! ```rust
//! use ftlog::{appender::{Period, FileAppender, Duration}};
//! // clean files named like `current-\d{8}T\d{4}.log`.
//! // files like `another-\d{8}T\d{4}.log` or `current-\d{8}T\d{4}` will not be deleted, since the filenames' stem do not match.
//! // files like `current-\d{8}.log` will remains either, since the rotation durations do not match.
//!
//! // Rotate every day, clean stale logs that were modified 7 days ago on each rotation
//! let appender = FileAppender::builder().path("./mylog.log").rotate(Period::Minute).expire(Duration::days(7)).build();
//! ```
//!
//! ## Rotation timezone
//!
//! By default, rotation is done by local timezone.
//! You can configure appender to use UTC or a fixed timezone when rotates.
//!
//! ```rust
//! use ftlog::appender::{FileAppender, Period};
//! use ftlog::LogTimezone;
//!
//! // Rotate every day by UTC, clean stale logs that were modified 7 days ago on each rotation
//! let appender = FileAppender::builder()
//!     .path("./mylog.log")
//!     .rotate(Period::Minute)
//!     .timezone(LogTimezone::Utc)
//!     .build();
//! ```
#[cfg(not(feature = "tsc"))]
use std::time::Instant;
use std::{
    borrow::Cow,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

#[cfg(feature = "tsc")]
use minstant::Instant;
use time::{Date, Duration, Month, OffsetDateTime, Time, UtcOffset};
use typed_builder::TypedBuilder;

use crate::{local_timezone, LogTimezone};

/// Log rotation frequency
#[derive(Clone, Copy)]
pub enum Period {
    /// rotate log every minute
    Minute,
    /// rotate log every hour
    Hour,
    /// rotate log everyday
    Day,
    /// rotate log every month
    Month,
    /// rotate log every year
    Year,
}
struct Rotate {
    start: Instant,
    wait: Duration,

    period: Period,
    expire: Option<Duration>,
}

#[derive(TypedBuilder)]
#[builder(build_method(vis = "", name = __build), builder_method(vis = ""))]
pub struct FileAppenderBuilder {
    #[builder(setter(transform = |x: impl AsRef<Path>| x.as_ref().to_path_buf()))]
    path: PathBuf,
    #[builder(default, setter(into))]
    rotate: Option<Period>,
    #[builder(default, setter(into))]
    expire: Option<Duration>,
    #[builder(default=LogTimezone::Local)]
    timezone: LogTimezone,
}

#[allow(dead_code, non_camel_case_types, missing_docs)]
#[automatically_derived]
impl<
        __rotate: typed_builder::Optional<Option<Period>>,
        __expire: typed_builder::Optional<Option<Duration>>,
        __timezone: typed_builder::Optional<LogTimezone>,
    > FileAppenderBuilderBuilder<((PathBuf,), __rotate, __expire, __timezone)>
{
    pub fn build(self) -> FileAppender {
        let builder = self.__build();
        match (builder.rotate, builder.expire) {
            // rotate with auto clean
            (Some(period), Some(expire)) => {
                let (start, wait) = FileAppender::until(period, &builder.timezone);
                let path = FileAppender::file(&builder.path, period, &builder.timezone);
                let mut file = BufWriter::new(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .unwrap(),
                );
                let p = builder.path.clone();
                let del_msg = clean_expire_log(p, period, expire);
                if !del_msg.is_empty() {
                    file.write_fmt(format_args!("Log file deleted: {}", del_msg))
                        .unwrap_or_else(|_| {
                            panic!("Write msg to \"{}\" failed", path.to_string_lossy())
                        });
                }
                FileAppender {
                    file,
                    path: builder.path,
                    rotate: Some(Rotate {
                        start,
                        wait,
                        period,
                        expire: Some(expire),
                    }),
                    timezone: builder.timezone,
                }
            }
            // rotate only
            (Some(period), None) => {
                let (start, wait) = FileAppender::until(period, &builder.timezone);
                let path = FileAppender::file(&builder.path, period, &builder.timezone);
                let file = BufWriter::new(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .unwrap(),
                );
                FileAppender {
                    file,
                    path: builder.path,
                    rotate: Some(Rotate {
                        start,
                        wait,
                        period,
                        expire: None,
                    }),
                    timezone: builder.timezone,
                }
            }
            // single file
            _ => FileAppender {
                file: BufWriter::new(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&builder.path)
                        .unwrap_or_else(|_| {
                            panic!(
                                "Fail to create log file: {}",
                                builder.path.to_string_lossy()
                            )
                        }),
                ),
                path: builder.path,
                rotate: None,
                timezone: builder.timezone,
            },
        }
    }
}

/// Appender to local file
pub struct FileAppender {
    file: BufWriter<File>,
    path: PathBuf,
    rotate: Option<Rotate>,
    timezone: LogTimezone,
}

impl FileAppender {
    /// FileAppender builder.
    ///
    /// You can configure file path, rotation period, expire duration and timezone in builder,
    /// and get a corresponding `FileAppender`.
    ///
    /// ```rust
    /// use ftlog::appender::{Duration, FileAppender, Period};
    /// use ftlog::LogTimezone;
    /// use time::UtcOffset;
    ///
    /// let appender = FileAppender::builder()
    ///     .path("./mylog.log")
    ///     .rotate(Period::Day)
    ///     .expire(Duration::days(7))
    ///     .timezone(LogTimezone::Fixed(UtcOffset::from_hms(8, 0, 0).unwrap()))
    ///     .build();
    /// ```
    pub fn builder() -> FileAppenderBuilderBuilder {
        FileAppenderBuilder::builder()
    }

    fn file<T: AsRef<Path>>(path: T, period: Period, timezone: &LogTimezone) -> PathBuf {
        let p = path.as_ref();
        let dt = OffsetDateTime::now_utc().to_offset(Self::offset_from_timezone(timezone));
        let ts = match period {
            Period::Year => format!("{}", dt.year()),
            Period::Month => format!("{}{:02}", dt.year(), dt.month() as u8),
            Period::Day => format!("{}{:02}{:02}", dt.year(), dt.month() as u8, dt.day()),
            Period::Hour => format!(
                "{}{:02}{:02}T{:02}",
                dt.year(),
                dt.month() as u8,
                dt.day(),
                dt.hour()
            ),
            Period::Minute => format!(
                "{}{:02}{:02}T{:02}{:02}",
                dt.year(),
                dt.month() as u8,
                dt.day(),
                dt.hour(),
                dt.minute()
            ),
        };

        if let Some(ext) = p.extension() {
            let file_name = p
                .file_stem()
                .map(|x| format!("{}-{}.{}", x.to_string_lossy(), ts, ext.to_string_lossy()))
                .expect("invalid file name");
            p.with_file_name(file_name)
        } else {
            p.with_file_name(format!(
                "{}-{}",
                p.file_name()
                    .map(|x| x.to_string_lossy())
                    .unwrap_or(Cow::from("log")),
                ts
            ))
        }
    }

    fn offset_from_timezone(timezone: &LogTimezone) -> UtcOffset {
        match timezone {
            LogTimezone::Local => local_timezone(),
            LogTimezone::Utc => UtcOffset::UTC,
            LogTimezone::Fixed(offset) => *offset,
        }
    }

    fn until(period: Period, timezone: &LogTimezone) -> (Instant, Duration) {
        let tm_now = OffsetDateTime::now_utc().to_offset(Self::offset_from_timezone(timezone));
        let now = Instant::now();
        let tm_next = Self::next(&tm_now, period);
        (now, tm_next - tm_now)
    }

    #[inline]
    fn next(now: &OffsetDateTime, period: Period) -> OffsetDateTime {
        let tm_next = match period {
            Period::Year => Date::from_ordinal_date(now.year() + 1, 1)
                .unwrap()
                .with_time(Time::MIDNIGHT),
            Period::Month => {
                let year = if now.month() == Month::December {
                    now.year() + 1
                } else {
                    now.year()
                };
                Date::from_calendar_date(year, now.month().next(), 1)
                    .unwrap()
                    .with_time(Time::MIDNIGHT)
            }
            Period::Day => now.date().with_time(Time::MIDNIGHT) + Duration::DAY,
            Period::Hour => now.date().with_hms(now.time().hour(), 0, 0).unwrap() + Duration::HOUR,
            Period::Minute => {
                let time = now.time();
                now.date().with_hms(time.hour(), time.minute(), 0).unwrap() + Duration::MINUTE
            }
        };
        tm_next.assume_offset(now.offset())
    }

    /// Create a file appender that write log to file
    pub fn new<T: AsRef<Path>>(path: T) -> Self {
        Self::builder().path(path).build()
    }
    /// Create a file appender that rotate a new file every given period
    pub fn rotate<T: AsRef<Path>>(path: T, period: Period) -> Self {
        Self::builder().path(path).rotate(period).build()
    }

    /// Create a file appender that rotate a new file every given period,
    /// auto delete logs that last modified
    /// before expire duration given by `keep` parameter.
    pub fn rotate_with_expire<T: AsRef<Path>>(path: T, period: Period, keep: Duration) -> Self {
        Self::builder()
            .path(path)
            .rotate(period)
            .expire(keep)
            .build()
    }
}

fn clean_expire_log(path: PathBuf, rotate_period: Period, keep_duration: Duration) -> String {
    let dir = path.parent().unwrap().to_path_buf();
    let dir = if dir.is_dir() {
        dir
    } else {
        PathBuf::from(".")
    };
    let to_remove = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|f| f.ok())
        .filter(|x| x.file_type().map(|x| x.is_file()).unwrap_or(false))
        .filter(|x| {
            let p = x.path();
            let name = p.file_stem().unwrap().to_string_lossy();
            if let Some((stem, time)) = name.rsplit_once('-') {
                let check = |(ix, x): (usize, char)| match ix {
                    8 => x == 'T',
                    _ => x.is_ascii_digit(),
                };
                let len = match rotate_period {
                    Period::Minute => time.len() == 13,
                    Period::Hour => time.len() == 11,
                    Period::Day => time.len() == 8,
                    Period::Month => time.len() == 6,
                    Period::Year => time.len() == 4,
                };
                len && time.chars().enumerate().all(check)
                    && path
                        .file_stem()
                        .map(|x| x.to_string_lossy() == stem)
                        .unwrap_or(false)
            } else {
                false
            }
        })
        .filter(|x| {
            x.metadata()
                .ok()
                .and_then(|x| x.modified().ok())
                .map(|time| {
                    time.elapsed()
                        .map(|elapsed| elapsed > keep_duration)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        });

    to_remove
        .filter(|f| std::fs::remove_file(f.path()).is_ok())
        .map(|x| x.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Write for FileAppender {
    fn write(&mut self, record: &[u8]) -> std::io::Result<usize> {
        if let Some(Rotate {
            start,
            wait,
            period,
            expire: keep,
        }) = &mut self.rotate
        {
            if start.elapsed() > *wait {
                // close current file and create new file
                self.file.flush()?;
                let path = Self::file(&self.path, *period, &self.timezone);
                // remove outdated log files
                if let Some(keep_duration) = keep {
                    let keep_duration = *keep_duration;
                    let path = self.path.clone();
                    let period = *period;
                    std::thread::spawn(move || {
                        let del_msg = clean_expire_log(path, period, keep_duration);
                        if !del_msg.is_empty() {
                            crate::info!("Log file deleted: {}", del_msg);
                        }
                    });
                };

                // rotate file
                self.file = BufWriter::new(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .unwrap(),
                );
                (*start, *wait) = Self::until(*period, &self.timezone);
            }
        };
        self.file.write_all(record).map(|_| record.len())
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn format(time: OffsetDateTime) -> String {
        format!(
            "{:0>4}-{:0>2}-{:0>2}T{:0>2}:{:0>2}:{:0>2}.{:0>3}",
            time.year(),
            time.month() as u8,
            time.day(),
            time.hour(),
            time.minute(),
            time.second(),
            time.millisecond()
        )
    }

    #[test]
    fn to_wait_ms() {
        // Mon Oct 24 2022 16:00:00 GMT+0000
        let now = OffsetDateTime::from_unix_timestamp(1666627200).unwrap();

        let tm_next = FileAppender::next(&now, Period::Year);
        let tm = OffsetDateTime::from_unix_timestamp(1672531200).unwrap();
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));

        let tm_next = FileAppender::next(&now, Period::Month);
        let tm = OffsetDateTime::from_unix_timestamp(1667260800).unwrap();
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));

        let tm_next = FileAppender::next(&now, Period::Day);
        let tm = OffsetDateTime::from_unix_timestamp(1666656000).unwrap();
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));

        let tm_next = FileAppender::next(&now, Period::Hour);
        let tm = OffsetDateTime::from_unix_timestamp(1666630800).unwrap();
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));

        let tm_next = FileAppender::next(&now, Period::Minute);
        let tm = OffsetDateTime::from_unix_timestamp(1666627260).unwrap();
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));

        // edge case: last day of the month
        let date = Date::from_calendar_date(2023, Month::January, 31).unwrap();
        let dt = date.with_time(Time::MIDNIGHT).assume_offset(now.offset());
        let tm_next = FileAppender::next(&dt, Period::Day);
        let tm = dt + Duration::DAY;
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));

        // edge case: last month of the year
        let date = Date::from_calendar_date(2022, Month::December, 1).unwrap();
        let dt = date.with_time(Time::MIDNIGHT).assume_offset(now.offset());
        let tm_next = FileAppender::next(&dt, Period::Month);
        let tm = Date::from_calendar_date(2023, Month::January, 1)
            .unwrap()
            .with_hms(0, 0, 0)
            .unwrap()
            .assume_offset(now.offset());
        assert_eq!(tm_next, tm, "{} != {}", format(now), format(tm_next));
    }
}
