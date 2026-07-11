//! Generic private EQB `/ws/frames` measurement client.
//!
//! Operator values are supplied at runtime. The program prints aggregate JSON
//! only; optional per-frame timing rows must be written outside the checkout.

use futures_util::StreamExt;
use serde::Serialize;
use std::{
    env, fs,
    io::{self, Write},
    os::unix::{fs::MetadataExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

#[derive(Debug)]
struct Config {
    url: String,
    origin: String,
    cookie_file: PathBuf,
    raw_output: Option<PathBuf>,
    duration: Duration,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    elapsed_ms: f64,
    frame_counter: u64,
    payload_bytes: usize,
    png_bytes: usize,
}

#[derive(Debug, Serialize, PartialEq)]
struct Summary {
    observation_seconds: f64,
    delivered_frames: usize,
    delivered_fps: f64,
    first_frame_counter: Option<u64>,
    last_frame_counter: Option<u64>,
    frame_counter_gaps: u64,
    non_monotonic_frames: u64,
    disconnects: u64,
    websocket_payload_bytes: u64,
    png_bytes: u64,
    payload_mbps: f64,
    png_mbps: f64,
    interarrival_samples: usize,
    interarrival_p50_ms: Option<f64>,
    interarrival_p95_ms: Option<f64>,
    interarrival_max_ms: Option<f64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_args(env::args().skip(1))?;
    require_private_file(&config.cookie_file, "cookie file")?;
    if let Some(path) = &config.raw_output {
        require_output_outside_checkout(path)?;
    }
    let cookie = read_cookie_header(&config.cookie_file)?;

    let mut request = config.url.clone().into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", config.origin.parse()?);
    request.headers_mut().insert("Cookie", cookie.parse()?);
    let (mut socket, _) = connect_async(request).await?;

    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + config.duration;
    let mut samples = Vec::new();
    let mut disconnects = 0;
    loop {
        let next = tokio::time::timeout_at(deadline, socket.next()).await;
        match next {
            Err(_) => break,
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes)))) => {
                if bytes.len() < 9 {
                    continue;
                }
                let prefix: [u8; 8] = bytes[..8].try_into().expect("length checked");
                samples.push(Sample {
                    elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
                    frame_counter: u64::from_le_bytes(prefix),
                    payload_bytes: bytes.len(),
                    png_bytes: bytes.len() - 8,
                });
            }
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))))
            | Ok(Some(Err(_)))
            | Ok(None) => {
                disconnects = 1;
                break;
            }
            Ok(Some(Ok(_))) => {}
        }
    }

    let observation_seconds = started.elapsed().as_secs_f64();
    if let Some(path) = &config.raw_output {
        write_raw(path, &samples)?;
    }
    let summary = summarize(&samples, observation_seconds, disconnects);
    serde_json::to_writer(io::stdout().lock(), &summary)?;
    println!();
    Ok(())
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut url = None;
    let mut origin = None;
    let mut cookie_file = None;
    let mut raw_output = None;
    let mut seconds = 60_u64;
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--url" => url = Some(value),
            "--origin" => origin = Some(value),
            "--cookie-file" => cookie_file = Some(PathBuf::from(value)),
            "--raw-output" => raw_output = Some(PathBuf::from(value)),
            "--seconds" => {
                seconds = value
                    .parse::<u64>()
                    .map_err(|_| "--seconds must be an integer".to_string())?;
                if seconds == 0 {
                    return Err("--seconds must be greater than zero".into());
                }
            }
            _ => return Err(format!("unknown argument: {flag}")),
        }
    }
    Ok(Config {
        url: url.ok_or("--url is required")?,
        origin: origin.ok_or("--origin is required")?,
        cookie_file: cookie_file.ok_or("--cookie-file is required")?,
        raw_output,
        duration: Duration::from_secs(seconds),
    })
}

fn require_private_file(path: &Path, label: &str) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.mode() & 0o777 != 0o600 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{label} must be a regular 0600 file"),
        ));
    }
    Ok(())
}

fn require_output_outside_checkout(path: &Path) -> io::Result<()> {
    let checkout = fs::canonicalize(env::current_dir()?)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)?;
    if parent.starts_with(checkout) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "raw output must be outside the checkout",
        ));
    }
    Ok(())
}

fn read_cookie_header(path: &Path) -> io::Result<String> {
    let contents = fs::read_to_string(path)?;
    let cookies = contents
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            (fields.len() >= 7).then(|| format!("{}={}", fields[5], fields[6]))
        })
        .collect::<Vec<_>>();
    if cookies.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cookie file contains no Netscape-format cookies",
        ));
    }
    Ok(cookies.join("; "))
}

fn write_raw(path: &Path, samples: &[Sample]) -> io::Result<()> {
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    writeln!(output, "elapsed_ms,frame_counter,payload_bytes,png_bytes")?;
    for sample in samples {
        writeln!(
            output,
            "{:.6},{},{},{}",
            sample.elapsed_ms, sample.frame_counter, sample.payload_bytes, sample.png_bytes
        )?;
    }
    Ok(())
}

fn summarize(samples: &[Sample], observation_seconds: f64, disconnects: u64) -> Summary {
    let mut gaps = 0_u64;
    let mut non_monotonic = 0_u64;
    for pair in samples.windows(2) {
        let previous = pair[0].frame_counter;
        let current = pair[1].frame_counter;
        if current <= previous {
            non_monotonic += 1;
        } else if current > previous + 1 {
            gaps += current - previous - 1;
        }
    }
    let mut intervals = samples
        .windows(2)
        .map(|pair| pair[1].elapsed_ms - pair[0].elapsed_ms)
        .collect::<Vec<_>>();
    intervals.sort_by(f64::total_cmp);
    let payload_bytes = samples
        .iter()
        .map(|sample| sample.payload_bytes as u64)
        .sum();
    let png_bytes = samples.iter().map(|sample| sample.png_bytes as u64).sum();
    let safe_seconds = observation_seconds.max(f64::EPSILON);
    Summary {
        observation_seconds,
        delivered_frames: samples.len(),
        delivered_fps: samples.len() as f64 / safe_seconds,
        first_frame_counter: samples.first().map(|sample| sample.frame_counter),
        last_frame_counter: samples.last().map(|sample| sample.frame_counter),
        frame_counter_gaps: gaps,
        non_monotonic_frames: non_monotonic,
        disconnects,
        websocket_payload_bytes: payload_bytes,
        png_bytes,
        payload_mbps: payload_bytes as f64 * 8.0 / safe_seconds / 1_000_000.0,
        png_mbps: png_bytes as f64 * 8.0 / safe_seconds / 1_000_000.0,
        interarrival_samples: intervals.len(),
        interarrival_p50_ms: percentile(&intervals, 0.50),
        interarrival_p95_ms: percentile(&intervals, 0.95),
        interarrival_max_ms: intervals.last().copied(),
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let index = ((sorted.len() - 1) as f64 * quantile).ceil() as usize;
    sorted.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_reports_gaps_non_monotonic_intervals_and_bandwidth() {
        let samples = vec![
            sample(0.0, 10, 108),
            sample(10.0, 12, 208),
            sample(30.0, 11, 308),
            sample(60.0, 13, 408),
        ];
        let summary = summarize(&samples, 2.0, 1);
        assert_eq!(summary.delivered_frames, 4);
        assert_eq!(summary.delivered_fps, 2.0);
        assert_eq!(summary.frame_counter_gaps, 2);
        assert_eq!(summary.non_monotonic_frames, 1);
        assert_eq!(summary.websocket_payload_bytes, 1032);
        assert_eq!(summary.png_bytes, 1000);
        assert_eq!(summary.interarrival_p50_ms, Some(20.0));
        assert_eq!(summary.interarrival_p95_ms, Some(30.0));
        assert_eq!(summary.disconnects, 1);
    }

    #[test]
    fn summary_handles_empty_and_single_frame_windows() {
        let empty = summarize(&[], 60.0, 0);
        assert_eq!(empty.delivered_frames, 0);
        assert_eq!(empty.interarrival_p50_ms, None);
        let single = summarize(&[sample(4.0, 7, 20)], 1.0, 0);
        assert_eq!(single.first_frame_counter, Some(7));
        assert_eq!(single.interarrival_samples, 0);
    }

    fn sample(elapsed_ms: f64, frame_counter: u64, payload_bytes: usize) -> Sample {
        Sample {
            elapsed_ms,
            frame_counter,
            payload_bytes,
            png_bytes: payload_bytes - 8,
        }
    }
}
