//! Session log in `log/AAAA-MM-JJ_HHMMSS.log` (constitution, principe IV).
//!
//! Every line is `[2026-10-29T12:34:56Z] KIND text`, masked by the [`Redactor`]. A file is
//! capped at `file_max` bytes (one marker line, then nothing more), the folder at
//! `total_max` bytes (oldest session logs deleted first). Logging is never fatal.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::home::Home;
use crate::secret::Redactor;
use crate::text;

/// Room kept at the end of a file for the cap marker line.
pub const MARKER_RESERVE: u64 = 256;

/// The session log; disabled in ephemeral mode or when the folder cannot be written.
pub struct SessionLog {
    inner: Option<LogFile>,
}

/// Never prints the redactor (it holds the secrets in clear).
impl std::fmt::Debug for SessionLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionLog")
            .field("path", &self.path())
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

struct LogFile {
    file: File,
    path: PathBuf,
    redactor: Redactor,
    written: u64,
    file_max: u64,
    stopped: bool,
}

impl SessionLog {
    /// Opens a new session file in `home/log/` after rotation. Never fails: on any problem
    /// (ephemeral home, unwritable folder), returns a disabled log.
    pub fn open(home: &Home, redactor: Redactor, file_max: u64, total_max: u64) -> SessionLog {
        if home.ephemeral() {
            return SessionLog::disabled();
        }
        SessionLog::open_in(
            &home.log_dir(),
            redactor,
            file_max,
            total_max,
            SystemTime::now(),
        )
    }

    fn open_in(
        dir: &Path,
        redactor: Redactor,
        file_max: u64,
        total_max: u64,
        now: SystemTime,
    ) -> SessionLog {
        if fs::create_dir_all(dir).is_err() {
            return SessionLog::disabled();
        }
        rotate(dir, file_max, total_max);
        let stamp = file_stamp(now);
        for i in 0..100u32 {
            let name = if i == 0 {
                format!("{stamp}.log")
            } else {
                format!("{stamp}-{i}.log")
            };
            let path = dir.join(name);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return SessionLog {
                        inner: Some(LogFile {
                            file,
                            path,
                            redactor,
                            written: 0,
                            file_max,
                            stopped: false,
                        }),
                    };
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        SessionLog::disabled()
    }

    /// A log that writes nothing.
    pub fn disabled() -> SessionLog {
        SessionLog { inner: None }
    }

    /// Appends `[timestamp] KIND text` (one physical line per line of `text`), masked.
    pub fn line(&mut self, kind: &str, text: &str) {
        let Some(log) = self.inner.as_mut() else {
            return;
        };
        if log.stopped {
            return;
        }
        let ts = utc_timestamp(SystemTime::now());
        let kind = log.redactor.redact(kind);
        let body = log.redactor.redact(text);
        let body = body
            .strip_suffix("\r\n")
            .or_else(|| body.strip_suffix('\n'))
            .unwrap_or(&body);
        let mut out = String::with_capacity(body.len() + 32);
        for l in body.split('\n') {
            let l = l.strip_suffix('\r').unwrap_or(l);
            out.push('[');
            out.push_str(&ts);
            out.push_str("] ");
            out.push_str(&kind);
            if !l.is_empty() {
                out.push(' ');
                out.push_str(l);
            }
            out.push('\n');
        }
        let len = out.len() as u64;
        if log.written + len + MARKER_RESERVE <= log.file_max {
            log.write(&out);
            return;
        }
        let marker = format!("[{ts}] LOG {}\n", text::log_capped(log.file_max));
        if log.written + marker.len() as u64 <= log.file_max {
            log.write(&marker);
        }
        log.stopped = true;
    }

    /// Path of the session file, if logging is enabled.
    pub fn path(&self) -> Option<&Path> {
        self.inner.as_ref().map(|l| l.path.as_path())
    }

    /// True when lines are being written.
    pub fn is_enabled(&self) -> bool {
        self.inner.as_ref().is_some_and(|l| !l.stopped)
    }
}

impl LogFile {
    fn write(&mut self, s: &str) {
        match self.file.write_all(s.as_bytes()) {
            Ok(()) => self.written += s.len() as u64,
            Err(_) => self.stopped = true,
        }
    }
}

/// Deletes the oldest session logs until the existing ones plus a full new file fit in
/// `total_max`. Files that do not look like session logs are never touched.
fn rotate(dir: &Path, file_max: u64, total_max: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<((String, u32), PathBuf, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let key = parse_log_name(&name)?;
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| (key, entry.path(), meta.len()))
        })
        .collect();
    logs.sort();
    let mut total: u64 = logs.iter().map(|(_, _, size)| *size).sum();
    for (_, path, size) in logs {
        if total.saturating_add(file_max) <= total_max {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

/// `AAAA-MM-JJ_HHMMSS.log` → (stamp, 0) ; `AAAA-MM-JJ_HHMMSS-N.log` → (stamp, N).
fn parse_log_name(name: &str) -> Option<(String, u32)> {
    let stem = name.strip_suffix(".log")?;
    let stamp = stem.get(..17)?;
    let suffix = match stem.get(17..)? {
        "" => 0,
        rest => {
            let digits = rest.strip_prefix('-')?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok()?
        }
    };
    let ok = stamp.bytes().enumerate().all(|(i, c)| match i {
        4 | 7 => c == b'-',
        10 => c == b'_',
        _ => c.is_ascii_digit(),
    });
    ok.then(|| (stamp.to_string(), suffix))
}

/// `2026-10-29T12:34:56Z`
pub fn utc_timestamp(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = civil(t);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `2026-10-29_123456`
pub fn file_stamp(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = civil(t);
    format!("{y:04}-{mo:02}-{d:02}_{h:02}{mi:02}{s:02}")
}

/// Whole seconds since the Unix epoch, rounded down (also before 1970).
fn unix_seconds(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => {
            let d = e.duration();
            let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
            if d.subsec_nanos() > 0 {
                -secs - 1
            } else {
                -secs
            }
        }
    }
}

/// (year, month, day, hour, minute, second) in UTC.
fn civil(t: SystemTime) -> (i64, u32, u32, u32, u32, u32) {
    let secs = unix_seconds(t);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;
    (y, m, d, h, mi, s)
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 → proleptic Gregorian date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::test_support::TempDir;
    use crate::secret::MASK;
    use std::time::Duration;

    fn at(secs: i64) -> SystemTime {
        if secs >= 0 {
            UNIX_EPOCH + Duration::from_secs(secs as u64)
        } else {
            UNIX_EPOCH - Duration::from_secs(secs.unsigned_abs())
        }
    }

    #[test]
    fn timestamps_on_known_epochs() {
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (-1, "1969-12-31T23:59:59Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_000_000_000, "2001-09-09T01:46:40Z"),
            (1_234_567_890, "2009-02-13T23:31:30Z"),
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_735_689_599, "2024-12-31T23:59:59Z"),
            (1_792_585_296, "2026-10-21T12:21:36Z"),
            (1_793_277_296, "2026-10-29T12:34:56Z"),
            (4_102_444_799, "2099-12-31T23:59:59Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
            (-2_208_988_800, "1900-01-01T00:00:00Z"),
            (-11_644_473_600, "1601-01-01T00:00:00Z"),
        ];
        for (secs, expected) in cases {
            assert_eq!(utc_timestamp(at(secs)), expected, "{secs}");
        }
    }

    #[test]
    fn civil_from_days_on_known_days() {
        // Times before 1601 cannot be a `SystemTime` on Windows: test the algorithm itself.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(-141_427), (1582, 10, 15));
        assert_eq!(civil_from_days(-719_468), (0, 3, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_755), (2026, 10, 29));
        // Every day of a 400-year cycle round-trips through a simple day counter.
        let mut date = (1970i64, 1u32, 1u32);
        for days in 0..146_097 {
            assert_eq!(civil_from_days(days), date, "{days}");
            date = next_day(date);
        }
    }

    fn next_day((y, m, d): (i64, u32, u32)) -> (i64, u32, u32) {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = match m {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        match (d < len, m < 12) {
            (true, _) => (y, m, d + 1),
            (false, true) => (y, m + 1, 1),
            (false, false) => (y + 1, 1, 1),
        }
    }

    #[test]
    fn sub_second_times_round_down() {
        let t = UNIX_EPOCH + Duration::from_millis(1_793_277_296_999);
        assert_eq!(utc_timestamp(t), "2026-10-29T12:34:56Z");
        let before = UNIX_EPOCH - Duration::from_millis(500);
        assert_eq!(utc_timestamp(before), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn file_stamps() {
        assert_eq!(file_stamp(at(1_793_277_296)), "2026-10-29_123456");
        assert_eq!(file_stamp(at(0)), "1970-01-01_000000");
    }

    fn home_in(t: &TempDir) -> Home {
        Home::at(t.path().to_path_buf())
    }

    fn read_log(log: &SessionLog) -> String {
        fs::read_to_string(log.path().unwrap()).unwrap()
    }

    #[test]
    fn open_creates_a_timestamped_file_in_log_dir() {
        let t = TempDir::new("log-open");
        let log = SessionLog::open(&home_in(&t), Redactor::default(), 1 << 20, 1 << 22);
        assert!(log.is_enabled());
        let path = log.path().unwrap().to_path_buf();
        assert_eq!(path.parent().unwrap(), t.path().join("log"));
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        assert!(name.ends_with(".log"), "{name}");
        assert_eq!(name.len(), "2026-10-29_123456.log".len(), "{name}");
        assert!(path.is_file());
    }

    #[test]
    fn lines_are_timestamped_and_kinded() {
        let t = TempDir::new("log-lines");
        let mut log = SessionLog::open(&home_in(&t), Redactor::default(), 1 << 20, 1 << 22);
        log.line("CMD", "echo bonjour");
        log.line("OUT", "ligne 1\r\nligne 2\n");
        log.line("END", "");
        let content = read_log(&log);
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 4, "{content}");
        for l in &lines {
            assert!(l.starts_with('['), "{l}");
            assert_eq!(&l[21..23], "] ", "{l}");
            assert!(l[1..21].ends_with('Z'), "{l}");
        }
        assert!(lines[0].ends_with("] CMD echo bonjour"));
        assert!(lines[1].ends_with("] OUT ligne 1"));
        assert!(lines[2].ends_with("] OUT ligne 2"));
        assert!(lines[3].ends_with("] END"));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn every_line_is_redacted() {
        let t = TempDir::new("log-redact");
        let mut log = SessionLog::open(
            &home_in(&t),
            Redactor::new(&["sk-canari-123456"]),
            1 << 20,
            1 << 22,
        );
        log.line("CMD", "curl -H 'Authorization: Bearer sk-canari-123456'");
        log.line("OUT", "a\nsk-canari-123456\nb");
        log.line("sk-canari-123456", "type masqué");
        let content = read_log(&log);
        assert!(!content.contains("sk-canari-123456"), "{content}");
        assert_eq!(content.matches(MASK).count(), 3, "{content}");
    }

    #[test]
    fn file_is_capped_with_a_single_marker_line() {
        let t = TempDir::new("log-cap");
        let file_max = 1024;
        let mut log = SessionLog::open(&home_in(&t), Redactor::default(), file_max, 1 << 22);
        for i in 0..200 {
            log.line("OUT", &format!("ligne numéro {i} avec du texte"));
        }
        let content = read_log(&log);
        assert!(content.len() as u64 <= file_max, "{}", content.len());
        let markers = content.lines().filter(|l| l.contains("plafonné")).count();
        assert_eq!(markers, 1, "{content}");
        assert!(content.lines().last().unwrap().contains("plafonné"));
        let size = content.len();
        log.line("OUT", "encore");
        assert_eq!(read_log(&log).len(), size);
    }

    #[test]
    fn oversized_line_stops_the_log_with_marker() {
        let t = TempDir::new("log-big-line");
        let mut log = SessionLog::open(&home_in(&t), Redactor::default(), 2048, 1 << 22);
        log.line("OUT", "petit");
        log.line("OUT", &"x".repeat(5000));
        log.line("OUT", "après");
        let content = read_log(&log);
        assert!(content.contains("petit"));
        assert!(!content.contains("xxxx"));
        assert!(!content.contains("après"));
        assert!(content.contains("plafonné"));
        assert!(content.len() <= 2048);
    }

    fn write_old(dir: &Path, name: &str, size: usize) {
        fs::write(dir.join(name), vec![b'a'; size]).unwrap();
    }

    #[test]
    fn rotation_deletes_oldest_until_budget_fits() {
        let t = TempDir::new("log-rotate");
        let dir = t.path().join("log");
        fs::create_dir_all(&dir).unwrap();
        write_old(&dir, "2026-01-01_000000.log", 400);
        write_old(&dir, "2026-01-02_000000.log", 400);
        write_old(&dir, "2026-01-03_000000.log", 400);
        write_old(&dir, "2026-01-03_000000-1.log", 400);
        write_old(&dir, "notes.txt", 5000);
        write_old(&dir, "autre.log", 5000);
        // Existing 1600 + a full new file (500) must fit in 1200: at most 700 may stay,
        // so only the newest file (same second, suffix -1) survives.
        let log = SessionLog::open(&home_in(&t), Redactor::default(), 500, 1200);
        assert!(!dir.join("2026-01-01_000000.log").exists());
        assert!(!dir.join("2026-01-02_000000.log").exists());
        assert!(!dir.join("2026-01-03_000000.log").exists());
        assert!(dir.join("2026-01-03_000000-1.log").exists());
        assert!(dir.join("notes.txt").exists(), "fichier étranger supprimé");
        assert!(dir.join("autre.log").exists(), "fichier étranger supprimé");
        assert!(log.path().unwrap().exists());
    }

    #[test]
    fn rotation_stops_as_soon_as_the_budget_fits() {
        let t = TempDir::new("log-rotate-partial");
        let dir = t.path().join("log");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "2026-01-02_000000.log",
            "2026-01-01_000000.log",
            "2026-01-03_000000.log",
        ] {
            write_old(&dir, name, 400);
        }
        // 1200 + 500 > 1500 → delete the oldest; 800 + 500 fits.
        let _log = SessionLog::open(&home_in(&t), Redactor::default(), 500, 1500);
        assert!(!dir.join("2026-01-01_000000.log").exists());
        assert!(dir.join("2026-01-02_000000.log").exists());
        assert!(dir.join("2026-01-03_000000.log").exists());
    }

    #[test]
    fn total_size_never_exceeds_the_budget_over_many_sessions() {
        let t = TempDir::new("log-rotate-many");
        let dir = t.path().join("log");
        let (file_max, total_max) = (600u64, 2000u64);
        for i in 0..12 {
            // Distinct, increasing stamps (one per day).
            let now = at(1_793_277_296 + i * 86_400);
            let mut log = SessionLog::open_in(&dir, Redactor::default(), file_max, total_max, now);
            for j in 0..50 {
                log.line("OUT", &format!("session {i} ligne {j}"));
            }
            let total: u64 = fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().metadata().unwrap().len())
                .sum();
            assert!(total <= total_max, "session {i} : {total}");
        }
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("2026-11-09")),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("2026-10-29")),
            "{names:?}"
        );
    }

    #[test]
    fn rotation_keeps_everything_under_budget() {
        let t = TempDir::new("log-rotate-ok");
        let dir = t.path().join("log");
        fs::create_dir_all(&dir).unwrap();
        write_old(&dir, "2026-01-01_000000.log", 100);
        write_old(&dir, "2026-01-02_000000.log", 100);
        let _log = SessionLog::open(&home_in(&t), Redactor::default(), 500, 10_000);
        assert!(dir.join("2026-01-01_000000.log").exists());
        assert!(dir.join("2026-01-02_000000.log").exists());
    }

    #[test]
    fn same_second_sessions_get_distinct_files() {
        let t = TempDir::new("log-collide");
        let now = at(1_793_277_296);
        let a = SessionLog::open_in(
            &t.path().join("log"),
            Redactor::default(),
            1 << 20,
            1 << 22,
            now,
        );
        let b = SessionLog::open_in(
            &t.path().join("log"),
            Redactor::default(),
            1 << 20,
            1 << 22,
            now,
        );
        let pa = a.path().unwrap();
        let pb = b.path().unwrap();
        assert_ne!(pa, pb);
        assert_eq!(pa.file_name().unwrap(), "2026-10-29_123456.log");
        assert_eq!(pb.file_name().unwrap(), "2026-10-29_123456-1.log");
    }

    #[test]
    fn ephemeral_home_disables_logging() {
        let t = TempDir::new("log-ephemeral");
        let home = Home {
            root: t.path().to_path_buf(),
            writable: false,
        };
        let mut log = SessionLog::open(&home, Redactor::default(), 1 << 20, 1 << 22);
        assert!(!log.is_enabled());
        assert_eq!(log.path(), None);
        log.line("CMD", "rien");
        assert!(!t.path().join("log").exists());
    }

    #[test]
    fn unwritable_log_dir_is_not_fatal() {
        let t = TempDir::new("log-blocked");
        fs::write(t.path().join("log"), b"un fichier, pas un dossier").unwrap();
        let mut log = SessionLog::open(&home_in(&t), Redactor::default(), 1 << 20, 1 << 22);
        assert!(!log.is_enabled());
        log.line("CMD", "rien");
    }

    #[test]
    fn debug_output_never_shows_the_secrets() {
        let t = TempDir::new("log-debug");
        let log = SessionLog::open(
            &home_in(&t),
            Redactor::new(&["sk-canari-debug-42"]),
            1 << 20,
            1 << 22,
        );
        let debug = format!("{log:?}");
        assert!(!debug.contains("sk-canari-debug-42"), "{debug}");
        assert!(debug.contains("SessionLog"), "{debug}");
        assert!(format!("{:?}", SessionLog::disabled()).contains("SessionLog"));
    }

    #[test]
    fn disabled_log_writes_nothing() {
        let mut log = SessionLog::disabled();
        log.line("CMD", "x");
        assert!(log.path().is_none());
        assert!(!log.is_enabled());
    }

    #[test]
    fn marker_fits_in_reserve() {
        let marker = format!(
            "[{}] LOG {}\n",
            utc_timestamp(SystemTime::now()),
            text::log_capped(u64::MAX)
        );
        assert!((marker.len() as u64) < MARKER_RESERVE, "{}", marker.len());
    }

    #[test]
    fn log_names_are_recognised() {
        assert_eq!(
            parse_log_name("2026-10-29_123456.log"),
            Some(("2026-10-29_123456".to_string(), 0))
        );
        assert_eq!(
            parse_log_name("2026-10-29_123456-12.log"),
            Some(("2026-10-29_123456".to_string(), 12))
        );
        for bad in [
            "2026-10-29_123456.txt",
            "2026-10-29-123456.log",
            "2026-1a-29_123456.log",
            "2026-10-29_123456-.log",
            "2026-10-29_123456x.log",
            "notes.log",
            "é026-10-29_123456.log",
        ] {
            assert_eq!(parse_log_name(bad), None, "{bad}");
        }
    }
}
